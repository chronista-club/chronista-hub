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
}

impl Config {
    pub fn from_env() -> Self {
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
        }
    }
}
