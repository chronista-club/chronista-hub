//! Auth — pluggable Verifier + StubVerifier。 TS `auth.ts` 相当。
//!
//! User token: `Authorization: Bearer <jwt>` / App token: `X-App-Token: app:<appId>:<s1>,<s2>`。
//! StubVerifier は JWT signature を検証せず payload を decode するだけ (dev/test 専用)。
//! 本番では Creo ID JWKS を検証する Verifier に差し替える (ADR-002 / ADR-010)。

use base64::Engine;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    User {
        user_id: String,
        handle: Option<String>,
        scopes: Vec<String>,
    },
    App {
        app_id: String,
        scopes: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    User,
    App,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    Unauthorized,
    InsufficientScope { missing: Vec<String> },
}

pub trait Verifier: Send + Sync {
    fn verify_user_token(&self, token: &str) -> Option<Principal>;
    fn verify_app_token(&self, token: &str) -> Option<Principal>;
}

/// StubVerifier — JWT payload を decode するだけで verify しない。 本番禁止。
pub struct StubVerifier;

impl Verifier for StubVerifier {
    fn verify_user_token(&self, token: &str) -> Option<Principal> {
        let payload = decode_jwt_payload(token)?;
        let user_id = payload.get("sub").and_then(|v| v.as_str())?.to_string();
        let handle = payload
            .get("handle")
            .and_then(|v| v.as_str())
            .map(String::from);
        let scopes = payload
            .get("scopes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Some(Principal::User {
            user_id,
            handle,
            scopes,
        })
    }

    fn verify_app_token(&self, token: &str) -> Option<Principal> {
        parse_stub_app_token(token)
    }
}

/// 簡易 app-token (= 暫定 product-token) のパース: `app:<appId>:<scope1>,<scope2>`。
///
/// **署名検証なし**。 本物の product-token (Hub 発行・署名付き) は ADR-010 Phase 2。
/// StubVerifier と JwksVerifier の両方が当面これを ingestion 経路の暫定手段として使う。
fn parse_stub_app_token(token: &str) -> Option<Principal> {
    let parts: Vec<&str> = token.split(':').collect();
    if parts.len() != 3 || parts[0] != "app" {
        return None;
    }
    let app_id = parts[1];
    if app_id.is_empty() {
        return None;
    }
    let scopes = parts[2]
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    Some(Principal::App {
        app_id: app_id.to_string(),
        scopes,
    })
}

fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

// ============================================================
// JwksVerifier — Creo ID (OIDC) の user-jwt を JWKS で実検証 (ADR-002/010)
// ============================================================

/// Creo ID 発行 user-jwt の claims。
#[derive(Debug, Deserialize)]
struct Claims {
    /// subject = `usr_id` (EntId、 ADR-002 の stable key)。
    sub: String,
    /// display handle (任意 custom claim)。
    #[serde(default)]
    handle: Option<String>,
    /// OAuth2 scope (空白区切り文字列) — 任意。
    #[serde(default)]
    scope: String,
}

/// Creo ID JWKS で RS256 + iss + aud(list) + exp を検証する Verifier。
///
/// `from_jwks_json` で network 非依存に構築できる (test/起動時に fetch した JSON を渡す)。
/// JWKS は構築時に固定 — rotation 反映は再起動が必要 (5 分 cache + push は ADR-010 Phase 2)。
pub struct JwksVerifier {
    jwks: JwkSet,
    issuer: String,
    audiences: Vec<String>,
    /// 暫定 product-token (`app:id:scopes` 無署名) を受理するか。
    /// 本物の Hub 発行 product-token (ADR-010 Phase 2) ができるまでの interim。
    allow_stub_app_token: bool,
}

impl JwksVerifier {
    /// JWKS JSON 文書から構築 (`https://<issuer>/.well-known/jwks.json` の中身)。
    pub fn from_jwks_json(
        jwks_json: &str,
        issuer: impl Into<String>,
        audiences: Vec<String>,
        allow_stub_app_token: bool,
    ) -> anyhow::Result<Self> {
        let jwks: JwkSet = serde_json::from_str(jwks_json)?;
        Ok(Self {
            jwks,
            issuer: issuer.into(),
            audiences,
            allow_stub_app_token,
        })
    }

    /// RS256 + iss + aud(list 交差) + exp を検証して Claims を返す。
    fn verify(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        let header = decode_header(token)?;
        let kid = header
            .kid
            .ok_or(jsonwebtoken::errors::ErrorKind::InvalidToken)?;
        let jwk = self
            .jwks
            .find(&kid)
            .ok_or(jsonwebtoken::errors::ErrorKind::InvalidToken)?;
        let key = DecodingKey::from_jwk(jwk)?;
        let mut validation = Validation::new(Algorithm::RS256);
        // aud は list 交差で判定 (ADR-010): token の aud が我々の audience 群と 1 つでも一致すれば OK。
        validation.set_audience(&self.audiences);
        validation.set_issuer(&[&self.issuer]);
        Ok(decode::<Claims>(token, &key, &validation)?.claims)
    }
}

impl Verifier for JwksVerifier {
    fn verify_user_token(&self, token: &str) -> Option<Principal> {
        match self.verify(token) {
            Ok(claims) => Some(Principal::User {
                user_id: claims.sub,
                handle: claims.handle,
                scopes: claims.scope.split_whitespace().map(String::from).collect(),
            }),
            Err(err) => {
                tracing::debug!(error = %err, "JwksVerifier: user token rejected");
                None
            }
        }
    }

    fn verify_app_token(&self, token: &str) -> Option<Principal> {
        // product-token (署名付き、 Hub 発行) は未実装 (ADR-010 Phase 2)。
        // それまでの間、 ingestion を止めないため暫定 app-token を opt-in で受理。
        if self.allow_stub_app_token {
            parse_stub_app_token(token)
        } else {
            None
        }
    }
}

/// JWKS endpoint から JSON を取得 (起動時 1 回)。
pub async fn fetch_jwks(url: &str) -> anyhow::Result<String> {
    let body = reqwest::get(url).await?.error_for_status()?.text().await?;
    Ok(body)
}

fn scopes_of(p: &Principal) -> &[String] {
    match p {
        Principal::User { scopes, .. } => scopes,
        Principal::App { scopes, .. } => scopes,
    }
}

/// header から principal を解決。 TS `requireAuth` と同ロジック。
///
/// - app token が accept かつ存在すれば優先的に検証 + scope チェック
/// - 無ければ bearer (user) を検証
/// - どちらも取れなければ Unauthorized
///
/// 注: `required_scopes` は **app token にのみ** 適用する (TS `auth.ts` と同仕様)。
/// user token は scope 未評価 — user-scope ベース ACL は ADR-002/010 の Phase 2 拡張。
pub fn authenticate(
    verifier: &dyn Verifier,
    bearer: Option<&str>,
    app_token: Option<&str>,
    accept: &[PrincipalKind],
    required_scopes: &[String],
) -> Result<Principal, AuthError> {
    let mut principal: Option<Principal> = None;

    if accept.contains(&PrincipalKind::App)
        && let Some(tok) = app_token
        && let Some(p) = verifier.verify_app_token(tok)
    {
        let missing: Vec<String> = required_scopes
            .iter()
            .filter(|s| !scopes_of(&p).contains(s))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(AuthError::InsufficientScope { missing });
        }
        principal = Some(p);
    }

    if principal.is_none()
        && accept.contains(&PrincipalKind::User)
        && let Some(b) = bearer
        && let Some(tok) = b.strip_prefix("Bearer ")
    {
        principal = verifier.verify_user_token(tok);
    }

    principal.ok_or(AuthError::Unauthorized)
}
