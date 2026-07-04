//! registry_gc — 公開 hub の stale vp-world entry を Unregister で掃除する (throwaway)。
//!
//! DEPLOY.md の「registry の delete/expire path」の手動版。owner guard (ADR-020 §S5) が
//! そのまま効く: 未認証だと owner-less entry しか消えず、認証すると自分の entry も消せる。
//! 他人の owned entry はどちらでも消えない (storage 側 guard、removed=0)。
//!
//! 使い方:
//!
//!     # owner-less (permissive 時代の legacy) を無認証で掃除
//!     WLD_IDS=wld_a,wld_b HUB_ADDR=hub.chronista.club:12879 \
//!       cargo run -p chronista-hub-server --example registry_gc
//!
//!     # 自分の entry も掃除 (HUB_TOKEN_FILE = raw JWT を置いたファイル)
//!     WLD_IDS=wld_c HUB_TOKEN_FILE=/path/to/token \
//!       cargo run -p chronista-hub-server --example registry_gc
//!
//! 最後に Discover を打ち、この principal から見える残存 world 一覧を出す (掃除後検証)。

use anyhow::Result;
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
    let wld_ids: Vec<String> = std::env::var("WLD_IDS")
        .map(|s| {
            s.split(',')
                .map(|w| w.trim().to_string())
                .filter(|w| !w.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if wld_ids.is_empty() {
        anyhow::bail!("WLD_IDS (comma 区切り) を指定してください");
    }

    let quic = QuicClient::builder()
        .trust_anchors(TrustAnchors::System)
        .build()?;
    let client = ProtocolClient::new(quic);

    match std::env::var("HUB_TOKEN_FILE") {
        Ok(path) if !path.is_empty() => {
            let token = std::fs::read_to_string(&path)?;
            client
                .connect_with_credential(&addr, token.trim().as_bytes())
                .await?;
            println!("✓ connect + authenticate OK (credential 提示、principal 有り)");
        }
        _ => {
            client.connect(&addr).await?;
            println!("✓ connect OK (未認証 = owner-less entry のみ削除可)");
        }
    }

    let ch: UnisonChannel = client.open_channel("worlds").await?;
    for wld_id in &wld_ids {
        let res: Value = ch
            .request("Unregister", &json!({ "wld_id": wld_id }))
            .await?;
        // handler は Err を {"error": "..."} の正常 reply に変換して返す — 見逃さない。
        if let Some(err) = res.get("error").and_then(|v| v.as_str()) {
            println!("✗ Unregister {wld_id} → error: {err}");
            continue;
        }
        let removed = res.get("removed").and_then(|v| v.as_u64()).unwrap_or(0);
        let mark = if removed > 0 { "🧹" } else { "–" };
        println!("{mark} Unregister {wld_id} → removed={removed}");
    }

    let disc: Value = ch.request("Discover", &json!({})).await?;
    if let Some(err) = disc.get("error").and_then(|v| v.as_str()) {
        println!("✗ Discover → error: {err}");
        ch.close().await?;
        client.disconnect().await?;
        return Ok(());
    }
    let worlds = disc.get("worlds").and_then(|v| v.as_array());
    println!(
        "\n残存 world ({} 件、この principal から見える範囲):",
        worlds.map(|w| w.len()).unwrap_or(0)
    );
    if let Some(list) = worlds {
        for w in list {
            println!(
                "  - {} (wld_id={}, registered_at={})",
                w.get("handle").and_then(|v| v.as_str()).unwrap_or("?"),
                w.get("wld_id").and_then(|v| v.as_str()).unwrap_or("?"),
                w.get("registered_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
            );
        }
    }

    ch.close().await?;
    client.disconnect().await?;
    Ok(())
}
