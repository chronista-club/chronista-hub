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

/// hub に接続する。 `cred` があれば connect_with_credential で federation auth を通す
/// (ADR-020 §S3)。 None なら素の connect (permissive hub のみ受理)。
async fn connect(client: &ProtocolClient, addr: &str, cred: Option<&[u8]>) -> Result<()> {
    match cred {
        Some(c) => client.connect_with_credential(addr, c).await?,
        None => client.connect(addr).await?,
    }
    Ok(())
}

async fn register(
    addr: &str,
    wld_id: &str,
    handle: &str,
    name: &str,
    endpoints: &[&str],
    cred: Option<&[u8]>,
) -> Result<Value> {
    let client = ProtocolClient::new_default()?;
    connect(&client, addr, cred).await?;
    let ch: UnisonChannel = client.open_channel("worlds").await?;
    let resp: Value = ch
        .request(
            "Register",
            &json!({ "wld_id": wld_id, "handle": handle, "name": name, "endpoints": endpoints }),
        )
        .await?;
    ch.close().await?;
    client.disconnect().await?;
    Ok(resp)
}

async fn discover(addr: &str, cred: Option<&[u8]>) -> Result<Vec<Value>> {
    let client = ProtocolClient::new_default()?;
    connect(&client, addr, cred).await?;
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
    // HUB_CRED があれば federation auth credential (Creo ID JWT 等) として提示 (ADR-020 §S3)。
    let cred = std::env::var("HUB_CRED").ok().filter(|s| !s.is_empty());
    let cred = cred.as_deref().map(str::as_bytes);
    println!(
        "hub Unison surface: {addr} (auth: {})\n",
        if cred.is_some() { "credential" } else { "none" }
    );

    // 1) 2 world を register (wld_id = location 独立 routing key、 endpoints = direct 候補)
    let a = register(
        &addr,
        "wld_demoA",
        "world-a",
        "World A",
        &["[::1]:32000"],
        cred,
    )
    .await?;
    println!("✓ world-a registered: {a}");
    let b = register(
        &addr,
        "wld_demoB",
        "world-b",
        "World B",
        &["[::1]:32001"],
        cred,
    )
    .await?;
    println!("✓ world-b registered: {b}");

    // 2) world-b 視点で discover → 両方見えるはず (相互 discovery)
    let worlds = discover(&addr, cred).await?;
    let handles: Vec<&str> = worlds
        .iter()
        .filter_map(|w| w.get("handle").and_then(|v| v.as_str()))
        .collect();
    println!("\n✓ discover → {handles:?}");
    if !handles.contains(&"world-a") || !handles.contains(&"world-b") {
        bail!("mutual discovery FAILED — expected both world-a and world-b, got {handles:?}");
    }
    println!("✅ 相互 discovery OK — world-b は world-a を (そして自身も) hub 経由で発見した");

    // 2b) S2: wld_id + endpoints が registry に index され Discover で round-trip するか
    let wld_a = worlds
        .iter()
        .find(|w| w.get("handle").and_then(|v| v.as_str()) == Some("world-a"));
    let wld_id_ok =
        wld_a.and_then(|w| w.get("wld_id").and_then(|v| v.as_str())) == Some("wld_demoA");
    let ep_ok = wld_a
        .and_then(|w| w.get("endpoints").and_then(|v| v.as_array()))
        .map(|eps| eps.iter().any(|e| e.as_str() == Some("[::1]:32000")))
        .unwrap_or(false);
    println!("✓ wld_id round-trip: {wld_id_ok} / endpoints round-trip: {ep_ok}");
    if !wld_id_ok || !ep_ok {
        bail!("S2 FAILED — wld_id/endpoints が Discover で復元されない (world-a={wld_a:?})");
    }
    println!("✅ S2 OK — wld_id + endpoints が registry に index され Discover で返った");

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
