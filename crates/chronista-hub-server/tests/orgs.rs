//! 団体 (organization) と読める公開 URL (裁定 2026-10-14、 ADR-024)。
//!
//! - `@anycreative` は人と同じ handle 空間の団体 (migration 013)。 ログインは持たない
//! - `GET /v1/me/publishers`: 自分が使える名義 (自分 + 所属する団体)
//! - `PUT/DELETE /v1/publications/@{handle}/{slug}`: アプリが product token で登録する
//!   (プランの判定はアプリ。 Hub は名義と URL の host を確かめる)
//! - `/@{handle}`: 名義のページ、 `/@{handle}/{slug}`: アプリの公開ページへ 302

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use chronista_hub_server::app::{AppState, build_router};
use chronista_hub_server::auth::StubVerifier;
use chronista_hub_server::db::{Db, connect_mem, run_pending_migrations};
use chronista_hub_server::event_log::EventLog;
use chronista_hub_server::product_token::ProductTokenStore;
use chronista_hub_server::storage::Storage;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORG: &str = "usr_vspVZdfCBo2YG3DN";
const MITO: &str = "google-oauth2|1234567890";
const MAKO: &str = "apple|0987654321";
const SHIP_LOG: &str = "https://app.creo-memories.in/r/atl_1CfdiCfAX5XzXdYtVwsWUf";

struct Hub {
    r: axum::Router,
    db: Db,
}

async fn hub() -> Hub {
    let db = connect_mem("chronista", "hub").await.unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    run_pending_migrations(&db, &dir).await.unwrap();
    let r = build_router(AppState {
        storage: Storage::new(db.clone()),
        event_log: EventLog::new(db.clone()),
        verifier: Arc::new(StubVerifier),
        product_tokens: ProductTokenStore::new(db.clone()),
        admin_key: None,
        issuer: "https://id.creo-memories.in/".into(),
        settings: None,
        service: "chronista-hub".into(),
        version: "0.0.1".into(),
    });
    Hub { r, db }
}

impl Hub {
    fn storage(&self) -> Storage {
        Storage::new(self.db.clone())
    }

    async fn token(&self, app_id: &str, scopes: &[&str]) -> String {
        let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
        ProductTokenStore::new(self.db.clone())
            .issue(app_id, &scopes, None, None)
            .await
            .unwrap()
            .token
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
        let resp = self.r.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, headers, String::from_utf8_lossy(&body).to_string())
    }

    async fn get(
        &self,
        uri: &str,
        sub: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut req = Request::builder().uri(uri);
        if let Some(s) = sub {
            req = req.header("authorization", format!("Bearer {}", user_jwt(s)));
        }
        self.send(req.body(Body::empty()).unwrap()).await
    }

    async fn json(&self, uri: &str, sub: Option<&str>) -> (StatusCode, Value) {
        let (status, _, body) = self.get(uri, sub).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    async fn claim(&self, sub: &str, handle: &str) -> Value {
        let req = Request::builder()
            .method("PUT")
            .uri("/v1/me/handle")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", user_jwt(sub)))
            .body(Body::from(json!({ "handle": handle }).to_string()))
            .unwrap();
        let (_, _, body) = self.send(req).await;
        serde_json::from_str(&body).unwrap()
    }

    async fn publish(&self, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/v1/publications/{path}"))
            .header("content-type", "application/json")
            .header("x-app-token", token)
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    async fn unpublish(&self, token: &str, path: &str, actor: &str) -> StatusCode {
        let req = Request::builder()
            .method("DELETE")
            .uri(format!(
                "/v1/publications/{path}?actorSub={}",
                actor.replace('|', "%7C")
            ))
            .header("x-app-token", token)
            .body(Body::empty())
            .unwrap();
        self.send(req).await.0
    }

    /// sub の人を名簿に載せて usr_id を返す
    async fn usr_id(&self, sub: &str) -> String {
        self.storage().ensure_user(sub).await.unwrap().usr_id
    }
}

fn user_jwt(sub: &str) -> String {
    let b64 = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
    format!(
        "{}.{}.sig",
        b64(r#"{"alg":"none"}"#),
        b64(&json!({ "sub": sub }).to_string())
    )
}

#[tokio::test]
async fn anycreative_is_a_public_organization_that_nobody_can_claim() {
    let h = hub().await;
    let (status, v) = h.json("/v1/users/@anycreative", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["usrId"], ORG);
    assert_eq!(v["accountType"], "organization");
    assert_eq!(v["displayName"], "ANYCREATIVE");
    assert!(v.get("creoSub").is_none());
    // 人の行も accountType を持つ
    h.claim(MITO, "mito").await;
    let (_, mito) = h.json("/v1/users/@mito", None).await;
    assert_eq!(mito["accountType"], "user");
    // 予約名は今までどおり名義ではない
    let (status, _) = h.json("/v1/users/@chronista-club", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // 団体の handle は claim できない
    let req = Request::builder()
        .method("PUT")
        .uri("/v1/me/handle")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", user_jwt(MAKO)))
        .body(Body::from(json!({ "handle": "anycreative" }).to_string()))
        .unwrap();
    assert_eq!(h.send(req).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn the_ship_log_is_reachable_at_its_readable_address() {
    let h = hub().await;
    let (status, headers, _) = h.get("/@anycreative/ship-log", None).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers["location"], SHIP_LOG);
    assert_eq!(headers["cache-control"], "no-store");
    // 大文字と末尾スラッシュは正しい形へ 301 (spec の path-schema)
    for (asked, canonical) in [
        ("/@AnyCreative/Ship-Log", "/@anycreative/ship-log"),
        ("/@anycreative/ship-log/", "/@anycreative/ship-log"),
        ("/@AnyCreative", "/@anycreative"),
        ("/@anycreative/", "/@anycreative"),
    ] {
        let (status, headers, _) = h.get(asked, None).await;
        assert_eq!(status, StatusCode::MOVED_PERMANENTLY, "{asked}");
        assert_eq!(headers["location"], canonical, "{asked}");
    }
    // 名義のページに並ぶ
    let (status, headers, body) = h.get("/@anycreative", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    assert!(body.contains("ANYCREATIVE"), "{body}");
    assert!(body.contains("団体"));
    assert!(body.contains("Weekly Ship Log"));
    assert!(body.contains("href=\"/@anycreative/ship-log\""));
    // API でも読める
    let (status, v) = h.json("/v1/users/@anycreative/publications", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["publications"][0]["slug"], "ship-log");
    assert_eq!(v["publications"][0]["url"], SHIP_LOG);
    assert_eq!(
        v["publications"][0]["canonicalPath"],
        "/@anycreative/ship-log"
    );
}

#[tokio::test]
async fn unknown_names_and_non_handle_paths_are_404_and_old_routes_still_work() {
    let h = hub().await;
    for uri in [
        "/@nobody",
        "/@nobody/x",
        "/@anycreative/nope",
        "/favicon.ico",
        "/x/y",
        "/@chronista-club",
    ] {
        let (status, _, _) = h.get(uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    assert_eq!(h.get("/start", None).await.0, StatusCode::OK);
    assert_eq!(h.get("/health", None).await.0, StatusCode::OK);
    assert_eq!(h.get("/v1/me", None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(h.get("/v1/apps", None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn my_publishers_are_me_and_my_organizations() {
    let h = hub().await;
    // handle が無ければ自分は名義にならない
    let (status, v) = h.json("/v1/me/publishers", Some(MITO)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["publishers"], json!([]));
    h.claim(MITO, "mito").await;
    let (_, v) = h.json("/v1/me/publishers", Some(MITO)).await;
    assert_eq!(v["publishers"].as_array().unwrap().len(), 1);
    assert_eq!(v["publishers"][0]["handle"], "mito");
    assert!(v["publishers"][0]["role"].is_null());
    // 団体の持ち主になると、 団体も名義に並ぶ
    let mito = h.usr_id(MITO).await;
    h.storage().add_member(ORG, &mito, "owner").await.unwrap();
    let (_, v) = h.json("/v1/me/publishers", Some(MITO)).await;
    assert_eq!(v["publishers"][1]["handle"], "anycreative");
    assert_eq!(v["publishers"][1]["accountType"], "organization");
    assert_eq!(v["publishers"][1]["role"], "owner");
    // guest は名義に使えない
    let mako = h.usr_id(MAKO).await;
    h.storage().add_member(ORG, &mako, "guest").await.unwrap();
    let (_, v) = h.json("/v1/me/publishers", Some(MAKO)).await;
    assert_eq!(v["publishers"], json!([]));
    // 人の token でなければ読めない
    assert_eq!(
        h.get("/v1/me/publishers", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn apps_register_publications_only_for_members() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    let body = json!({ "actorSub": MITO, "url": "https://app.creo-memories.in/r/atl_x", "title": "Notes" });
    let (status, v) = h.publish(&t, "@anycreative/notes", body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["reason"], "not_member");
    let mito = h.usr_id(MITO).await;
    h.storage().add_member(ORG, &mito, "owner").await.unwrap();
    let (status, v) = h.publish(&t, "@anycreative/notes", body.clone()).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["canonicalPath"], "/@anycreative/notes");
    assert_eq!(v["appId"], "creo-memories");
    // もう一度は更新
    let (status, v) = h
        .publish(
            &t,
            "@anycreative/notes",
            json!({ "actorSub": MITO, "url": "https://app.creo-memories.in/r/atl_y" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["title"].is_null(), "PUT replaces the title: {v}");
    let (status, headers, _) = h.get("/@anycreative/notes", None).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers["location"], "https://app.creo-memories.in/r/atl_y");
    let (_, list) = h.json("/v1/users/@anycreative/publications", None).await;
    let slugs: Vec<_> = list["publications"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["slug"].clone())
        .collect();
    assert_eq!(slugs, vec![json!("notes"), json!("ship-log")]);
    // 消す
    assert_eq!(
        h.unpublish(&t, "@anycreative/notes", MITO).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.get("/@anycreative/notes", None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.unpublish(&t, "@anycreative/notes", MITO).await,
        StatusCode::NOT_FOUND
    );
    // member でない人の名前では消せない
    assert_eq!(
        h.unpublish(&t, "@anycreative/ship-log", MAKO).await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn people_publish_under_their_own_handle_only() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    h.claim(MITO, "mito").await;
    let url = "https://app.creo-memories.in/r/atl_z";
    let (status, v) = h
        .publish(&t, "@mito/log", json!({ "actorSub": MAKO, "url": url }))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    let (status, v) = h
        .publish(&t, "@mito/log", json!({ "actorSub": MITO, "url": url }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let (status, v) = h
        .publish(&t, "@nobody/log", json!({ "actorSub": MITO, "url": url }))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    // 予約名は名義ではない
    let (status, _) = h
        .publish(
            &t,
            "@chronista-club/log",
            json!({ "actorSub": MITO, "url": url }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_apps_with_the_scope_can_register() {
    let h = hub().await;
    let mito = h.usr_id(MITO).await;
    h.storage().add_member(ORG, &mito, "owner").await.unwrap();
    let body =
        json!({ "actorSub": MITO, "url": "https://app.creo-memories.in/r/atl_x" }).to_string();
    // 人の token では登録できない (プランの判定を素通りするため)
    let req = Request::builder()
        .method("PUT")
        .uri("/v1/publications/@anycreative/notes")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", user_jwt(MITO)))
        .body(Body::from(body.clone()))
        .unwrap();
    assert_eq!(h.send(req).await.0, StatusCode::UNAUTHORIZED);
    // scope の無い product token は 403
    let t = h.token("creo-memories", &["register_resource"]).await;
    let (status, v) = h
        .publish(
            &t,
            "@anycreative/notes",
            serde_json::from_str(&body).unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["missing_scopes"], json!(["register_publication"]));
}

#[tokio::test]
async fn the_url_must_be_on_the_apps_own_host() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    h.claim(MITO, "mito").await;
    for url in [
        "http://app.creo-memories.in/r/atl_x",
        "https://evil.example/r/atl_x",
        "https://app.creo-memories.in.evil.example/x",
        "https://user@app.creo-memories.in/r/atl_x",
        "https://app.creo-memories.in:8443/r/atl_x",
        "https://app.creo-memories.in./r/atl_x",
        "https://in/x",
        "javascript:alert(1)",
        "not a url",
    ] {
        let (status, v) = h
            .publish(&t, "@mito/log", json!({ "actorSub": MITO, "url": url }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{url}: {v}");
    }
    // 1 つ上の domain は可 (creo-memories.in でも公開ページが開く)
    let (status, v) = h
        .publish(
            &t,
            "@mito/log",
            json!({ "actorSub": MITO, "url": "https://creo-memories.in/r/atl_x" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    // 保存と案内は正規化した形だけ。 読む側で host が変わる形 (`\@`、 改行) は残らない
    for (raw, stored) in [
        (
            "https://app.creo-memories.in\\@evil.example/x",
            "https://app.creo-memories.in/@evil.example/x",
        ),
        (
            "https://app.creo-memories.in/r/a\r\nb",
            "https://app.creo-memories.in/r/ab",
        ),
        (
            "HTTPS://APP.creo-memories.in/r/atl_x",
            "https://app.creo-memories.in/r/atl_x",
        ),
        (
            " https://app.creo-memories.in/r/atl_x",
            "https://app.creo-memories.in/r/atl_x",
        ),
    ] {
        let (status, v) = h
            .publish(&t, "@mito/norm", json!({ "actorSub": MITO, "url": raw }))
            .await;
        assert!(status.is_success(), "{raw}: {status} {v}");
        assert_eq!(v["url"], stored, "{raw}");
        let (status, headers, _) = h.get("/@mito/norm", None).await;
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(headers["location"], stored, "{raw}");
    }
}

#[tokio::test]
async fn app_input_never_adds_people_to_the_roster() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    let count = || async {
        let n: Option<i64> =
            h.db.conn()
                .query("RETURN count((SELECT id FROM user))")
                .await
                .unwrap()
                .take(0)
                .unwrap();
        n.unwrap()
    };
    let before = count().await;
    for (path, actor) in [
        ("@anycreative/notes", "junk|1"),
        ("@anycreative/Bad_Slug", "junk|2"),
        ("@anycreative/x", " junk|3"),
    ] {
        let (status, v) = h
            .publish(
                &t,
                path,
                json!({ "actorSub": actor, "url": "https://app.creo-memories.in/r/x" }),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {v}");
    }
    assert_eq!(
        h.unpublish(&t, "@anycreative/ship-log", "junk|4").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(count().await, before, "the roster did not grow");
}

#[tokio::test]
async fn slugs_follow_the_rules() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    h.claim(MITO, "mito").await;
    let url = "https://app.creo-memories.in/r/atl_x";
    for bad in [
        "Bad_Slug",
        "-x",
        "identity",
        "api",
        "creo-memories",
        "vantage-point",
    ] {
        let (status, v) = h
            .publish(
                &t,
                &format!("@mito/{bad}"),
                json!({ "actorSub": MITO, "url": url }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
    let (status, v) = h
        .publish(&t, "@mito/Notes", json!({ "actorSub": MITO, "url": url }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["slug"], "notes");
    let long = "a".repeat(121);
    let (status, _) = h
        .publish(
            &t,
            "@mito/x",
            json!({ "actorSub": MITO, "url": url, "title": long }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = h
        .publish(
            &t,
            "@mito/x",
            json!({ "actorSub": MITO, "url": url, "extra": 1 }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unknown fields are refused"
    );
}

#[tokio::test]
async fn another_apps_slug_cannot_be_taken_over() {
    let h = hub().await;
    // GFP は pending (migration 011)。 このテストでだけ active にする
    h.db.conn()
        .query("UPDATE type::record('app', 'gfp') SET status = 'active'")
        .await
        .unwrap()
        .check()
        .unwrap();
    let mito = h.usr_id(MITO).await;
    h.storage().add_member(ORG, &mito, "owner").await.unwrap();
    let gfp = h.token("gfp", &["register_publication"]).await;
    let (status, v) = h
        .publish(
            &gfp,
            "@anycreative/ship-log",
            json!({ "actorSub": MITO, "url": "https://app.gfp.works/x" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        h.unpublish(&gfp, "@anycreative/ship-log", MITO).await,
        StatusCode::CONFLICT
    );
    // 名簿に host を持たないアプリ (vantage-point は CLI で home_url / login_url が無い) は案内先を持てない
    let vp = h.token("vantage-point", &["register_publication"]).await;
    let (status, _) = h
        .publish(
            &vp,
            "@anycreative/vp",
            json!({ "actorSub": MITO, "url": "https://vantage-point.app/x" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn apps_that_are_not_active_cannot_register() {
    let h = hub().await;
    let mito = h.usr_id(MITO).await;
    h.storage().add_member(ORG, &mito, "owner").await.unwrap();
    // GFP は pending (migration 011)
    let gfp = h.token("gfp", &["register_publication"]).await;
    let (status, v) = h
        .publish(
            &gfp,
            "@anycreative/gfp-notes",
            json!({ "actorSub": MITO, "url": "https://app.gfp.works/x" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
}

#[tokio::test]
async fn renamed_handles_guide_to_the_new_address() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    h.claim(MITO, "mito").await;
    let url = "https://app.creo-memories.in/r/atl_z";
    h.publish(&t, "@mito/log", json!({ "actorSub": MITO, "url": url }))
        .await;
    h.claim(MITO, "mito-2").await;
    let (status, headers, _) = h.get("/@mito", None).await;
    assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(headers["location"], "/@mito-2");
    let (status, headers, _) = h.get("/@mito/log", None).await;
    assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(headers["location"], "/@mito-2/log");
    // 公開物は usr_id に付いているので、 新しい住所でそのまま開く
    let (status, headers, _) = h.get("/@mito-2/log", None).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(headers["location"], url);
    let (status, headers, _) = h.get("/v1/users/@mito/publications", None).await;
    assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(headers["location"], "/v1/users/@mito-2/publications");
    // アプリが旧 handle のまま消しに来ても、 同じ名義のものとして消せる
    assert_eq!(
        h.unpublish(&t, "@mito/log", MITO).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(h.get("/@mito-2/log", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn titles_are_escaped_on_the_name_page() {
    let h = hub().await;
    let t = h.token("creo-memories", &["register_publication"]).await;
    h.claim(MITO, "mito").await;
    h.publish(
        &t,
        "@mito/x",
        json!({ "actorSub": MITO, "url": "https://app.creo-memories.in/r/atl_x", "title": "<script>alert(1)</script>" }),
    )
    .await;
    let (_, _, body) = h.get("/@mito", None).await;
    assert!(!body.contains("<script>alert"), "{body}");
    assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(body.contains("個人"));
}
