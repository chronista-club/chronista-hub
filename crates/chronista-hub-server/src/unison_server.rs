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

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::sync::RwLock;

use unison::ServerHandle;
use unison::network::cert::CertSource;
use unison::network::channel::UnisonChannel;
use unison::network::context::ConnectionContext;
use unison::network::{MessageType, ProtocolMessage, ProtocolServer, UnisonStream};

use crate::auth::Verifier;
use crate::config::UnisonCert;
use crate::storage::Storage;

/// hub の Unison protocol schema (channels: unison.discovery + worlds + relay)。
const HUB_PROTOCOL_KDL: &str = include_str!("hub_protocol.kdl");

/// relay 用 transient registry: `wld_id → 当該 world の connection ctx` (ADR-020 §S4 / D1)。
///
/// **durable 0 = D1**: connection ごとに `worlds.Register` で挿入、 connection close で除去。
/// substrate (club-unison) には connection lookup を持ち込まず、 hub が application で保持する
/// (剃刀 — `design/server-initiated-stream.md` §7)。 値の `ctx` は server 側で `conn` が
/// set 済みなので `ctx.open_server_stream(...)` で target へ reliable stream を開ける。
type RelayRegistry = Arc<RwLock<HashMap<String, Arc<ConnectionContext>>>>;

/// source への status frame (`{status, detail}`)。 status = established / offline / error。
fn relay_status(status: &str, detail: &str) -> ProtocolMessage {
    ProtocolMessage::new_with_json(
        0,
        "status".to_string(),
        MessageType::Event,
        json!({ "status": status, "detail": detail }),
    )
    .unwrap_or_else(|_| {
        ProtocolMessage::new_encoded(0, "status".to_string(), MessageType::Event, Vec::new())
    })
}

/// target への open 宣言 frame (`{from}` = 送信元 wld_id)。
fn relay_open(from: &str) -> ProtocolMessage {
    ProtocolMessage::new_with_json(
        0,
        "open".to_string(),
        MessageType::Event,
        json!({ "from": from }),
    )
    .unwrap_or_else(|_| {
        ProtocolMessage::new_encoded(0, "open".to_string(), MessageType::Event, Vec::new())
    })
}

/// `worlds.Register`/`Unregister` の結果を relay registry へ反映する (wld_id keyed、 D1 transient)。
/// `registered` は当該 connection が登録した wld_id 群 (close 時の cleanup 用)。
async fn sync_relay_registry(
    registry: &RelayRegistry,
    ctx: &Arc<ConnectionContext>,
    method: &str,
    reply: &Value,
    registered: &mut Vec<String>,
) {
    let wld = reply.get("wld_id").and_then(|v| v.as_str());
    match (method, wld) {
        ("Register", Some(w)) => {
            registry
                .write()
                .await
                .insert(w.to_string(), Arc::clone(ctx));
            if !registered.iter().any(|x| x == w) {
                registered.push(w.to_string());
            }
        }
        ("Unregister", Some(w)) => {
            registry.write().await.remove(w);
            registered.retain(|x| x != w);
        }
        _ => {}
    }
}

/// Unison server を spawn し、 axum と同一 runtime で動く [`ServerHandle`] を返す。
/// handle は drop で shutdown するので、 呼び出し側でプロセス生存期間 hold すること。
pub async fn spawn_unison(
    addr: &str,
    cert: UnisonCert,
    cert_out: Option<String>,
    verifier: Arc<dyn Verifier>,
    auth_required: bool,
    storage: Storage,
) -> Result<ServerHandle> {
    let server =
        ProtocolServer::with_identity("chronista-hub", crate::VERSION, "club.chronista.hub");
    server.enable_discovery(HUB_PROTOCOL_KDL).await?;

    // connection-level auth (ADR-020 §S3): credential = Creo ID JWT (bytes) を
    // verify_user_token で検証 → principal を connection に立てる。 policy 注入のみで、
    // mechanism (unison.auth channel) は club-unison。 enable_auth は opt-in/非破壊。
    {
        let verifier = verifier.clone();
        server
            .enable_auth(move |cred: Vec<u8>| {
                let verifier = verifier.clone();
                async move {
                    let jwt = String::from_utf8(cred).ok()?;
                    let hub_principal = verifier.verify_user_token(&jwt)?;
                    let principal: unison::network::Principal = Arc::new(hub_principal);
                    Some(principal)
                }
            })
            .await;
    }

    // relay registry (ADR-020 §S4): worlds.Register で wld_id → ctx を transient 登録し、
    // relay channel が target ctx を引いて open_server_stream する。 hub の application state (D1)。
    let relay_registry: RelayRegistry = Arc::new(RwLock::new(HashMap::new()));

    {
        let storage = storage.clone();
        let relay_registry = relay_registry.clone();
        server
            .register_channel("worlds", move |ctx, stream| {
                // register_channel は Fn (接続毎に呼ばれる) なので、 毎回 clone する。
                let storage = storage.clone();
                let relay_registry = relay_registry.clone();
                async move {
                    let channel = UnisonChannel::new(stream);
                    // この connection が登録した wld_id (close 時に registry から除去 = D1)。
                    let mut registered: Vec<String> = Vec::new();
                    let result = loop {
                        match channel.recv().await {
                            Ok(msg) if msg.msg_type == MessageType::Request => {
                                let payload = msg.payload_as_value().unwrap_or_default();
                                // principal は unison.auth で connection に立つ (未認証なら None)。
                                let principal = ctx.principal().await;
                                let reply = handle_worlds(
                                    &storage,
                                    principal.as_ref(),
                                    auth_required,
                                    &msg.method,
                                    payload,
                                )
                                .await
                                .unwrap_or_else(|e| json!({ "error": e.to_string() }));
                                // relay registry を Register/Unregister に追従 (wld_id → ctx)。
                                sync_relay_registry(
                                    &relay_registry,
                                    &ctx,
                                    &msg.method,
                                    &reply,
                                    &mut registered,
                                )
                                .await;
                                if channel
                                    .send_response(msg.id, &msg.method, &reply)
                                    .await
                                    .is_err()
                                {
                                    break Ok(());
                                }
                            }
                            Ok(_) => continue,
                            Err(e) if e.is_normal_close() => break Ok(()),
                            Err(e) => break Err(e),
                        }
                    };
                    // cleanup: connection が消えたら登録 wld_id を registry から除去 (transient = D1)。
                    if !registered.is_empty() {
                        let mut reg = relay_registry.write().await;
                        for w in &registered {
                            reg.remove(w);
                        }
                    }
                    result
                }
            })
            .await;
    }

    // relay channel (ADR-020 §S4 = universal floor): direct 全滅時に hub が中継する。 source が
    // {to, from} を宣言 → hub が target ctx を registry から引いて open_server_stream で reliable な
    // server-initiated stream を開き、 source → target を dumb forward (片方向 = D5「片方向 tell」、
    // payload は opaque)。 双方向は B→A を B が relay することで創発する。
    {
        let relay_registry = relay_registry.clone();
        server
            .register_channel("relay", move |ctx, stream| {
                let relay_registry = relay_registry.clone();
                async move { handle_relay(&relay_registry, &ctx, auth_required, stream).await }
            })
            .await;
    }

    let cert_source = build_cert_source(cert, cert_out.as_deref())?;
    let handle = server.spawn_listen_with_cert(addr, cert_source).await?;
    tracing::info!(%addr, "Unison surface listening (channels: unison.discovery, worlds, relay)");
    Ok(handle)
}

/// config の [`UnisonCert`] を club-unison の [`CertSource`] に変換する (ADR-020 §S1)。
///
/// self-signed mode で `cert_out` が指定されていれば、 生成 cert の DER をそのパスへ
/// 書き出す (非 loopback client が `TrustAnchors::Custom` に pin する用)。
/// `spawn_listen_with_cert` は `CertSource` を consume するため、 export 用に
/// ここで一度 `resolve` して `Provided` に詰め替える。
fn build_cert_source(cert: UnisonCert, cert_out: Option<&str>) -> Result<CertSource> {
    match cert {
        UnisonCert::Dev => {
            tracing::info!("Unison cert: dev_localhost (loopback only — DEV)");
            Ok(CertSource::dev_localhost())
        }
        UnisonCert::SelfSigned { sans } => {
            let ck = CertSource::SelfSigned {
                subject_alt_names: sans.clone(),
            }
            .resolve()
            .context("resolve self-signed Unison cert")?;
            if let Some(path) = cert_out
                && let Some(leaf) = ck.cert.first()
            {
                std::fs::write(path, leaf.as_ref())
                    .with_context(|| format!("write Unison cert DER to {path}"))?;
                tracing::info!(
                    path,
                    bytes = leaf.as_ref().len(),
                    "Unison self-signed cert DER exported (clients pin via TrustAnchors::Custom)"
                );
            }
            tracing::info!(?sans, "Unison cert: self-signed (非 loopback 解禁)");
            Ok(CertSource::Provided { certified_key: ck })
        }
        UnisonCert::File {
            cert_path,
            key_path,
        } => {
            tracing::info!(cert_path, key_path, "Unison cert: from file");
            Ok(CertSource::FromFile {
                cert_path: cert_path.into(),
                key_path: key_path.into(),
            })
        }
    }
}

/// `worlds` channel の method dispatch。 federation scope を arm ごとに検証する (ADR-020 §S3)。
async fn handle_worlds(
    storage: &Storage,
    principal: Option<&unison::network::Principal>,
    auth_required: bool,
    method: &str,
    payload: Value,
) -> Result<Value> {
    match method {
        "Register" => {
            authorize_federation(principal, "federation.register", auth_required)?;
            let handle = payload
                .get("handle")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("field 'handle' required"))?;
            let name = payload
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(handle);
            // wld_id (location 独立 routing key) / endpoints (direct 到達候補) は additive、
            // 旧 client は省略 → None / 空配列で後方互換 (ADR-020 §S2)。
            let wld_id = payload.get("wld_id").and_then(|v| v.as_str());
            let endpoints: Vec<String> = payload
                .get("endpoints")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let registered_at = storage
                .register_world(wld_id, handle, name, &endpoints)
                .await?;
            tracing::info!(
                handle,
                ?wld_id,
                endpoint_count = endpoints.len(),
                "world registered via Unison"
            );
            Ok(json!({
                "wld_id": wld_id,
                "handle": handle,
                "registered_at": registered_at,
                "endpoints": endpoints,
            }))
        }
        "Discover" => {
            authorize_federation(principal, "federation.read", auth_required)?;
            let worlds = storage.list_resources_by_type("vp-world").await?;
            let list: Vec<Value> = worlds
                .iter()
                .map(|w| {
                    json!({
                        "wld_id": w.payload.get("wld_id").and_then(|v| v.as_str()),
                        "handle": w.handle,
                        "name": w.payload.get("name").and_then(|v| v.as_str()).unwrap_or(&w.handle),
                        "endpoints": w.payload.get("endpoints").cloned().unwrap_or_else(|| json!([])),
                        "registered_at": w.created_at,
                    })
                })
                .collect();
            Ok(json!({ "worlds": list }))
        }
        "Unregister" => {
            // deregister は自身の登録の撤回 = Register と同じ scope で gate (ADR-020 §S3)。
            authorize_federation(principal, "federation.register", auth_required)?;
            // wld_id (推奨) か handle のどちらかで対象を指す。 register_world と同じ rid 解決。
            let wld_id = payload.get("wld_id").and_then(|v| v.as_str());
            let handle = payload.get("handle").and_then(|v| v.as_str());
            if wld_id.is_none() && handle.is_none() {
                return Err(anyhow::anyhow!("field 'wld_id' or 'handle' required"));
            }
            let removed = storage.unregister_world(wld_id, handle).await?;
            tracing::info!(?wld_id, ?handle, removed, "world unregistered via Unison");
            Ok(json!({
                "wld_id": wld_id,
                "handle": handle,
                "removed": removed,
            }))
        }
        other => Err(anyhow::anyhow!(
            "unknown method '{other}' on channel 'worlds'"
        )),
    }
}

/// federation scope を検証する (ADR-020 §S3 / ADR-006 `federation.*` scope)。
///
/// - principal 有 → hub [`crate::auth::Principal`] に downcast して scope を確認。
/// - principal 無 → `auth_required` なら拒否、 permissive なら警告して通す
///   (credential 未提示の現 client を壊さず段階移行する)。
fn authorize_federation(
    principal: Option<&unison::network::Principal>,
    required_scope: &str,
    auth_required: bool,
) -> Result<()> {
    match principal {
        Some(p) => match p.downcast_ref::<crate::auth::Principal>() {
            Some(hp) if hp.scopes().iter().any(|s| s == required_scope) => Ok(()),
            Some(_) => Err(anyhow::anyhow!(
                "insufficient scope: '{required_scope}' required"
            )),
            None => Err(anyhow::anyhow!("unrecognized principal")),
        },
        None if auth_required => Err(anyhow::anyhow!(
            "authentication required (scope '{required_scope}')"
        )),
        None => {
            tracing::warn!(
                scope = required_scope,
                "federation request without auth (permissive mode)"
            );
            Ok(())
        }
    }
}

/// `relay` channel handler (ADR-020 §S4 = universal floor)。
///
/// source の宣言 `{to, from}` を読み、 target ctx を [`RelayRegistry`] から引いて
/// `open_server_stream` で reliable な server-initiated stream を開き、 source → target を
/// **片方向に dumb forward** する (payload は opaque = D5、 hub は中身を覗かない)。 双方向は
/// B→A を B が relay することで創発 (D5「片方向 tell の交換」)。 target offline は D3-c
/// (hub は貯めず source に offline を返す → 送り手 home-World が reconcile)。
async fn handle_relay(
    registry: &RelayRegistry,
    ctx: &Arc<ConnectionContext>,
    auth_required: bool,
    stream: UnisonStream,
) -> std::result::Result<(), unison::network::NetworkError> {
    // gate: federation 参加者 (federation.read)。 permissive default で現 client 無回帰。
    let principal = ctx.principal().await;
    if authorize_federation(principal.as_ref(), "federation.read", auth_required).is_err() {
        let _ = stream
            .send_frame(&relay_status("error", "unauthorized"))
            .await;
        return Ok(());
    }
    // 先頭 frame = 宛先宣言 {to, from}。
    let open = match stream.recv_frame().await {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    let pv = open.payload_as_value().unwrap_or_default();
    let from = pv
        .get("from")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let Some(to) = pv.get("to").and_then(|v| v.as_str()).map(str::to_string) else {
        let _ = stream
            .send_frame(&relay_status("error", "field 'to' required"))
            .await;
        return Ok(());
    };
    // target lookup (transient registry)。 不在 = offline → D3-c。
    let target_ctx = registry.read().await.get(&to).cloned();
    let Some(target_ctx) = target_ctx else {
        let _ = stream.send_frame(&relay_status("offline", &to)).await;
        return Ok(());
    };
    // target へ reliable な server-initiated stream を開く (club-unison 1.5.0 primitive)。
    let target_stream = match target_ctx.open_server_stream("relay").await {
        Ok(s) => s,
        Err(e) => {
            let _ = stream
                .send_frame(&relay_status("error", &format!("open: {e}")))
                .await;
            return Ok(());
        }
    };
    // target に送信元を宣言 ({from})。
    if target_stream.send_frame(&relay_open(&from)).await.is_err() {
        let _ = stream
            .send_frame(&relay_status("error", "target unreachable"))
            .await;
        return Ok(());
    }
    let _ = stream.send_frame(&relay_status("established", &to)).await;
    tracing::info!(from = %from, to = %to, "relay established (one-way, reliable)");
    // dumb forward: source → target。 payload opaque (hub は覗かない = D5)。 source close /
    // target write 失敗で抜ける (recv_frame Err = stream 終端)。
    while let Ok(msg) = stream.recv_frame().await {
        if target_stream.send_frame(&msg).await.is_err() {
            break;
        }
    }
    tracing::info!(from = %from, to = %to, "relay closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use unison::network::context::ConnectionContext;

    /// relay registry の lifecycle: Register で wld_id→ctx 挿入 / wld_id 無しは skip /
    /// Unregister で除去 / connection 追跡 (registered) が一致する (ADR-020 §S4 / D1)。
    #[tokio::test]
    async fn relay_registry_register_skip_unregister() {
        let registry: RelayRegistry = Arc::new(RwLock::new(HashMap::new()));
        let ctx = Arc::new(ConnectionContext::new());
        let mut registered: Vec<String> = Vec::new();

        // Register (wld_id 有) → 挿入 + 追跡
        let reply = json!({ "wld_id": "wld_a", "handle": "a", "registered_at": "t" });
        sync_relay_registry(&registry, &ctx, "Register", &reply, &mut registered).await;
        assert!(registry.read().await.contains_key("wld_a"));
        assert_eq!(registered, vec!["wld_a".to_string()]);

        // Register (wld_id 無し = 旧 client) → 挿入しない
        let reply_nowld = json!({ "wld_id": null, "handle": "b" });
        sync_relay_registry(&registry, &ctx, "Register", &reply_nowld, &mut registered).await;
        assert_eq!(
            registry.read().await.len(),
            1,
            "wld_id 無しは registry に入らない"
        );

        // 同 wld_id 再 Register → 追跡は重複しない
        sync_relay_registry(&registry, &ctx, "Register", &reply, &mut registered).await;
        assert_eq!(registered.len(), 1, "registered は wld_id 重複しない");

        // Unregister → 除去 + 追跡から外れる
        let unreg = json!({ "wld_id": "wld_a", "removed": 1 });
        sync_relay_registry(&registry, &ctx, "Unregister", &unreg, &mut registered).await;
        assert!(!registry.read().await.contains_key("wld_a"));
        assert!(registered.is_empty());
    }

    /// relay control frame が JSON payload を正しく載せる (source status / target open 宣言)。
    #[tokio::test]
    async fn relay_control_frames() {
        let st = relay_status("established", "wld_b");
        let v = st.payload_as_value().unwrap();
        assert_eq!(v["status"], "established");
        assert_eq!(v["detail"], "wld_b");

        let op = relay_open("wld_a");
        let v = op.payload_as_value().unwrap();
        assert_eq!(v["from"], "wld_a");
    }
}
