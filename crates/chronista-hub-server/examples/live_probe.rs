//! live_probe — 公開 hub へ register/discover を通す疎通確認 (throwaway)。
//!
//! trust mode は `HUB_CERT` env の有無で分岐する:
//!
//! - **System trust (推奨, pin なし)** — `HUB_CERT` 未設定。client は
//!   [`TrustAnchors::System`] (webpki-roots = Mozilla bundle、ISRG Root 含む) で
//!   検証する。hub が Let's Encrypt 等の実 CA cert を `CertSource::FromFile` で
//!   出している前提 (ADR-020 §S1、cert 永続化 = 実 CA path)。pin 不要なので
//!   cert が回っても client 無変更:
//!
//!       HUB_ADDR=hub.chronista.club:12879 \
//!         cargo run -p chronista-hub-server --example live_probe
//!
//! - **Custom pin (self-signed hub 用、後方互換)** — `HUB_CERT=<cert DER path>`。
//!   hub が self-signed cert を出している間 (cert 切替前) はこちら。exported
//!   cert DER を [`TrustAnchors::Custom`] に pin する:
//!
//!       HUB_CERT=<cert DER path> HUB_ADDR=hub.chronista.club:12879 \
//!         cargo run -p chronista-hub-server --example live_probe
//!
//! ⚠️ System trust は SNI 名 ↔ cert SAN を照合するため、`HUB_ADDR` は必ず
//! **hostname** で指定する (生 IP だと SNI 不一致で handshake 失敗)。

use anyhow::Result;
use rustls::pki_types::CertificateDer;
use serde_json::{Value, json};
use unison::ProtocolClient;
use unison::network::channel::UnisonChannel;
use unison::network::{QuicClient, TrustAnchors};

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let addr = std::env::var("HUB_ADDR").unwrap_or_else(|_| "hub.chronista.club:12879".into());

    // HUB_CERT があれば Custom pin (self-signed hub)、無ければ System trust (実 CA)。
    let trust = match std::env::var("HUB_CERT") {
        Ok(cert_path) if !cert_path.is_empty() => {
            let der = std::fs::read(&cert_path)?;
            println!(
                "hub={addr}  cert={cert_path} ({}B) を TrustAnchors::Custom で pin",
                der.len()
            );
            TrustAnchors::Custom(vec![CertificateDer::from(der)])
        }
        _ => {
            println!("hub={addr}  TrustAnchors::System (webpki-roots、pin なし = 実 CA 検証)");
            TrustAnchors::System
        }
    };
    println!();

    let quic = QuicClient::builder().trust_anchors(trust).build()?;
    let client = ProtocolClient::new(quic);

    client.connect(&addr).await?;
    println!("✓ QUIC connect OK (TLS handshake 成立 = server cert を信頼)");

    let ch: UnisonChannel = client.open_channel("nodes").await?;
    let reg: Value = ch
        .request(
            "Register",
            &json!({
                "node_id": "nd_liveprobe",
                "handle": "live-probe",
                "name": "Live Probe",
                "endpoints": ["[2400:4150:0:1::1]:32000"]
            }),
        )
        .await?;
    println!("✓ Register → {reg}");

    let disc: Value = ch.request("Discover", &json!({})).await?;
    println!("✓ Discover → {disc}");

    ch.close().await?;
    client.disconnect().await?;
    println!("\n✅ 公開 hub federation 疎通 OK (register + discover round-trip)");
    Ok(())
}
