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
