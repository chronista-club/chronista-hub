//! Unison 疎通 spike — chronista-hub の dependency graph 内で club-unison
//! (QUIC + KDL discovery) が compile / round-trip するかを検証する。
//!
//! 本実装 (hub に worlds.* channel を載せる) の前に、 Beta 版 (v1.1.0) の地雷を踏む:
//!   - edition 2024 / rust 1.95 で hub 側から build できるか
//!   - rustls provider 衝突 (reqwest=rustls, surrealdb=aws-lc-rs, unison=quinn+ring) が無いか
//!   - axum と同じ tokio runtime で QUIC server を同居できるか (spawn_listen)
//!   - enable_discovery (`unison.discovery`) が hub の文脈で動くか
//!
//! 実行: `cargo run -p chronista-hub-server --example unison_spike`

use anyhow::Result;
use serde_json::json;

use unison::network::channel::UnisonChannel;
use unison::network::discovery::{DISCOVERY_CHANNEL_NAME, GET_PROTOCOL_METHOD, ProtocolDocument};
use unison::network::{MessageType, ProtocolServer};
use unison::ProtocolClient;

/// spike 用最小 KDL — discovery 自身 + echo channel。
const SPIKE_KDL: &str = r#"
protocol "hub-spike" version="0.0.1" {
    namespace "chronista.hub.spike"

    channel "unison.discovery" from="client" lifetime="persistent" {
        request "GetProtocol" {
            field "format" type="string" required=#true
            returns "ProtocolDocument" {
                field "kdl" type="string" required=#true
                field "version" type="string" required=#true
                field "hash" type="string" required=#true
                field "codecs" type="json" required=#true
            }
        }
    }

    channel "echo" from="client" lifetime="persistent" {
        request "Echo" {
            field "payload" type="json" required=#true
            returns "EchoResult" {
                field "echoed" type="json" required=#true
            }
        }
    }
}
"#;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // rustls の process-level CryptoProvider を明示 install。
    // hub graph には aws-lc-rs (surrealdb/reqwest) と ring (quinn/club-unison) が両方入り、
    // rustls 0.23 が auto-detect できず panic する。 quinn が使う ring を default に選ぶ。
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("install ring CryptoProvider");

    // --- server: discovery + echo channel, ephemeral port ---
    let server = ProtocolServer::with_identity("hub-spike", "0.0.1", "chronista.hub.spike");
    server.enable_discovery(SPIKE_KDL).await?;
    server
        .register_channel("echo", |_ctx, stream| async move {
            let channel = UnisonChannel::new(stream);
            loop {
                match channel.recv().await {
                    Ok(msg) if msg.msg_type == MessageType::Request && msg.method == "Echo" => {
                        let payload = msg.payload_as_value().unwrap_or_default();
                        let inner = payload.get("payload").cloned().unwrap_or(json!(null));
                        let reply = json!({ "echoed": inner });
                        if channel
                            .send_response(msg.id, &msg.method, &reply)
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => continue,
                    Err(e) if e.is_normal_close() => return Ok(()),
                    Err(e) => return Err(e),
                }
            }
            Ok(())
        })
        .await;

    let handle = server.spawn_listen("[::1]:0").await?;
    let local = handle.local_addr();
    let addr = format!("[{}]:{}", local.ip(), local.port());
    println!("spike server listening on {addr}");

    // --- client: discovery round-trip + echo round-trip ---
    let client = ProtocolClient::new_default()?;
    client.connect(&addr).await?;

    // 1) unison.discovery (self-description は enable_discovery だけで動くはず)
    let dchan: UnisonChannel = client.open_channel(DISCOVERY_CHANNEL_NAME).await?;
    let dval = dchan
        .request(GET_PROTOCOL_METHOD, &json!({ "format": "kdl+hash" }))
        .await?;
    let doc: ProtocolDocument = serde_json::from_value(dval)?;
    assert_eq!(doc.version, "0.0.1", "discovery version mismatch");
    assert_eq!(doc.hash.len(), 64, "discovery hash should be sha256 hex");
    assert!(doc.kdl.contains("hub-spike"), "kdl should echo protocol name");
    println!("✓ discovery OK: version={} hash={}…", doc.version, &doc.hash[..16]);
    dchan.close().await?;

    // 2) echo channel round-trip
    let echan = client.open_channel("echo").await?;
    let resp: serde_json::Value = echan
        .request("Echo", &json!({ "payload": { "hello": "hub" } }))
        .await?;
    let echoed = resp
        .get("echoed")
        .and_then(|v| v.get("hello"))
        .and_then(|v| v.as_str());
    assert_eq!(echoed, Some("hub"), "echo payload should round-trip");
    println!("✓ echo round-trip OK: {resp}");
    echan.close().await?;

    client.disconnect().await?;
    handle.shutdown().await?;
    println!("✅ SPIKE PASS — club-unison は hub の依存グラフ内で QUIC round-trip 可能");
    Ok(())
}
