//! `/settings` — 自分の handle と呼び名を見て変える 1 枚 (裁定 2026-10-10)。
//!
//! データの持ち主は分けたまま (誰か = Creo ID、 つながり = Hub)、 画面は Hub に 1 つ。
//! ブラウザは Creo ID に PKCE でログインして token を取り、 Hub の `/v1/me` を叩く。
//! Hub は Auth0 の SPA client を 1 つ持つ (client_id は公開値)。

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::config::SettingsConfig;
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn router(settings: Option<SettingsConfig>) -> axum::Router {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    run_pending_migrations(&db, &dir).await.unwrap();
    build_router(AppState {
        storage: Storage::new(db.clone()),
        event_log: EventLog::new(db.clone()),
        verifier: Arc::new(StubVerifier),
        product_tokens: ProductTokenStore::new(db),
        admin_key: None,
        issuer: "https://id.creo-memories.in/".into(),
        settings,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    })
}

fn configured() -> SettingsConfig {
    SettingsConfig {
        client_id: "AbCdEf0123456789".into(),
        public_url: "https://hub.chronista.club".into(),
        audience: "https://id.anycreative.tech".into(),
    }
}

async fn get_settings(r: &axum::Router) -> (StatusCode, String, axum::http::HeaderMap) {
    let resp = r
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap(), headers)
}

#[tokio::test]
async fn settings_page_carries_what_the_browser_needs_to_log_in() {
    let r = router(Some(configured())).await;
    let (status, body, headers) = get_settings(&r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert_eq!(headers["cache-control"], "no-store");
    // PKCE に要る公開値だけ。 secret は無い
    assert!(body.contains("\"clientId\":\"AbCdEf0123456789\""), "{body}");
    assert!(body.contains("\"issuer\":\"https://id.creo-memories.in/\""));
    assert!(body.contains("\"audience\":\"https://id.anycreative.tech\""));
    assert!(body.contains("\"redirectUri\":\"https://hub.chronista.club/settings\""));
    // 画面の骨: handle と呼び名、 アカウントは Creo ID へ
    assert!(body.contains("/v1/me"));
    assert!(body.contains("handle"));
    assert!(body.contains("Creo ID"));
    assert!(!body.to_lowercase().contains("client_secret"));
    // CSP: inline script は hash で許可、 通信は自分と Creo ID だけ、 frame に入れない
    let csp = headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'sha256-"), "{csp}");
    assert!(
        csp.contains("connect-src 'self' https://id.creo-memories.in;"),
        "{csp}"
    );
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    // hash が本当に inline script のものか: <script>…</script> の中身を取って自分で計算する
    let start = body.rfind("<script>").unwrap() + "<script>".len();
    let end = body[start..].find("</script>").unwrap() + start;
    let js = &body[start..end];
    let digest = <sha2::Sha256 as sha2::Digest>::digest(js.as_bytes());
    let expected = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, digest);
    assert!(csp.contains(&format!("'sha256-{expected}'")), "{csp}");
    // ログアウトは Hub からだけ出る (Creo ID の SSO session は切らない)
    assert!(!body.contains("v2/logout"));
}

#[tokio::test]
async fn settings_page_escapes_config_values() {
    let mut cfg = configured();
    cfg.client_id = "x</script><script>alert(1)".into();
    let r = router(Some(cfg)).await;
    let (status, body, _) = get_settings(&r).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("</script><script>alert"), "{body}");
}

#[tokio::test]
async fn settings_is_503_until_the_client_is_configured() {
    let r = router(None).await;
    let (status, body, _) = get_settings(&r).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("準備中"), "{body}");
}
