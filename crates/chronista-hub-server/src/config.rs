//! 環境変数からの server config。

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub namespace: String,
    pub database: String,
    /// SurrealDB の接続先 URL (ADR-022)。 `rocksdb://<dir>` = embedded、
    /// `ws://host:port` = remote。 `mem://` は揮発 (test 用)。
    pub db_url: String,
    /// remote 接続時の signin 資格情報。 None なら signin しない (embedded は不要)。
    pub db_auth: Option<DbAuth>,
    pub auto_migrate: bool,
    pub migrations_dir: String,
    /// Unison (QUIC) surface の listen address (node registry/discovery channel)。
    pub unison_addr: String,
    /// Unison surface の TLS cert source (ADR-020 §S1)。
    pub unison_cert: UnisonCert,
    /// self-signed mode で生成 cert DER を書き出すパス (非 loopback client が
    /// `TrustAnchors::Custom` に pin する用)。 None なら書き出さない。
    pub unison_cert_out: Option<String>,
    /// federation discovery (nodes channel) の auth 強制 (ADR-020 §S3)。
    /// `CHRONISTA_HUB_FEDERATION_AUTH=required` で true。 default false = permissive
    /// (credential 提示なしも許容、 提示時のみ scope 検証 → 現 client を壊さず段階移行)。
    pub federation_auth_required: bool,
    pub auth: AuthConfig,
}

/// SurrealDB の signin 資格情報 (ADR-022)。 password は Debug に出さない。
#[derive(Clone, PartialEq, Eq)]
pub struct DbAuth {
    pub level: DbAuthLevel,
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for DbAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbAuth")
            .field("level", &self.level)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// signin する user の階層。 共有 instance (live storage) では最小権限の `Database` を使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbAuthLevel {
    Root,
    Namespace,
    Database,
}

/// Unison (QUIC) surface の TLS cert source (ADR-020 §S1)。
///
/// - `Dev`: dev_localhost (loopback のみ、 client は SkipVerification)。 default。
/// - `SelfSigned`: 指定 SAN の self-signed (非 loopback = tailnet/scratch 解禁)。
///   client は cert DER を `TrustAnchors::Custom` に pin する (hash でなく cert そのもの)。
/// - `File`: cert + key をファイルから (proper PKI = live、 client は System trust)。
#[derive(Debug, Clone)]
pub enum UnisonCert {
    Dev,
    SelfSigned { sans: Vec<String> },
    File { cert_path: String, key_path: String },
}

/// `/settings` の Creo ID ログイン (PKCE) に要る公開値。 secret は持たない。
#[derive(Debug, Clone)]
pub struct SettingsConfig {
    /// Auth0 の SPA application の client_id (公開値)
    pub client_id: String,
    /// Hub 自身の URL (redirect_uri = `{public_url}/settings`)
    pub public_url: String,
    /// token の aud。 共通の aud `https://id.anycreative.tech` (ADR-023 Q2 の A)
    pub audience: String,
}

/// 認証設定。 default は ecosystem canonical Creo ID (ADR-002/010)。
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// OIDC issuer (`iss` claim 検証用)。
    pub issuer: String,
    /// JWKS endpoint。 起動時に fetch して RS256 鍵を得る。
    pub jwks_url: String,
    /// 許容 audience list。 token の `aud` がこのいずれかを含めば OK。
    pub audiences: Vec<String>,
    /// dev/test: JWKS を使わず無署名 StubVerifier を許可 (本番禁止)。
    pub stub_auth_allowed: bool,
    /// interim: JWKS mode でも無署名 app-token (暫定 product-token) を受理するか。
    /// default false (fail-closed)。 署名なし token の dev 用 opt-in。
    pub allow_stub_app_token: bool,
    /// 管理 API (token 発行/rotate/revoke) を守る admin key。 None なら管理 API 無効。
    pub admin_key: Option<String>,
    /// JWKS background refetch 間隔 (秒)。 ADR-010: 5 分。
    pub jwks_refresh_secs: u64,
    /// `/settings` のブラウザログイン用 (`HUB_CLIENT_ID`)。 None なら `/settings` は 503。
    pub settings: Option<SettingsConfig>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let issuer = std::env::var("CREO_ID_ISSUER")
            .unwrap_or_else(|_| "https://id.creo-memories.in/".into());
        let jwks_url = std::env::var("CREO_ID_JWKS_URL")
            .unwrap_or_else(|_| format!("{}.well-known/jwks.json", trailing_slash(&issuer)));
        let audiences = std::env::var("CREO_ID_AUDIENCES")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["chronista-hub".into()]);

        let unison_cert = match std::env::var("CHRONISTA_HUB_CERT_MODE").as_deref() {
            Ok("self-signed") | Ok("selfsigned") => {
                let sans = std::env::var("CHRONISTA_HUB_CERT_SANS")
                    .ok()
                    .map(|s| {
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                            .collect::<Vec<_>>()
                    })
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| vec!["localhost".into(), "::1".into(), "127.0.0.1".into()]);
                UnisonCert::SelfSigned { sans }
            }
            Ok("file") => UnisonCert::File {
                cert_path: std::env::var("CHRONISTA_HUB_CERT_PATH").unwrap_or_default(),
                key_path: std::env::var("CHRONISTA_HUB_CERT_KEY_PATH").unwrap_or_default(),
            },
            _ => UnisonCert::Dev,
        };
        let unison_cert_out = std::env::var("CHRONISTA_HUB_CERT_OUT")
            .ok()
            .filter(|s| !s.is_empty());
        let federation_auth_required =
            std::env::var("CHRONISTA_HUB_FEDERATION_AUTH").as_deref() == Ok("required");

        let db_url = resolve_db_url(
            std::env::var("CHRONISTA_HUB_DB_URL").ok().as_deref(),
            std::env::var("CHRONISTA_HUB_DB_PATH").ok().as_deref(),
        );
        let db_auth = parse_db_auth(
            std::env::var("SURREALDB_USERNAME").ok().as_deref(),
            std::env::var("SURREALDB_PASSWORD").ok().as_deref(),
            std::env::var("SURREALDB_AUTH_LEVEL").ok().as_deref(),
        )?;

        Ok(Config {
            port: std::env::var("CHRONISTA_HUB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            namespace: std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "chronista".into()),
            database: std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "hub".into()),
            db_url,
            db_auth,
            auto_migrate: std::env::var("AUTO_MIGRATE_ENABLED").as_deref() == Ok("true"),
            migrations_dir: std::env::var("MIGRATIONS_DIR")
                .unwrap_or_else(|_| "./migrations".into()),
            unison_addr: std::env::var("CHRONISTA_HUB_UNISON_ADDR")
                .unwrap_or_else(|_| "[::1]:7879".into()),
            unison_cert,
            unison_cert_out,
            federation_auth_required,
            auth: AuthConfig {
                issuer,
                jwks_url,
                audiences,
                stub_auth_allowed: std::env::var("STUB_AUTH_ALLOWED").as_deref() == Ok("true"),
                allow_stub_app_token: std::env::var("STUB_APP_TOKEN_ALLOWED").as_deref()
                    == Ok("true"),
                admin_key: std::env::var("HUB_ADMIN_KEY")
                    .ok()
                    .filter(|s| !s.is_empty()),
                jwks_refresh_secs: std::env::var("JWKS_REFRESH_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(300),
                settings: std::env::var("HUB_CLIENT_ID")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|client_id| SettingsConfig {
                        client_id,
                        public_url: std::env::var("HUB_PUBLIC_URL")
                            .ok()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "https://hub.chronista.club".into())
                            .trim_end_matches('/')
                            .to_string(),
                        audience: std::env::var("HUB_LOGIN_AUDIENCE")
                            .ok()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "https://id.anycreative.tech".into()),
                    }),
            },
        })
    }
}

/// DB 接続先 URL を決める (ADR-022)。
///
/// `CHRONISTA_HUB_DB_URL` が最優先。 未設定なら旧来の `CHRONISTA_HUB_DB_PATH` を
/// embedded rocksdb として扱う (既存 deploy / e2e を壊さない)。 どちらも無ければ
/// `./data/hub.rocksdb`。 空文字は未設定扱い。
pub fn resolve_db_url(db_url: Option<&str>, db_path: Option<&str>) -> String {
    if let Some(url) = db_url.filter(|s| !s.is_empty()) {
        return url.to_string();
    }
    let path = db_path
        .filter(|s| !s.is_empty())
        .unwrap_or("./data/hub.rocksdb");
    format!("rocksdb://{path}")
}

/// signin 資格情報を組み立てる (ADR-022)。 username / password は両方揃って初めて有効。
/// 片方だけ・未知の level は設定ミスとして起動を止める。 level の default は `database`。
pub fn parse_db_auth(
    username: Option<&str>,
    password: Option<&str>,
    level: Option<&str>,
) -> anyhow::Result<Option<DbAuth>> {
    let username = username.filter(|s| !s.is_empty());
    let password = password.filter(|s| !s.is_empty());
    let (username, password) = match (username, password) {
        (None, None) => return Ok(None),
        (Some(u), Some(p)) => (u, p),
        _ => anyhow::bail!("SURREALDB_USERNAME と SURREALDB_PASSWORD は両方設定する"),
    };
    let level = match level.filter(|s| !s.is_empty()).unwrap_or("database") {
        "root" => DbAuthLevel::Root,
        "namespace" => DbAuthLevel::Namespace,
        "database" => DbAuthLevel::Database,
        other => {
            anyhow::bail!("SURREALDB_AUTH_LEVEL={other:?} は不正 (root / namespace / database)")
        }
    };
    Ok(Some(DbAuth {
        level,
        username: username.to_string(),
        password: password.to_string(),
    }))
}

/// issuer に末尾 `/` を保証 (jwks_url 連結用)。
fn trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_url_defaults_to_embedded_rocksdb() {
        assert_eq!(resolve_db_url(None, None), "rocksdb://./data/hub.rocksdb");
    }

    #[test]
    fn db_url_falls_back_to_legacy_db_path() {
        assert_eq!(
            resolve_db_url(None, Some("/app/data/hub.rocksdb")),
            "rocksdb:///app/data/hub.rocksdb"
        );
    }

    #[test]
    fn db_url_wins_over_db_path() {
        assert_eq!(
            resolve_db_url(
                Some("ws://100.82.103.64:8001"),
                Some("/app/data/hub.rocksdb")
            ),
            "ws://100.82.103.64:8001"
        );
    }

    #[test]
    fn empty_db_url_is_unset() {
        assert_eq!(resolve_db_url(Some(""), Some("/x")), "rocksdb:///x");
    }

    #[test]
    fn db_auth_absent_is_none() {
        assert_eq!(parse_db_auth(None, None, None).unwrap(), None);
    }

    #[test]
    fn db_auth_defaults_to_database_level() {
        let auth = parse_db_auth(Some("hub"), Some("secret"), None)
            .unwrap()
            .unwrap();
        assert_eq!(auth.level, DbAuthLevel::Database);
        assert_eq!(auth.username, "hub");
        assert_eq!(auth.password, "secret");
    }

    #[test]
    fn db_auth_level_is_parsed() {
        for (s, level) in [
            ("root", DbAuthLevel::Root),
            ("namespace", DbAuthLevel::Namespace),
            ("database", DbAuthLevel::Database),
        ] {
            let auth = parse_db_auth(Some("u"), Some("p"), Some(s))
                .unwrap()
                .unwrap();
            assert_eq!(auth.level, level);
        }
    }

    #[test]
    fn db_auth_rejects_half_configured_credentials() {
        assert!(parse_db_auth(Some("u"), None, None).is_err());
        assert!(parse_db_auth(None, Some("p"), None).is_err());
    }

    #[test]
    fn db_auth_rejects_unknown_level() {
        assert!(parse_db_auth(Some("u"), Some("p"), Some("admin")).is_err());
    }

    #[test]
    fn db_auth_debug_does_not_leak_password() {
        let auth = parse_db_auth(Some("u"), Some("hunter2"), None)
            .unwrap()
            .unwrap();
        assert!(!format!("{auth:?}").contains("hunter2"));
    }
}
