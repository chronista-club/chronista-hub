//! quic_probe — hub の QUIC 受付が生きているかを実接続で判定する liveness probe。
//!
//! REST の `/health` は axum(TCP 3000) の生死しか映さず、 QUIC(UDP 7879/publish 12879)
//! listener の死を検知できない (issue #35: rootless podman の pasta UDP forward が
//! 連続稼働で劣化し、 QUIC 受付だけ停止する事象があった)。 この probe は **publish port
//! への実 QUIC 接続** で pasta 経路ごと生死をテストする:
//!
//! - connect 成功 → exit 0
//! - connect 失敗 / timeout → exit 1
//!
//! cert 検証は [`TrustAnchors::SkipVerification`] で skip する。 probe の関心は
//! 「QUIC handshake が成立するか」だけで、 相手が誰か (cert) は問わない。 これにより
//! `localhost:12879` (SNI=localhost、 cert SAN=hub.chronista.club) でも SNI 不一致で
//! 弾かれずに経路を叩ける。 **auth は不要** (channel を open せず connect で止める)。
//!
//! 運用: host の systemd timer から `podman run --rm --network=host <image>
//! quic_probe` で回し、 exit 1 なら hub service を restart する (OnFailure)。
//!
//!     HUB_ADDR=localhost:12879 PROBE_TIMEOUT_SECS=10 \
//!       cargo run -p chronista-hub-server --example quic_probe

use anyhow::{Result, anyhow};
use std::time::Duration;
use unison::ProtocolClient;
use unison::network::{QuicClient, TrustAnchors};

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let addr = std::env::var("HUB_ADDR").unwrap_or_else(|_| "localhost:12879".into());
    let timeout_secs = std::env::var("PROBE_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    // 一過性の瞬断で restart を誘発しないよう、 全リトライ失敗で初めて exit 1。
    let retries = std::env::var("PROBE_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3u32);

    match probe(&addr, timeout_secs, retries).await {
        Ok(()) => {
            println!("QUIC probe OK: {addr}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("QUIC probe FAILED after {retries} attempts: {addr}: {e}");
            std::process::exit(1);
        }
    }
}

/// publish port へ QUIC 接続を張り handshake の成立を確認する。 `retries` 回まで
/// 試し、 いずれか成功で OK。 全敗で最後のエラーを返す (間に 2s のバックオフ)。
async fn probe(addr: &str, timeout_secs: u64, retries: u32) -> Result<()> {
    let mut last_err = anyhow!("no attempt made");
    for attempt in 0..retries.max(1) {
        match try_connect(addr, timeout_secs).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e;
                if attempt + 1 < retries {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
    Err(last_err)
}

/// QUIC 接続を 1 回試す。 handshake が成立すれば十分 (channel は open しない =
/// auth 不要)。 cert は [`TrustAnchors::SkipVerification`] で不問。
async fn try_connect(addr: &str, timeout_secs: u64) -> Result<()> {
    let quic = QuicClient::builder()
        .trust_anchors(TrustAnchors::SkipVerification)
        .build()?;
    let client = ProtocolClient::new(quic);

    tokio::time::timeout(Duration::from_secs(timeout_secs), client.connect(addr))
        .await
        .map_err(|_| anyhow!("connect timed out after {timeout_secs}s"))??;

    // 後片付けの失敗は probe 結果に影響させない。
    client.disconnect().await.ok();
    Ok(())
}
