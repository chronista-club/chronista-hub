//! Chronista Hub server バイナリエントリ。
//!
//! 起動シーケンス:
//!   1. embedded SurrealDB (kv-rocksdb) 接続
//!   2. AUTO_MIGRATE_ENABLED なら listen 前に migration 適用 (失敗で exit)
//!   3. consumer 起動 + axum serve (graceful shutdown)

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::{JwksVerifier, StubVerifier, Verifier, fetch_jwks};
use chronista_hub_server::config::{AuthConfig, Config};
use chronista_hub_server::consumer::spawn_consumer;
use chronista_hub_server::db::{connect_rocksdb, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use chronista_hub_server::{SERVICE_NAME, VERSION};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // rustls の process-level CryptoProvider を明示 install。 hub graph には aws-lc-rs
    // (surrealdb/reqwest) と ring (quinn/club-unison) が両方入り、 rustls 0.23 が default を
    // auto-detect できず panic する。 quinn が使う ring を選ぶ (既存 reqwest/surrealdb は
    // 各自 explicit config なので影響なし)。 二重 install は無視。
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cfg = Config::from_env();

    let db = connect_rocksdb(&cfg.db_path, &cfg.namespace, &cfg.database).await?;
    tracing::info!(
        ns = %cfg.namespace, db = %cfg.database, path = %cfg.db_path,
        "connected to embedded SurrealDB"
    );

    if cfg.auto_migrate {
        let applied = run_pending_migrations(&db, Path::new(&cfg.migrations_dir)).await?;
        tracing::info!(count = applied.len(), "migrations applied");
    }

    let storage = Storage::new(db.clone());
    let event_log = EventLog::new(db.clone());
    let product_tokens = ProductTokenStore::new(db.clone());
    let consumer = spawn_consumer(event_log.clone(), storage.clone(), 1000);

    // Unison (QUIC) surface — world registry/discovery channel。 axum と同一 tokio runtime。
    // handle を drop すると shutdown するので、 プロセス生存期間 hold する (`_unison`)。
    let _unison = chronista_hub_server::unison_server::spawn_unison(
        &cfg.unison_addr,
        cfg.unison_cert.clone(),
        cfg.unison_cert_out.clone(),
        storage.clone(),
    )
    .await
    .context("spawn Unison surface")?;

    let (verifier, jwks_verifier) = build_verifier(&cfg.auth).await?;

    // JWKS background refresh (ADR-010: 5 分 cache 相当)。 失敗時は旧 keys を維持。
    if let Some(jwks) = jwks_verifier {
        spawn_jwks_refresh(jwks, cfg.auth.jwks_url.clone(), cfg.auth.jwks_refresh_secs);
    }

    if cfg.auth.admin_key.is_some() {
        tracing::info!("admin API enabled (token issue/rotate/revoke)");
    } else {
        tracing::info!("admin API disabled (HUB_ADMIN_KEY unset)");
    }

    let state = AppState {
        storage,
        event_log,
        verifier,
        product_tokens,
        admin_key: cfg.auth.admin_key.clone(),
        service: SERVICE_NAME.to_string(),
        version: VERSION.to_string(),
    };
    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "listening + consumer running");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    consumer.stop().await;
    tracing::info!("shut down cleanly");
    Ok(())
}

/// env に応じて Verifier を構築。 JwksVerifier の場合は refresh task 用に concrete handle も返す。
///
/// - `STUB_AUTH_ALLOWED=true`: 無署名 StubVerifier (dev/test 専用、 loud warn)。
/// - それ以外: Creo ID JWKS を起動時 fetch して JwksVerifier (user-jwt 実検証)。
///   JWKS 到達不可なら fail-fast (= 本物の認証を要求したのに IdP に繋がらない)。
async fn build_verifier(
    auth: &AuthConfig,
) -> anyhow::Result<(Arc<dyn Verifier>, Option<Arc<JwksVerifier>>)> {
    if auth.stub_auth_allowed {
        tracing::warn!(
            "auth: StubVerifier 使用中 (STUB_AUTH_ALLOWED=true) — JWT 署名を検証しません。 dev/test 専用、 本番禁止"
        );
        return Ok((Arc::new(StubVerifier), None));
    }

    tracing::info!(jwks_url = %auth.jwks_url, "auth: fetching Creo ID JWKS");
    let jwks_json = fetch_jwks(&auth.jwks_url)
        .await
        .with_context(|| format!("fetch JWKS from {}", auth.jwks_url))?;
    let verifier = Arc::new(JwksVerifier::from_jwks_json(
        &jwks_json,
        auth.issuer.clone(),
        auth.audiences.clone(),
        auth.allow_stub_app_token,
    )?);
    tracing::info!(
        issuer = %auth.issuer, audiences = ?auth.audiences,
        "auth: JwksVerifier ready (user-jwt RS256)"
    );
    if auth.allow_stub_app_token {
        tracing::warn!(
            "auth: 無署名 app-token を受理中 (STUB_APP_TOKEN_ALLOWED=true — dev 専用、 本番は product-token を使う)"
        );
    }
    Ok((verifier.clone(), Some(verifier)))
}

/// JWKS を定期 refetch する background task。 fetch/parse 失敗時は旧 keys を維持して続行。
fn spawn_jwks_refresh(jwks: Arc<JwksVerifier>, url: String, interval_secs: u64) {
    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(interval_secs.max(30)));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        ticker.tick().await; // 初回 tick は即時 — 起動時 fetch 済みなので skip
        loop {
            ticker.tick().await;
            match fetch_jwks(&url).await {
                Ok(json) => match jwks.refresh_from_json(&json) {
                    Ok(()) => tracing::debug!("jwks refreshed"),
                    Err(err) => {
                        tracing::warn!(error = %err, "jwks refresh: parse failed — keeping old keys");
                    }
                },
                Err(err) => {
                    tracing::warn!(error = %err, "jwks refresh: fetch failed — keeping old keys");
                }
            }
        }
    });
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
