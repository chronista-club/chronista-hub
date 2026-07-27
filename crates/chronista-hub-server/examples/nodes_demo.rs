//! nodes_demo — 単一 hub 内で 2 つの VP node が相互 discovery する e2e demo (client 側)。
//!
//! 起動済みの hub (Unison surface = `[::1]:7879`、 REST = `:3000`) に対して:
//!   1. node-a / node-b をそれぞれ別 client で `nodes.Register`
//!   2. node-b client が `nodes.Discover` → 両 node を発見 (= 相互 discovery)
//!   3. (cross-transport) Unison で登録した node が REST `/v1/tree/@node-a` にも現れる
//!
//! 実行 (hub を別ターミナルで起動した状態で):
//!   cargo run -p chronista-hub-server --example nodes_demo
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
    node_id: &str,
    handle: &str,
    name: &str,
    endpoints: &[&str],
    cred: Option<&[u8]>,
) -> Result<Value> {
    let client = ProtocolClient::new_default()?;
    connect(&client, addr, cred).await?;
    let ch: UnisonChannel = client.open_channel("nodes").await?;
    let resp: Value = ch
        .request(
            "Register",
            &json!({ "node_id": node_id, "handle": handle, "name": name, "endpoints": endpoints }),
        )
        .await?;
    ch.close().await?;
    client.disconnect().await?;
    Ok(resp)
}

async fn discover(addr: &str, cred: Option<&[u8]>) -> Result<Vec<Value>> {
    let client = ProtocolClient::new_default()?;
    connect(&client, addr, cred).await?;
    let ch: UnisonChannel = client.open_channel("nodes").await?;
    let resp: Value = ch.request("Discover", &json!({})).await?;
    ch.close().await?;
    client.disconnect().await?;
    Ok(resp
        .get("nodes")
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

    // 1) 2 node を register (node_id = location 独立 routing key、 endpoints = direct 候補)
    let a = register(
        &addr,
        "nd_demoA",
        "node-a",
        "Node A",
        &["[::1]:32000"],
        cred,
    )
    .await?;
    println!("✓ node-a registered: {a}");
    let b = register(
        &addr,
        "nd_demoB",
        "node-b",
        "Node B",
        &["[::1]:32001"],
        cred,
    )
    .await?;
    println!("✓ node-b registered: {b}");

    // 2) node-b 視点で discover → 両方見えるはず (相互 discovery)
    let nodes = discover(&addr, cred).await?;
    let handles: Vec<&str> = nodes
        .iter()
        .filter_map(|w| w.get("handle").and_then(|v| v.as_str()))
        .collect();
    println!("\n✓ discover → {handles:?}");
    if !handles.contains(&"node-a") || !handles.contains(&"node-b") {
        bail!("mutual discovery FAILED — expected both node-a and node-b, got {handles:?}");
    }
    println!("✅ 相互 discovery OK — node-b は node-a を (そして自身も) hub 経由で発見した");

    // 2b) S2: node_id + endpoints が registry に index され Discover で round-trip するか
    let nd_a = nodes
        .iter()
        .find(|w| w.get("handle").and_then(|v| v.as_str()) == Some("node-a"));
    let node_id_ok =
        nd_a.and_then(|w| w.get("node_id").and_then(|v| v.as_str())) == Some("nd_demoA");
    let ep_ok = nd_a
        .and_then(|w| w.get("endpoints").and_then(|v| v.as_array()))
        .map(|eps| eps.iter().any(|e| e.as_str() == Some("[::1]:32000")))
        .unwrap_or(false);
    println!("✓ node_id round-trip: {node_id_ok} / endpoints round-trip: {ep_ok}");
    if !node_id_ok || !ep_ok {
        bail!("S2 FAILED — node_id/endpoints が Discover で復元されない (node-a={nd_a:?})");
    }
    println!("✅ S2 OK — node_id + endpoints が registry に index され Discover で返った");

    // 3) cross-transport: Unison で登録 → REST tree read に現れる
    let rest = reqwest::get("http://localhost:3000/v1/tree/node-a")
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
            "\n✓ cross-transport: REST GET /v1/tree/node-a → {n} resource(s): {}",
            serde_json::to_string(&body).unwrap_or_default()
        );
    } else {
        println!("\n(REST tree read skipped — hub :3000 未到達)");
    }

    Ok(())
}
