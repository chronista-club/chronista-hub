//! 利用者の名簿と handle (ADR-023 D3、 ADR-002 / 008 / 012 の規則)。
//!
//! 裁定 2026-10-09: handle は「必要になったときに claim する」。 初回接触 (`GET /v1/me`) で
//! `user` 行を作って `usr_` EntId を振り、 handle は空のまま使える。
//!
//! - `GET /v1/me`: 自分の行。 user-jwt (Creo ID の `sub`) で本人を特定する。 無ければ作る
//! - `PATCH /v1/me`: 呼び名 (display_name)。 自由文字列、 一意ではない
//! - `PUT /v1/me/handle`: handle の claim。 早い者勝ち、 1 回決めたら固定 (rename は次の段)
//! - `GET /v1/users/@{handle}`: 公開。 usr_id / handle / display_name だけ

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::db::{connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn router() -> axum::Router {
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
        settings: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    })
}

/// StubVerifier 用の無署名 user-jwt (payload に `sub` だけ)。
fn user_jwt(sub: &str) -> String {
    let b64 = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
    format!(
        "{}.{}.sig",
        b64(r#"{"alg":"none"}"#),
        b64(&json!({ "sub": sub }).to_string())
    )
}

async fn send(router: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, v)
}

async fn get(router: &axum::Router, uri: &str, sub: Option<&str>) -> (StatusCode, Value) {
    let mut req = Request::builder().uri(uri);
    if let Some(s) = sub {
        req = req.header("authorization", format!("Bearer {}", user_jwt(s)));
    }
    send(router, req.body(Body::empty()).unwrap()).await
}

async fn claim(router: &axum::Router, sub: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("PUT")
        .uri("/v1/me/handle")
        .header("content-type", "application/json");
    if let Some(s) = sub {
        req = req.header("authorization", format!("Bearer {}", user_jwt(s)));
    }
    send(router, req.body(Body::from(body.to_string())).unwrap()).await
}

async fn patch_me(router: &axum::Router, sub: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("PATCH")
        .uri("/v1/me")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", user_jwt(sub)));
    send(router, req.body(Body::from(body.to_string())).unwrap()).await
}

const MITO: &str = "google-oauth2|1234567890";
const MAKO: &str = "apple|0987654321";

#[tokio::test]
async fn me_needs_a_user_token() {
    let r = router().await;
    let (status, _) = get(&r, "/v1/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = claim(&r, None, json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

async fn fresh_db() -> chronista_hub_server::db::Db {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    run_pending_migrations(&db, &dir).await.unwrap();
    db
}

#[tokio::test]
async fn app_tokens_and_product_tokens_are_not_users() {
    // 名簿は人のもの。 app-token (x-app-token) でも product-token (cht_…) でも入れない
    let db = fresh_db().await;
    let pt = ProductTokenStore::new(db.clone());
    let issued = pt.issue("creo-memories", &[], None, None).await.unwrap();
    let r = build_router(AppState {
        storage: Storage::new(db.clone()),
        event_log: EventLog::new(db.clone()),
        verifier: Arc::new(StubVerifier),
        product_tokens: pt,
        admin_key: None,
        issuer: "https://id.creo-memories.in/".into(),
        settings: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    });
    let req = Request::builder()
        .uri("/v1/me")
        .header("x-app-token", "app:creo-memories:register_resource")
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(&r, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let req = Request::builder()
        .uri("/v1/me")
        .header("authorization", format!("Bearer {}", issued.token))
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(&r, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_lost_claim_never_returns_someone_elses_row() {
    // storage 直: UPDATE の WHERE に合わなかったとき (自分が既に別の handle を持つ、
    // 事前チェックの後に他人が取った) に、 その handle の持ち主の行を返してはいけない
    let st = Storage::new(fresh_db().await);
    let mito = st.ensure_user(MITO).await.unwrap();
    let mako = st.ensure_user(MAKO).await.unwrap();
    assert!(
        st.claim_handle(&mako.usr_id, "foo")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        st.claim_handle(&mito.usr_id, "foo")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        st.claim_handle(&mito.usr_id, "bar")
            .await
            .unwrap()
            .is_some()
    );
    let again = st.claim_handle(&mito.usr_id, "baz").await.unwrap();
    assert!(again.is_none(), "{again:?}");
    let me = st.get_user_by_id(&mito.usr_id).await.unwrap().unwrap();
    assert_eq!(me.handle.as_deref(), Some("bar"));
    assert_eq!(me.creo_sub.as_deref(), Some(MITO));
}

#[tokio::test]
async fn concurrent_first_contact_converges_on_one_row() {
    let st = Storage::new(fresh_db().await);
    let (a, b, c) = tokio::join!(
        st.ensure_user(MITO),
        st.ensure_user(MITO),
        st.ensure_user(MITO)
    );
    let (a, b, c) = (a.unwrap(), b.unwrap(), c.unwrap());
    assert_eq!(a.usr_id, b.usr_id);
    assert_eq!(b.usr_id, c.usr_id);
}

#[tokio::test]
async fn first_contact_puts_me_on_the_roster_without_a_handle() {
    let r = router().await;
    let (status, me) = get(&r, "/v1/me", Some(MITO)).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    let usr_id = me["usrId"].as_str().unwrap().to_string();
    assert!(usr_id.starts_with("usr_"), "{usr_id}");
    assert!(usr_id.len() > 8, "{usr_id}");
    assert!(me["handle"].is_null());
    assert!(me["canonicalPath"].is_null());
    assert!(me["displayName"].is_null());
    // Creo ID の sub は本人にだけ見せる
    assert_eq!(me["creoSub"], MITO);

    // 2 回目も同じ行 (usr_id は 1 度振ったら変わらない)
    let (_, again) = get(&r, "/v1/me", Some(MITO)).await;
    assert_eq!(again["usrId"], usr_id);

    // handle 無しの人が何人いてもよい (unique index は handle が空の行を数えない)
    let (status, mako) = get(&r, "/v1/me", Some(MAKO)).await;
    assert_eq!(status, StatusCode::OK, "{mako}");
    assert_ne!(mako["usrId"], usr_id);
    assert!(mako["handle"].is_null());
}

#[tokio::test]
async fn display_name_can_be_set_before_claiming_a_handle() {
    let r = router().await;
    let (status, v) = patch_me(&r, MITO, json!({ "displayName": "みと" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["displayName"], "みと");
    assert!(v["handle"].is_null());
    let (_, me) = get(&r, "/v1/me", Some(MITO)).await;
    assert_eq!(me["displayName"], "みと");

    // 呼び名は一意ではない。 同じ名前を別の人が名乗れる
    let (status, v) = patch_me(&r, MAKO, json!({ "displayName": "みと" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    // field を送らなければ触らない (PATCH の意味)
    let (status, v) = patch_me(&r, MITO, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["displayName"], "みと");
    // null で消せる。 空白だけは空と同じ
    let (_, v) = patch_me(&r, MITO, json!({ "displayName": null })).await;
    assert!(v["displayName"].is_null(), "{v}");
    patch_me(&r, MITO, json!({ "displayName": "みと" })).await;
    let (_, v) = patch_me(&r, MITO, json!({ "displayName": "  " })).await;
    assert!(v["displayName"].is_null(), "{v}");
    // 長すぎは 400、 未知の field も 400
    let (status, _) = patch_me(&r, MITO, json!({ "displayName": "x".repeat(81) })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = patch_me(&r, MITO, json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn claiming_a_handle_keeps_the_usr_id() {
    let r = router().await;
    let (_, before) = get(&r, "/v1/me", Some(MITO)).await;
    let usr_id = before["usrId"].as_str().unwrap().to_string();
    patch_me(&r, MITO, json!({ "displayName": "みと" })).await;

    let (status, v) = claim(&r, Some(MITO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["usrId"], usr_id);
    assert_eq!(v["handle"], "mito");
    assert_eq!(v["displayName"], "みと");
    assert_eq!(v["canonicalPath"], "/@mito");

    let (status, me) = get(&r, "/v1/me", Some(MITO)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["handle"], "mito");
    assert_eq!(me["usrId"], usr_id);

    // 公開の行。 sub と email は出ない
    let (status, pub_) = get(&r, "/v1/users/@mito", None).await;
    assert_eq!(status, StatusCode::OK, "{pub_}");
    assert_eq!(pub_["usrId"], usr_id);
    assert_eq!(pub_["handle"], "mito");
    assert_eq!(pub_["displayName"], "みと");
    assert_eq!(pub_["canonicalPath"], "/@mito");
    assert!(pub_.get("creoSub").is_none());
    assert!(pub_.get("email").is_none());
    // `@` 無しでも、 大文字でも同じ行に着く (lowercase canonical)
    let (status, _) = get(&r, "/v1/users/mito", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = get(&r, "/v1/users/@Mito", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn claiming_without_prior_contact_also_works() {
    // アプリが /v1/me を経由せずいきなり claim へ来ても、 行を作ってから claim する
    let r = router().await;
    let (status, v) = claim(&r, Some(MITO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert!(v["usrId"].as_str().unwrap().starts_with("usr_"));
}

#[tokio::test]
async fn unknown_handle_is_404() {
    let r = router().await;
    let (status, _) = get(&r, "/v1/users/@nobody", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_taken_handle_cannot_be_claimed_by_someone_else() {
    let r = router().await;
    let (status, _) = claim(&r, Some(MITO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, v) = claim(&r, Some(MAKO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    let (status, v) = claim(&r, Some(MAKO), json!({ "handle": "MITO" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    // mako は名簿には居るが handle はまだ無い
    let (_, me) = get(&r, "/v1/me", Some(MAKO)).await;
    assert!(me["usrId"].is_string());
    assert!(me["handle"].is_null());
}

#[tokio::test]
async fn reserved_handles_cannot_be_claimed() {
    let r = router().await;
    for h in ["chronista", "creo", "vp", "chronista-hub"] {
        let (status, v) = claim(&r, Some(MITO), json!({ "handle": h })).await;
        assert_eq!(status, StatusCode::CONFLICT, "{h} → {v}");
    }
    let (status, _) = get(&r, "/v1/users/@chronista", None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "reserved rows are not public users"
    );
}

#[tokio::test]
async fn a_handle_is_claimed_once_and_then_fixed() {
    let r = router().await;
    let (status, first) = claim(&r, Some(MITO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::CREATED);
    // 同じ handle をもう一度は冪等
    let (status, again) = claim(&r, Some(MITO), json!({ "handle": "mito" })).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    assert_eq!(again["usrId"], first["usrId"]);
    // 別の handle へは変えられない (rename は次の段)
    let (status, v) = claim(&r, Some(MITO), json!({ "handle": "mito2" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    let (_, me) = get(&r, "/v1/me", Some(MITO)).await;
    assert_eq!(me["handle"], "mito");
}

#[tokio::test]
async fn handle_must_match_the_pattern() {
    let r = router().await;
    for h in [
        "",
        "-mito",
        "mi to",
        "mito_",
        "みと",
        "a".repeat(32).as_str(),
    ] {
        let (status, v) = claim(&r, Some(MITO), json!({ "handle": h })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{h:?} → {v}");
    }
    let (status, v) = claim(&r, Some(MITO), json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = claim(
        &r,
        Some(MITO),
        json!({ "handle": "mito", "displayName": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "unknown field → {v}");
    // 大文字は小文字に正規化して受ける
    let (status, v) = claim(&r, Some(MITO), json!({ "handle": "Mito" })).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["handle"], "mito");
}
