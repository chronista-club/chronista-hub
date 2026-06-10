//! Auth — pluggable Verifier + StubVerifier。 TS `auth.ts` 相当。
//!
//! User token: `Authorization: Bearer <jwt>` / App token: `X-App-Token: app:<appId>:<s1>,<s2>`。
//! StubVerifier は JWT signature を検証せず payload を decode するだけ (dev/test 専用)。
//! 本番では Creo ID JWKS を検証する Verifier に差し替える (ADR-002 / ADR-010)。

use base64::Engine;

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
        // 簡易フォーマット: "app:<appId>:<scope1>,<scope2>"
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
            && let Some(p) = verifier.verify_app_token(tok) {
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

    if principal.is_none() && accept.contains(&PrincipalKind::User)
        && let Some(b) = bearer
            && let Some(tok) = b.strip_prefix("Bearer ") {
                principal = verifier.verify_user_token(tok);
            }

    principal.ok_or(AuthError::Unauthorized)
}
