//! 環境変数からの server config。

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub namespace: String,
    pub database: String,
    /// RocksDB の格納ディレクトリ。
    pub db_path: String,
    pub auto_migrate: bool,
    pub migrations_dir: String,
    pub auth: AuthConfig,
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
}

impl Config {
    pub fn from_env() -> Self {
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

        Config {
            port: std::env::var("CHRONISTA_HUB_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3000),
            namespace: std::env::var("SURREALDB_NAMESPACE").unwrap_or_else(|_| "chronista".into()),
            database: std::env::var("SURREALDB_DATABASE").unwrap_or_else(|_| "hub".into()),
            db_path: std::env::var("CHRONISTA_HUB_DB_PATH")
                .unwrap_or_else(|_| "./data/hub.rocksdb".into()),
            auto_migrate: std::env::var("AUTO_MIGRATE_ENABLED").as_deref() == Ok("true"),
            migrations_dir: std::env::var("MIGRATIONS_DIR")
                .unwrap_or_else(|_| "./migrations".into()),
            auth: AuthConfig {
                issuer,
                jwks_url,
                audiences,
                stub_auth_allowed: std::env::var("STUB_AUTH_ALLOWED").as_deref() == Ok("true"),
            },
        }
    }
}

/// issuer に末尾 `/` を保証 (jwks_url 連結用)。
fn trailing_slash(s: &str) -> String {
    if s.ends_with('/') {
        s.to_string()
    } else {
        format!("{s}/")
    }
}
