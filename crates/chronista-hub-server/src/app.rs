//! axum Router 組み立て + handler。 TS server (health/tree/events) の API surface を移植。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{AuthError, PrincipalKind, Verifier, authenticate};
use crate::event_log::EventLog;
use crate::model::{AppEntry, AppStatus, Visibility, canonical_handle, validate_envelope};
use crate::product_token::ProductTokenStore;
use crate::storage::{Storage, TreeReadOptions};

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
    pub event_log: EventLog,
    pub verifier: Arc<dyn Verifier>,
    pub product_tokens: ProductTokenStore,
    /// 管理 API (token 発行/rotate/revoke) を守る admin key。 None なら管理 API 無効 (fail-closed)。
    pub admin_key: Option<String>,
    /// Creo ID の issuer (`iss`)。`/start` が来た人の発行元と突き合わせる (ADR-023)。
    pub issuer: String,
    pub service: String,
    pub version: String,
}

/// 500 に落とすための anyhow ラッパ。
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // 詳細は log のみに出し、 クライアントには固定メッセージ (内部情報の漏洩防止)。
        tracing::error!(error = %self.0, "internal server error");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "internal server error" })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/start", get(crate::start::start))
        .route("/v1/tree/{handle}", get(tree_by_handle))
        .route("/v1/tree/{handle}/{*path}", get(tree_by_path))
        .route("/v1/resources/{id}", get(resource_by_id))
        .route("/v1/apps", get(list_apps))
        // --- 利用者の名簿 (ADR-023 D3): 本人は user-jwt、 公開は handle ---
        .route("/v1/me", get(me).patch(patch_me))
        .route("/v1/me/handle", axum::routing::put(claim_handle))
        .route("/v1/users/{handle}", get(user_by_handle))
        .route("/v1/apps/{app_id}/manifest", get(app_manifest))
        .route("/v1/events", post(post_events))
        // --- admin (X-Admin-Key 必須、 HUB_ADMIN_KEY 未設定なら全て 404) ---
        .route("/v1/apps/{app_id}", axum::routing::put(put_app))
        .route(
            "/v1/apps/{app_id}/tokens",
            get(list_tokens).post(issue_token),
        )
        .route("/v1/apps/{app_id}/tokens/rotate", post(rotate_token))
        .route(
            "/v1/apps/{app_id}/tokens/{token_id}",
            axum::routing::delete(revoke_token),
        )
        .with_state(state)
}

async fn root(State(st): State<AppState>) -> Response {
    Json(json!({ "service": st.service, "version": st.version })).into_response()
}

async fn health(State(st): State<AppState>) -> Response {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Json(json!({
        "status": "ok",
        "service": st.service,
        "version": st.version,
        "timestamp": ts,
    }))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct TreeQuery {
    visibility: Option<String>,
    #[serde(rename = "type")]
    r#type: Option<String>,
    limit: Option<usize>,
}

impl TreeQuery {
    fn into_options(self) -> TreeReadOptions {
        let visibility = match self.visibility.as_deref() {
            Some("public") => Some(Visibility::Public),
            Some("shared") => Some(Visibility::Shared),
            Some("private") => Some(Visibility::Private),
            _ => None,
        };
        TreeReadOptions {
            visibility,
            r#type: self.r#type,
            limit: self.limit,
        }
    }
}

fn strip_handle(handle: &str) -> &str {
    handle.strip_prefix('@').unwrap_or(handle)
}

async fn tree_by_handle(
    State(st): State<AppState>,
    Path(handle): Path<String>,
    Query(q): Query<TreeQuery>,
) -> Result<Response, AppError> {
    let h = strip_handle(&handle);
    let resources = st
        .storage
        .get_resources_by_handle(h, &q.into_options())
        .await?;
    Ok(Json(json!({ "handle": h, "path": "/", "resources": resources })).into_response())
}

async fn tree_by_path(
    State(st): State<AppState>,
    Path((handle, path)): Path<(String, String)>,
    Query(q): Query<TreeQuery>,
) -> Result<Response, AppError> {
    let h = strip_handle(&handle);
    let normalized = format!("/{path}");
    let resources = st
        .storage
        .get_resources_by_path(h, &normalized, &q.into_options())
        .await?;
    Ok(Json(json!({ "handle": h, "path": normalized, "resources": resources })).into_response())
}

async fn resource_by_id(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    match st.storage.get_resource_by_id(&id).await? {
        Some(r) => Ok(Json(r).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

async fn app_manifest(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
) -> Result<Response, AppError> {
    match st.storage.get_app_manifest(&app_id).await? {
        Some(m) => Ok(Json(m).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response()),
    }
}

/// 家族のアプリの名簿 (ADR-009 の段階 1)。 公開情報なので認証なし、 active だけを返す。
async fn list_apps(State(st): State<AppState>) -> Result<Response, AppError> {
    let apps = st.storage.list_active_apps().await?;
    Ok(Json(json!({ "apps": apps })).into_response())
}

async fn post_events(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    // --- auth (user|app、 app には register_resource scope 要求) ---
    let bearer = headers.get("authorization").and_then(|v| v.to_str().ok());
    let app_token = headers.get("x-app-token").and_then(|v| v.to_str().ok());
    let required = ["register_resource".to_string()];
    match authenticate(
        st.verifier.as_ref(),
        Some(&st.product_tokens),
        bearer,
        app_token,
        &[PrincipalKind::User, PrincipalKind::App],
        &required,
    )
    .await
    {
        Ok(_principal) => {}
        Err(AuthError::Unauthorized) => {
            return Ok((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response());
        }
        Err(AuthError::InsufficientScope { missing }) => {
            return Ok((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "insufficient scope", "missing_scopes": missing })),
            )
                .into_response());
        }
    }

    // --- parse + validate ---
    let value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid JSON" })),
            )
                .into_response());
        }
    };
    let envelope = match validate_envelope(&value) {
        Ok(e) => e,
        Err(errors) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "validation failed", "details": errors })),
            )
                .into_response());
        }
    };

    // --- append (consumer が storage 反映) ---
    let result = st.event_log.append(&envelope).await?;
    if !result.accepted {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({ "error": "conflict", "reason": result.reason.unwrap_or_else(|| "duplicate".into()) })),
        )
            .into_response());
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "accepted": true, "event_id": envelope.event_id })),
    )
        .into_response())
}

// ============================================================
// Admin endpoints — product-token 発行 / rotate / revoke / 一覧 (ADR-010)
// ============================================================

/// X-Admin-Key を HUB_ADMIN_KEY と照合。 未設定なら 404 (機能ごと隠す)、 不一致は 403。
/// 比較は SHA-256 hash 同士 (naive な byte 比較の timing 漏れを避ける)。
fn check_admin(st: &AppState, headers: &HeaderMap) -> Result<(), Box<Response>> {
    use sha2::{Digest, Sha256};
    let Some(expected) = st.admin_key.as_deref() else {
        return Err(Box::new(
            (StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response(),
        ));
    };
    let provided = headers
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let eq: bool = {
        use subtle::ConstantTimeEq;
        Sha256::digest(provided.as_bytes())
            .ct_eq(&Sha256::digest(expected.as_bytes()))
            .into()
    };
    if !eq {
        return Err(Box::new(
            (
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "invalid admin key" })),
            )
                .into_response(),
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
struct TokenRequest {
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    ttl_days: Option<u32>,
}

async fn issue_token(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let req: TokenRequest = if body.is_empty() {
        TokenRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(r) => r,
            Err(_) => {
                return Ok((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "invalid JSON" })),
                )
                    .into_response());
            }
        }
    };
    let issued = st
        .product_tokens
        .issue(&app_id, &req.scopes, req.ttl_days, req.note)
        .await?;
    tracing::info!(app_id = %app_id, token_id = %issued.token_id, "admin: product-token issued");
    Ok((StatusCode::CREATED, Json(issued)).into_response())
}

async fn rotate_token(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let req: TokenRequest = if body.is_empty() {
        TokenRequest::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_default()
    };
    let issued = st
        .product_tokens
        .rotate(&app_id, &req.scopes, req.note)
        .await?;
    tracing::info!(app_id = %app_id, token_id = %issued.token_id, "admin: product-token rotated (30d overlap)");
    Ok((StatusCode::CREATED, Json(issued)).into_response())
}

async fn revoke_token(
    State(st): State<AppState>,
    Path((app_id, token_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let revoked = st.product_tokens.revoke(&app_id, &token_id).await?;
    if revoked {
        tracing::info!(app_id = %app_id, token_id = %token_id, "admin: product-token revoked");
        Ok((StatusCode::OK, Json(json!({ "revoked": true }))).into_response())
    } else {
        Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response())
    }
}

async fn list_tokens(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let tokens = st.product_tokens.list(&app_id).await?;
    Ok(Json(json!({ "tokens": tokens })).into_response())
}

// ============================================================
// Admin endpoints — アプリの名簿への登録 / 更新 (ADR-009 の段階 1)
// ============================================================

/// `PUT /v1/apps/{app_id}` の body。 field 名は spec (`resource-type "app"`) の snake_case。
/// 知らない field は弾く: GET の camelCase (`loginUrl`) をそのまま PUT し返すと、 黙って
/// 無視された URL が消える (PUT は置き換え) ため。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PutAppRequest {
    name: Option<String>,
    description: Option<String>,
    icon_url: Option<String>,
    home_url: Option<String>,
    login_url: Option<String>,
    manifest_url: Option<String>,
    status: Option<AppStatus>,
}

async fn put_app(
    State(st): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    if let Err(resp) = check_admin(&st, &headers) {
        return Ok(*resp);
    }
    let bad =
        |msg: &str| Ok((StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response());
    let req: PutAppRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            return bad(
                "invalid JSON (status must be pending / active / deregistering / deregistered)",
            );
        }
    };
    if !is_app_id(&app_id) {
        return bad(
            "app_id must be 2-40 chars of a-z, 0-9 and '-', starting with a letter or digit",
        );
    }
    let name = req.name.as_deref().map(str::trim).unwrap_or("");
    if name.is_empty() || name.chars().count() > 80 {
        return bad("name is required (1-80 chars)");
    }
    for (field, url) in [
        ("icon_url", &req.icon_url),
        ("home_url", &req.home_url),
        ("login_url", &req.login_url),
        ("manifest_url", &req.manifest_url),
    ] {
        if let Some(u) = url
            && !is_https_url(u)
        {
            return bad(&format!("{field} must be an https:// URL"));
        }
    }
    let entry = AppEntry {
        app_id,
        name: name.to_string(),
        description: req.description.filter(|d| !d.trim().is_empty()),
        icon_url: req.icon_url,
        home_url: req.home_url,
        login_url: req.login_url,
        manifest_url: req.manifest_url,
        status: req.status.unwrap_or(AppStatus::Active),
    };
    st.storage.put_app(&entry).await?;
    tracing::info!(app_id = %entry.app_id, status = ?entry.status, "admin: app roster entry put");
    Ok(Json(entry).into_response())
}

/// app_id は URL と record id に入るので、 小文字英数と `-` に限る。
fn is_app_id(s: &str) -> bool {
    let len = s.len();
    (2..=40).contains(&len)
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
}

/// `/start` に link として出るので、 https で host のある URL だけを通す
/// (`javascript:` や `data:` を名簿に入れない)。
fn is_https_url(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty() && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ============================================================
// 利用者の名簿と handle (ADR-023 D3、 ADR-002 / 008 / 012 の規則)
// ============================================================

/// Bearer の user-jwt から Creo ID の `sub` を取る。 無ければ 401。
async fn require_user(st: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
    let bearer = headers.get("authorization").and_then(|v| v.to_str().ok());
    match authenticate(
        st.verifier.as_ref(),
        None,
        bearer,
        None,
        &[PrincipalKind::User],
        &[],
    )
    .await
    {
        Ok(crate::auth::Principal::User { user_id, .. }) => Ok(user_id),
        _ => Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response(),
        )),
    }
}

/// 自分の行。 初回接触ならここで名簿に載せて `usr_id` を振る (裁定 2026-10-09)。
/// handle は claim するまで null。
async fn me(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let sub = match require_user(&st, &headers).await {
        Ok(s) => s,
        Err(resp) => return Ok(*resp),
    };
    let u = st.storage.ensure_user(&sub).await?;
    Ok(Json(me_view(&u)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PatchMeRequest {
    display_name: Option<String>,
}

/// 呼び名 (display_name) を変える。 自由文字列、 一意ではない、 空白だけは空と同じ。
async fn patch_me(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let sub = match require_user(&st, &headers).await {
        Ok(s) => s,
        Err(resp) => return Ok(*resp),
    };
    let bad =
        |msg: &str| Ok((StatusCode::BAD_REQUEST, Json(json!({ "error": msg }))).into_response());
    let req: PatchMeRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => return bad("invalid JSON (only displayName is accepted)"),
    };
    let display_name = req
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    if display_name.is_some_and(|d| d.chars().count() > 80) {
        return bad("displayName must be 80 characters or fewer");
    }
    let u = st.storage.ensure_user(&sub).await?;
    match st.storage.set_display_name(&u.usr_id, display_name).await? {
        Some(u) => Ok(Json(me_view(&u)).into_response()),
        None => Err(anyhow::anyhow!("user row vanished during patch_me").into()),
    }
}

#[derive(Debug, Deserialize)]
struct ClaimRequest {
    handle: Option<String>,
}

/// handle の claim。 早い者勝ち、 1 回決めたら固定 (rename は次の段、 ADR-002 の方針で)。
/// 予約名 (migration 004) と使用中は 409。 同じ handle をもう一度は 200 で冪等。
/// 名簿にまだ居なければ (アプリが `/v1/me` を経ずに来た)、 載せてから claim する。
async fn claim_handle(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let sub = match require_user(&st, &headers).await {
        Ok(s) => s,
        Err(resp) => return Ok(*resp),
    };
    let req: ClaimRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(_) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid JSON" })),
            )
                .into_response());
        }
    };
    let Some(handle) = req.handle.as_deref().and_then(canonical_handle) else {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "handle must match ^[a-z0-9][a-z0-9-]{0,30}$" })),
        )
            .into_response());
    };
    let conflict =
        |msg: &str| Ok((StatusCode::CONFLICT, Json(json!({ "error": msg }))).into_response());
    let mine = st.storage.ensure_user(&sub).await?;
    if let Some(current) = &mine.handle {
        if *current == handle {
            return Ok(Json(me_view(&mine)).into_response());
        }
        return conflict("handle already claimed by you; rename is not available yet");
    }
    match st.storage.claim_handle(&mine.usr_id, &handle).await? {
        Some(u) => {
            tracing::info!(handle, usr_id = %u.usr_id, "handle claimed");
            Ok((StatusCode::CREATED, Json(me_view(&u))).into_response())
        }
        None => conflict("handle is taken or reserved"),
    }
}

fn me_view(u: &crate::model::UserEntry) -> serde_json::Value {
    json!({
        "usrId": u.usr_id,
        "handle": u.handle,
        "displayName": u.display_name,
        "canonicalPath": u.canonical_path(),
        "creoSub": u.creo_sub,
    })
}

/// 公開の行 (ADR-008 の 2 軸: usr_id と handle)。 `@` は任意、 大文字は小文字へ寄せる。
/// 予約名の行は利用者ではないので 404。
async fn user_by_handle(
    State(st): State<AppState>,
    Path(handle): Path<String>,
) -> Result<Response, AppError> {
    let not_found =
        || Ok((StatusCode::NOT_FOUND, Json(json!({ "error": "not found" }))).into_response());
    let Some(h) = canonical_handle(strip_handle(&handle)) else {
        return not_found();
    };
    match st.storage.get_user_by_handle(&h).await? {
        Some(u) if u.account_type == "user" => match u.public() {
            Some(p) => Ok(Json(p).into_response()),
            None => not_found(),
        },
        _ => not_found(),
    }
}
