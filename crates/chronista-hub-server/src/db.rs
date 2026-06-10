//! Embedded SurrealDB 接続 (kv-rocksdb) + 起動時 in-process migrator。
//!
//! ADR-016: 低レイテンシのため別プロセス無しの in-process embedded を採用。
//! migration は HTTP `/sql` ではなく SDK 経由で `migrations/*.surql` を順次適用する。

use std::collections::HashSet;
use std::path::Path;

use surrealdb::Surreal;
use surrealdb::engine::local::{Db as LocalDb, Mem, RocksDb};

pub type Db = Surreal<LocalDb>;

/// RocksDB backed embedded 接続 (本番 / 永続化)。
pub async fn connect_rocksdb(path: &str, ns: &str, dbname: &str) -> anyhow::Result<Db> {
    let db = Surreal::new::<RocksDb>(path).await?;
    db.use_ns(ns).use_db(dbname).await?;
    Ok(db)
}

/// In-memory embedded 接続 (test / 揮発)。
pub async fn connect_mem(ns: &str, dbname: &str) -> anyhow::Result<Db> {
    let db = Surreal::new::<Mem>(()).await?;
    db.use_ns(ns).use_db(dbname).await?;
    Ok(db)
}

/// 未適用 migration を順次適用し、 適用した名前を返す。
///
/// - forward-only / 冪等 (migration は DEFINE ... OVERWRITE / IF NOT EXISTS で書く)
/// - fail-fast (statement error は `Response::check` で検出して即 Err)
/// - `_migrations` table で適用済みを追跡
pub async fn run_pending_migrations(db: &Db, migrations_dir: &Path) -> anyhow::Result<Vec<String>> {
    let applied = applied_migrations(db).await;

    let mut files: Vec<String> = std::fs::read_dir(migrations_dir)
        .map_err(|e| anyhow::anyhow!("read migrations dir {:?}: {e}", migrations_dir))?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".surql"))
        .collect();
    files.sort();

    let mut applied_now = Vec::new();
    for file in files {
        let name = file.trim_end_matches(".surql").to_string();
        if applied.contains(&name) {
            continue;
        }
        let path = migrations_dir.join(&file);
        let surql = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("read migration {file}: {e}"))?;

        tracing::info!(migration = %name, "applying migration");
        db.query(surql)
            .await
            .map_err(|e| anyhow::anyhow!("migration {name} failed: {e}"))?
            .check()
            .map_err(|e| anyhow::anyhow!("migration {name} returned errors: {e}"))?;

        db.query("CREATE _migrations SET name = $name, applied_at = time::now()")
            .bind(("name", name.clone()))
            .await?
            .check()?;
        applied_now.push(name);
    }

    Ok(applied_now)
}

/// `_migrations` から適用済み名を取得。 table 未定義などは空集合扱い。
async fn applied_migrations(db: &Db) -> HashSet<String> {
    match db.query("SELECT VALUE name FROM _migrations").await {
        Ok(mut res) => match res.take::<Vec<String>>(0) {
            Ok(names) => names.into_iter().collect(),
            Err(_) => HashSet::new(),
        },
        Err(_) => HashSet::new(),
    }
}
