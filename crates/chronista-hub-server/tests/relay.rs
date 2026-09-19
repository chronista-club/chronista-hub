//! relay channel (ADR-020 §S4 = universal floor) の QUIC 実疎通テスト。
//!
//! hub の Unison surface を `[::1]:0` で実際に立て、 2 本の client で
//! 「target が `nodes.Register` で registry に載る → source が `relay` を開いて `{to, from}` を
//! 宣言 → 後続 frame が target の server-initiated stream に同順で届く」までを通す。
//! QUIC 経路はそれまで `examples/` にしか無く CI が実行していなかったので、 ここで常時回す。

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::config::UnisonCert;
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::storage::Storage;
use chronista_hub_server::unison_server::spawn_unison;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::timeout;
use unison::ProtocolClient;
use unison::ServerHandle;
use unison::network::channel::UnisonChannel;

const WAIT: Duration = Duration::from_secs(5);

async fn spawn_hub() -> Result<(ServerHandle, String)> {
    // aws-lc-rs と ring が両方 graph に居て rustls が provider を auto-detect できないので、
    // main.rs と同じく ring を明示する。 test は同一 process で並走するので二重 install は無視。
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = connect_mem("chronista", "hub").await?;
    run_pending_migrations(
        &db,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations"),
    )
    .await?;
    let handle = spawn_unison(
        "[::1]:0",
        UnisonCert::Dev,
        None,
        Arc::new(StubVerifier),
        false,
        Storage::new(db),
    )
    .await?;
    let local = handle.local_addr();
    let addr = format!("[{}]:{}", local.ip(), local.port());
    Ok((handle, addr))
}

/// frame の JSON payload を取り出す。
fn payload(msg: &unison::network::ProtocolMessage) -> Value {
    msg.payload_as_value().unwrap_or_default()
}

/// target は Register した `nodes` channel を開いたまま保持する (閉じると registry から外れる)。
#[tokio::test]
async fn relay_forwards_frames_to_registered_target_in_order() -> Result<()> {
    const N: i64 = 50;
    let (handle, addr) = spawn_hub().await?;

    // ─── target: relay の受け口を connect 前に登録し、 nodes.Register で registry に載る ──
    let (tx, mut rx) = mpsc::unbounded_channel::<(String, Value)>();
    let target = ProtocolClient::insecure_localhost()?;
    target
        .register_server_channel("relay", move |stream| {
            let tx = tx.clone();
            async move {
                while let Ok(msg) = stream.recv_frame().await {
                    let _ = tx.send((msg.method.clone(), payload(&msg)));
                }
                Ok(())
            }
        })
        .await;
    target.connect(&addr).await?;
    let nodes: UnisonChannel = target.open_channel("nodes").await?;
    let _: Value = nodes
        .request(
            "Register",
            &json!({ "node_id": "nd_target", "handle": "relay-target", "name": "T", "endpoints": [] }),
        )
        .await?;

    // ─── source: relay を開いて宛先を宣言 → established を待つ ──
    let source = ProtocolClient::insecure_localhost()?;
    source.connect(&addr).await?;
    let relay: UnisonChannel = source.open_channel("relay").await?;
    relay
        .send_event("open", &json!({ "to": "nd_target", "from": "nd_source" }))
        .await?;
    let status = timeout(WAIT, relay.recv()).await??;
    assert_eq!(
        payload(&status)["status"],
        "established",
        "registry に居る target へは stream が確立するべき: {:?}",
        payload(&status)
    );

    // ─── target には先頭で送信元宣言 {from} が届く ──
    let (method, open) = timeout(WAIT, rx.recv()).await?.expect("target stream");
    assert_eq!(method, "open");
    assert_eq!(open["from"], "nd_source");

    // ─── source → target: N 件が取りこぼし無く同順で届く ──
    for i in 0..N {
        relay.send_event("msg", &json!({ "seq": i })).await?;
    }
    let mut got = Vec::new();
    for _ in 0..N {
        let (_, v) = timeout(WAIT, rx.recv()).await?.expect("frame");
        got.push(v["seq"].as_i64().unwrap_or(-1));
    }
    assert_eq!(
        got,
        (0..N).collect::<Vec<_>>(),
        "relay は同順・全件で forward するべき"
    );

    relay.close().await?;
    nodes.close().await?;
    source.disconnect().await?;
    target.disconnect().await?;
    handle.shutdown().await?;
    Ok(())
}

/// registry に居ない宛先へは `offline` を返す (ADR-020 D3-c)。
#[tokio::test]
async fn relay_reports_offline_for_unregistered_target() -> Result<()> {
    let (handle, addr) = spawn_hub().await?;

    let source = ProtocolClient::insecure_localhost()?;
    source.connect(&addr).await?;
    let relay: UnisonChannel = source.open_channel("relay").await?;
    relay
        .send_event("open", &json!({ "to": "nd_nobody", "from": "nd_source" }))
        .await?;
    let status = timeout(WAIT, relay.recv()).await??;
    assert_eq!(payload(&status)["status"], "offline");
    assert_eq!(payload(&status)["detail"], "nd_nobody");

    source.disconnect().await?;
    handle.shutdown().await?;
    Ok(())
}
