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

use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use unison::ServerHandle;
use unison::network::cert::CertSource;
use unison::network::channel::UnisonChannel;
use unison::network::{MessageType, ProtocolServer};

use crate::auth::Verifier;
use crate::config::UnisonCert;
use crate::storage::Storage;

/// hub の Unison protocol schema (channels: unison.discovery + worlds)。
const HUB_PROTOCOL_KDL: &str = include_str!("hub_protocol.kdl");

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

    server
        .register_channel("worlds", move |ctx, stream| {
            // register_channel は Fn (接続毎に呼ばれる) なので、 storage は毎回 clone する。
            let storage = storage.clone();
            async move {
                let channel = UnisonChannel::new(stream);
                loop {
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

    let cert_source = build_cert_source(cert, cert_out.as_deref())?;
    let handle = server.spawn_listen_with_cert(addr, cert_source).await?;
    tracing::info!(%addr, "Unison surface listening (channels: unison.discovery, worlds)");
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
