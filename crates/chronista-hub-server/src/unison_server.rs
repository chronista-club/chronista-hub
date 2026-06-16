//! Unison (QUIC) surface — world registry / discovery channel。
//!
//! REST (axum) と **同一 tokio runtime** で動く薄い層 (`spawn_listen`)。
//! resource ingestion (`/v1/events`) と tree read (`/v1/tree`) は REST 据え置きで、
//! ここでは「world が自分を登録 / 互いを発見する」discovery だけを Unison channel で提供する。
//!
//! channels:
//! - `unison.discovery` — server 自身の protocol.kdl を hash 付きで返す (enable_discovery、組込)。
//! - `worlds` — `Register` (vp-world を registry へ upsert) / `Discover` (registry 一覧)。
//!
//! MVP scope: 単一 hub 内の相互 discovery。multi-hub federation は ADR-018 (defer)。
//!
//! 注意: rustls の process-level CryptoProvider を呼び出し前に install しておくこと
//! (main.rs で `rustls::crypto::ring::default_provider().install_default()`)。
//! quinn と surrealdb/reqwest で provider feature が両立し auto-detect が panic するため。

use anyhow::Result;
use serde_json::{Value, json};

use unison::ServerHandle;
use unison::network::channel::UnisonChannel;
use unison::network::{MessageType, ProtocolServer};

use crate::storage::Storage;

/// hub の Unison protocol schema (channels: unison.discovery + worlds)。
const HUB_PROTOCOL_KDL: &str = include_str!("hub_protocol.kdl");

/// Unison server を spawn し、 axum と同一 runtime で動く [`ServerHandle`] を返す。
/// handle は drop で shutdown するので、 呼び出し側でプロセス生存期間 hold すること。
pub async fn spawn_unison(addr: &str, storage: Storage) -> Result<ServerHandle> {
    let server =
        ProtocolServer::with_identity("chronista-hub", crate::VERSION, "club.chronista.hub");
    server.enable_discovery(HUB_PROTOCOL_KDL).await?;

    server
        .register_channel("worlds", move |_ctx, stream| {
            // register_channel は Fn (接続毎に呼ばれる) なので、 storage は毎回 clone する。
            let storage = storage.clone();
            async move {
                let channel = UnisonChannel::new(stream);
                loop {
                    match channel.recv().await {
                        Ok(msg) if msg.msg_type == MessageType::Request => {
                            let payload = msg.payload_as_value().unwrap_or_default();
                            let reply = handle_worlds(&storage, &msg.method, payload)
                                .await
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
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
            }
        })
        .await;

    let handle = server.spawn_listen(addr).await?;
    tracing::info!(%addr, "Unison surface listening (channels: unison.discovery, worlds)");
    Ok(handle)
}

/// `worlds` channel の method dispatch。
async fn handle_worlds(storage: &Storage, method: &str, payload: Value) -> Result<Value> {
    match method {
        "Register" => {
            let handle = payload
                .get("handle")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("field 'handle' required"))?;
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(handle);
            let registered_at = storage.register_world(handle, name).await?;
            tracing::info!(handle, "world registered via Unison");
            Ok(json!({ "handle": handle, "registered_at": registered_at }))
        }
        "Discover" => {
            let worlds = storage.list_resources_by_type("vp-world").await?;
            let list: Vec<Value> = worlds
                .iter()
                .map(|w| {
                    json!({
                        "handle": w.handle,
                        "name": w.payload.get("name").and_then(|v| v.as_str()).unwrap_or(&w.handle),
                        "registered_at": w.created_at,
                    })
                })
                .collect();
            Ok(json!({ "worlds": list }))
        }
        other => Err(anyhow::anyhow!(
            "unknown method '{other}' on channel 'worlds'"
        )),
    }
}
