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
use chronista_hub_server::storage::Storage;
use chronista_hub_server::{SERVICE_NAME, VERSION};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

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
    let consumer = spawn_consumer(event_log.clone(), storage.clone(), 1000);

    let verifier = build_verifier(&cfg.auth).await?;
    let state = AppState {
        storage,
        event_log,
        verifier,
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

/// env に応じて Verifier を構築。
///
/// - `STUB_AUTH_ALLOWED=true`: 無署名 StubVerifier (dev/test 専用、 loud warn)。
/// - それ以外: Creo ID JWKS を起動時 fetch して JwksVerifier (user-jwt 実検証)。
///   JWKS 到達不可なら fail-fast (= 本物の認証を要求したのに IdP に繋がらない)。
async fn build_verifier(auth: &AuthConfig) -> anyhow::Result<Arc<dyn Verifier>> {
    if auth.stub_auth_allowed {
        tracing::warn!(
            "auth: StubVerifier 使用中 (STUB_AUTH_ALLOWED=true) — JWT 署名を検証しません。 dev/test 専用、 本番禁止"
        );
        return Ok(Arc::new(StubVerifier));
    }

    tracing::info!(jwks_url = %auth.jwks_url, "auth: fetching Creo ID JWKS");
    let jwks_json = fetch_jwks(&auth.jwks_url)
        .await
        .with_context(|| format!("fetch JWKS from {}", auth.jwks_url))?;
    // allow_stub_app_token=true: product-token (署名付き Hub 発行) は ADR-010 Phase 2 のため、
    // それまで ingestion を止めないよう暫定 app-token を受理する。
    let verifier = JwksVerifier::from_jwks_json(
        &jwks_json,
        auth.issuer.clone(),
        auth.audiences.clone(),
        true,
    )?;
    tracing::info!(
        issuer = %auth.issuer, audiences = ?auth.audiences,
        "auth: JwksVerifier ready (user-jwt RS256)"
    );
    tracing::warn!(
        "auth: interim 無署名 app-token を受理中 (product-token = ADR-010 Phase 2 まで)"
    );
    Ok(Arc::new(verifier))
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
