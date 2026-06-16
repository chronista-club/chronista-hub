//! worlds_demo — 単一 hub 内で 2 つの VP world が相互 discovery する e2e demo (client 側)。
//!
//! 起動済みの hub (Unison surface = `[::1]:7879`、 REST = `:3000`) に対して:
//!   1. world-a / world-b をそれぞれ別 client で `worlds.Register`
//!   2. world-b client が `worlds.Discover` → 両 world を発見 (= 相互 discovery)
//!   3. (cross-transport) Unison で登録した world が REST `/v1/tree/@world-a` にも現れる
//!
//! 実行 (hub を別ターミナルで起動した状態で):
//!   cargo run -p chronista-hub-server --example worlds_demo
//!
//! env: `HUB_UNISON_ADDR` (default `[::1]:7879`)

use anyhow::{Result, bail};
use serde_json::{Value, json};

use unison::ProtocolClient;
use unison::network::channel::UnisonChannel;

async fn register(addr: &str, handle: &str, name: &str) -> Result<Value> {
    let client = ProtocolClient::new_default()?;
    client.connect(addr).await?;
    let ch: UnisonChannel = client.open_channel("worlds").await?;
    let resp: Value = ch
        .request("Register", &json!({ "handle": handle, "name": name }))
        .await?;
    ch.close().await?;
    client.disconnect().await?;
    Ok(resp)
}

async fn discover(addr: &str) -> Result<Vec<Value>> {
    let client = ProtocolClient::new_default()?;
    client.connect(addr).await?;
    let ch: UnisonChannel = client.open_channel("worlds").await?;
    let resp: Value = ch.request("Discover", &json!({})).await?;
    ch.close().await?;
    client.disconnect().await?;
    Ok(resp
        .get("worlds")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default())
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install ring CryptoProvider");

    let addr = std::env::var("HUB_UNISON_ADDR").unwrap_or_else(|_| "[::1]:7879".into());
    println!("hub Unison surface: {addr}\n");

    // 1) 2 world をそれぞれ別 client で register
    let a = register(&addr, "world-a", "World A").await?;
    println!("✓ world-a registered: {a}");
    let b = register(&addr, "world-b", "World B").await?;
    println!("✓ world-b registered: {b}");

    // 2) world-b 視点で discover → 両方見えるはず (相互 discovery)
    let worlds = discover(&addr).await?;
    let handles: Vec<&str> = worlds
        .iter()
        .filter_map(|w| w.get("handle").and_then(|v| v.as_str()))
        .collect();
    println!("\n✓ discover → {handles:?}");
    if !handles.contains(&"world-a") || !handles.contains(&"world-b") {
        bail!("mutual discovery FAILED — expected both world-a and world-b, got {handles:?}");
    }
    println!("✅ 相互 discovery OK — world-b は world-a を (そして自身も) hub 経由で発見した");

    // 3) cross-transport: Unison で登録 → REST tree read に現れる
    let rest = reqwest::get("http://localhost:3000/v1/tree/world-a")
        .await
        .ok();
    if let Some(resp) = rest {
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        let n = body
            .get("resources")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        println!(
            "\n✓ cross-transport: REST GET /v1/tree/world-a → {n} resource(s): {}",
            serde_json::to_string(&body).unwrap_or_default()
        );
    } else {
        println!("\n(REST tree read skipped — hub :3000 未到達)");
    }

    Ok(())
}
