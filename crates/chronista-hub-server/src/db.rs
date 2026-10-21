//! SurrealDB 接続 (URL で embedded / remote を切替) + 起動時 in-process migrator。
//!
//! ADR-016 は in-process embedded (kv-rocksdb) を採用した。 ADR-022 で接続先を URL 化し、
//! 本番は remote (`ws://`) へ寄せて SurrealDB Studio から直接繋げるようにした。
//! `rocksdb://` を渡せば従来どおり embedded で動く (手元 / e2e)。
//! migration はどちらでも HTTP `/sql` ではなく SDK 経由で `migrations/*.surql` を順次適用する。

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use surrealdb::Surreal;
use surrealdb::engine::any::{self, Any};
use surrealdb::opt::auth::{Database, Namespace, Root};
use tokio::sync::{Mutex, Notify};

use crate::config::{DbAuth, DbAuthLevel};

/// SDK の接続 1 本 (= サーバ上のセッション 1 つ)。
pub type Conn = Surreal<Any>;

/// Hub の DB への入口 (incident 2026-10-15、 ADR-022 追記)。
///
/// - **接続を共有する**: `Db` の clone は `Arc` の clone で、 SDK の `Surreal::clone()` を呼ばない。
///   SDK 3.x の `Surreal::clone()` はサーバ上に新しいセッションを作る (Attach / signin / use の
///   replay) ので、 axum が要求ごとに state を clone すると要求ごとにセッションが生まれては消えていた。
///   2026-10-15 の障害は「起動時に作ったセッションは動き、 新しく作るセッションだけ Session not
///   found になる」形だった。 1 本を使い回せば、 壊れるときは全部一緒に壊れ、 見張りが気づける
/// - **見張りと作り直し**: [`Db::check`] が DB に問い合わせ、 失敗したら (remote のときだけ) 接続を
///   作り直す。 [`Db::spawn_watchdog`] が定期的に、 [`Db::kick`] が要求の失敗をきっかけにすぐ走らせる
/// - **状態を外に見せる**: [`Db::health`] を `/health` が返す
#[derive(Clone)]
pub struct Db {
    inner: Arc<Inner>,
}

struct Inner {
    conn: RwLock<Arc<Conn>>,
    target: Target,
    generation: AtomicU64,
    health: RwLock<DbHealth>,
    reconnecting: Mutex<()>,
    kick: Notify,
}

/// つなぎ直すのに要るもの。 password は Debug に出さない (DbAuth の Debug が伏せる)
#[derive(Clone, Debug)]
struct Target {
    url: String,
    auth: Option<DbAuth>,
    ns: String,
    db: String,
}

/// DB の状態 (`/health` の `db`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DbHealth {
    /// 最後の点検で問い合わせが通ったか (点検前は true)
    pub ok: bool,
    /// 最後に点検した時刻 (unix ms)。 点検前は None
    pub checked_at: Option<i64>,
    /// 最後に失敗したときの壊れ方 (`connection` / `session` / `auth` / `timeout` / `error`)。
    /// 回復しても残す。 生の文言は log にだけ出す (外に DB の中身を見せない)
    pub last_error: Option<String>,
    pub last_error_at: Option<i64>,
    /// 起動してから接続を作り直した回数
    pub reconnects: u64,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("target", &self.inner.target)
            .field("generation", &self.generation())
            .finish()
    }
}

/// 点検の問い合わせ。 認証と ns / db の選択が生きていないと通らないものにする
/// (`RETURN 1` はセッションが壊れていても通りうる)
const PING: &str = "INFO FOR DB";
const PING_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// kick で連続して点検しないための間隔
const MIN_CHECK_GAP: Duration = Duration::from_secs(1);

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// URL で接続し、 `auth` があれば signin してから ns / db を選ぶ。
///
/// - `rocksdb://<dir>`: embedded 永続 (signin 不要)
/// - `ws://host:port` / `wss://...`: remote (`auth` 必須運用)
/// - `mem://`: embedded 揮発 (test)
pub async fn connect(
    url: &str,
    auth: Option<&DbAuth>,
    ns: &str,
    dbname: &str,
) -> anyhow::Result<Db> {
    let target = Target {
        url: url.to_string(),
        auth: auth.cloned(),
        ns: ns.to_string(),
        db: dbname.to_string(),
    };
    let conn = open(&target).await?;
    Ok(Db {
        inner: Arc::new(Inner {
            conn: RwLock::new(Arc::new(conn)),
            target,
            generation: AtomicU64::new(0),
            health: RwLock::new(DbHealth {
                ok: true,
                checked_at: None,
                last_error: None,
                last_error_at: None,
                reconnects: 0,
            }),
            reconnecting: Mutex::new(()),
            kick: Notify::new(),
        }),
    })
}

/// 新しい接続を開く (signin と ns / db の選択まで)。
async fn open(t: &Target) -> anyhow::Result<Conn> {
    let db = any::connect(t.url.as_str()).await?;
    if let Some(auth) = &t.auth {
        let (username, password) = (auth.username.clone(), auth.password.clone());
        match auth.level {
            DbAuthLevel::Root => db.signin(Root { username, password }).await?,
            DbAuthLevel::Namespace => {
                db.signin(Namespace {
                    namespace: t.ns.clone(),
                    username,
                    password,
                })
                .await?
            }
            DbAuthLevel::Database => {
                db.signin(Database {
                    namespace: t.ns.clone(),
                    database: t.db.clone(),
                    username,
                    password,
                })
                .await?
            }
        };
    }
    db.use_ns(t.ns.as_str()).use_db(t.db.as_str()).await?;
    Ok(db)
}

/// In-memory embedded 接続 (test / 揮発)。
pub async fn connect_mem(ns: &str, dbname: &str) -> anyhow::Result<Db> {
    connect("mem://", None, ns, dbname).await
}

impl Db {
    /// 今の接続。 `Arc` の clone なので安く、 SDK のセッションは増えない。
    /// 呼び出しの間だけ持つ (保持し続けると、 作り直したあとも古い接続を使い続ける)
    pub fn conn(&self) -> Arc<Conn> {
        Arc::clone(&self.inner.conn.read().unwrap_or_else(|e| e.into_inner()))
    }

    /// 接続を作り直した回数 (作り直すたびに 1 増える)
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::SeqCst)
    }

    /// remote (ws / http) か。 embedded (mem / rocksdb) は作り直さない
    /// (mem を開き直すと空の DB になる)
    pub fn is_remote(&self) -> bool {
        let url = self.inner.target.url.as_str();
        ["ws://", "wss://", "http://", "https://"]
            .iter()
            .any(|p| url.starts_with(p))
    }

    pub fn health(&self) -> DbHealth {
        self.inner
            .health
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 点検の問い合わせを 1 回 (timeout 付き)。
    pub async fn ping(&self) -> anyhow::Result<()> {
        let conn = self.conn();
        match tokio::time::timeout(PING_TIMEOUT, conn.query(PING)).await {
            Ok(r) => {
                r?.check()?;
                Ok(())
            }
            Err(_) => anyhow::bail!("db ping timed out after {}s", PING_TIMEOUT.as_secs()),
        }
    }

    /// 接続を作り直す (remote のときだけ)。 `seen` は呼び出し側が見た generation。
    /// その間に別の誰かが作り直していたら何もしない (同時の作り直しを 1 回にまとめる)
    pub async fn reconnect_from(&self, seen: u64) -> anyhow::Result<()> {
        if !self.is_remote() {
            return Ok(());
        }
        let _one_at_a_time = self.inner.reconnecting.lock().await;
        if self.generation() != seen {
            return Ok(());
        }
        let fresh = tokio::time::timeout(CONNECT_TIMEOUT, open(&self.inner.target))
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "db reconnect timed out after {}s",
                    CONNECT_TIMEOUT.as_secs()
                )
            })??;
        *self.inner.conn.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(fresh);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner
            .health
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .reconnects += 1;
        tracing::warn!(url = %self.inner.target.url, "db: reconnected (new connection and session)");
        Ok(())
    }

    /// 点検: 問い合わせて、 失敗したら (remote なら) 作り直してもう一度問い合わせる。
    /// 結果を health に書き、 状態が変わったときだけ log を出す。
    pub async fn check(&self) -> DbHealth {
        let seen = self.generation();
        let result = match self.ping().await {
            Ok(()) => Ok(()),
            Err(e) if self.is_remote() => {
                tracing::warn!(error = %e, "db: ping failed; reconnecting");
                match self.reconnect_from(seen).await {
                    Ok(()) => self.ping().await,
                    Err(re) => Err(re),
                }
            }
            Err(e) => Err(e),
        };
        self.record(result)
    }

    pub(crate) fn record(&self, result: anyhow::Result<()>) -> DbHealth {
        let mut h = self.inner.health.write().unwrap_or_else(|e| e.into_inner());
        let was_ok = h.ok;
        h.checked_at = Some(now_ms());
        match result {
            Ok(()) => {
                h.ok = true;
                if !was_ok {
                    tracing::info!(reconnects = h.reconnects, "db: recovered");
                }
            }
            Err(e) => {
                h.ok = false;
                h.last_error = Some(kind_of(&e).unwrap_or("error").to_string());
                h.last_error_at = h.checked_at;
                if was_ok {
                    tracing::error!(error = %e, "db: unavailable");
                }
            }
        }
        h.clone()
    }

    /// 見張りにすぐ点検させる (要求が DB のエラーで失敗したとき)。 何度呼んでも 1 回にまとまる
    pub fn kick(&self) {
        self.inner.kick.notify_one();
    }

    /// 見張りを起こす: `every` ごと、 または [`Db::kick`] で点検する。
    pub fn spawn_watchdog(&self, every: Duration) -> tokio::task::JoinHandle<()> {
        let db = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(every);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // 初回は起動時に点検済み
            loop {
                tokio::select! {
                    _ = ticker.tick() => {}
                    _ = db.inner.kick.notified() => {}
                }
                db.check().await;
                tokio::time::sleep(MIN_CHECK_GAP).await;
            }
        })
    }
}

/// DB に届かない・セッションが壊れている類のエラーか (要求を 503 にし、 見張りを起こす)。
///
/// まず SDK の型 (`surrealdb::Error`) で分ける: 接続のエラー、 セッションが無い、 セッション /
/// token の期限切れ。 型が落ちた (文字列に包まれた) エラーは文言で分ける (SDK 3.3.4 / core 3.3.4
/// に実在する文言、 2026-10-21 に確認)。 作り直しそのものはこの分類に依らない
/// ([`Db::check`] は点検の失敗なら何でも作り直す)。 ここで取りこぼしても、 回復が定期点検まで
/// 遅れるだけ
pub fn is_unavailable(err: &anyhow::Error) -> bool {
    kind_of(err).is_some()
}

/// 壊れ方の分類 (`/health` に出す短い語。 SDK の生の文言は log にだけ出す)
fn kind_of(err: &anyhow::Error) -> Option<&'static str> {
    use surrealdb::types::{AuthError, NotAllowedError, NotFoundError};
    for e in err.chain() {
        if let Some(se) = e.downcast_ref::<surrealdb::Error>() {
            if se.is_connection() {
                return Some("connection");
            }
            if matches!(se.not_found_details(), Some(NotFoundError::Session { .. })) {
                return Some("session");
            }
            if matches!(
                se.not_allowed_details(),
                Some(NotAllowedError::Auth(
                    AuthError::SessionExpired | AuthError::TokenExpired
                ))
            ) {
                return Some("auth");
            }
        }
    }
    const MARKERS: &[(&str, &str)] = &[
        ("Session not found", "session"),
        ("The session has expired", "auth"),
        // Hub は必ずサービス用の利用者で signin している。 匿名扱いになったら、
        // セッションが認証を失った = 接続が壊れている
        ("Anonymous access not allowed", "auth"),
        ("Failed to send command", "connection"),
        ("Connection uninitialised", "connection"),
        ("connection was closed", "connection"),
        ("Connection refused", "connection"),
        ("Connection reset", "connection"),
        ("db ping timed out", "timeout"),
        ("db reconnect timed out", "timeout"),
    ];
    err.chain().map(|e| e.to_string()).find_map(|m| {
        MARKERS
            .iter()
            .find(|(k, _)| m.contains(k))
            .map(|(_, kind)| *kind)
    })
}

/// 未適用 migration を順次適用し、 適用した名前を返す。
///
/// - forward-only / 冪等 (migration は DEFINE ... OVERWRITE / IF NOT EXISTS で書く)
/// - fail-fast (statement error は `Response::check` で検出して即 Err)
/// - `_migrations` table で適用済みを追跡
pub async fn run_pending_migrations(db: &Db, migrations_dir: &Path) -> anyhow::Result<Vec<String>> {
    let conn = db.conn();
    let applied = applied_migrations(&conn).await;

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
        conn.query(surql)
            .await
            .map_err(|e| anyhow::anyhow!("migration {name} failed: {e}"))?
            .check()
            .map_err(|e| anyhow::anyhow!("migration {name} returned errors: {e}"))?;

        conn.query("CREATE _migrations SET name = $name, applied_at = time::now()")
            .bind(("name", name.clone()))
            .await?
            .check()?;
        applied_now.push(name);
    }

    Ok(applied_now)
}

/// `_migrations` から適用済み名を取得。 table 未定義などは空集合扱い。
async fn applied_migrations(conn: &Conn) -> HashSet<String> {
    match conn.query("SELECT VALUE name FROM _migrations").await {
        Ok(mut res) => match res.take::<Vec<String>>(0) {
            Ok(names) => names.into_iter().collect(),
            Err(_) => HashSet::new(),
        },
        Err(_) => HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_errors_are_told_apart_from_query_errors() {
        use surrealdb::types::{AuthError, ConnectionError, NotAllowedError, NotFoundError};
        // SDK の型で分かるもの
        let typed: Vec<(anyhow::Error, &str)> = vec![
            (
                surrealdb::Error::not_found(
                    "Session not found: 47a7bd1b".into(),
                    NotFoundError::Session { id: None },
                )
                .into(),
                "session",
            ),
            (
                surrealdb::Error::connection("IO error".into(), ConnectionError::ConnectionFailed)
                    .into(),
                "connection",
            ),
            (
                surrealdb::Error::not_allowed(
                    "The session has expired".into(),
                    NotAllowedError::Auth(AuthError::SessionExpired),
                )
                .into(),
                "auth",
            ),
        ];
        for (e, kind) in typed {
            assert_eq!(kind_of(&e), Some(kind), "{e}");
            assert!(is_unavailable(&e.context("list_active_apps")));
        }
        // 文字列に包まれたもの (SDK 3.3.4 / core 3.3.4 の実在の文言)
        for (m, kind) in [
            (
                "Session not found: 47a7bd1b-dbd6-4a7f-bc2c-5f258570c2da",
                "session",
            ),
            ("The session has expired", "auth"),
            (
                "Anonymous access not allowed: Not enough permissions to perform this action",
                "auth",
            ),
            ("Failed to send command: channel closed", "connection"),
            ("Connection uninitialised", "connection"),
            ("IO error: Connection refused (os error 61)", "connection"),
            ("db ping timed out after 5s", "timeout"),
        ] {
            assert_eq!(kind_of(&anyhow::anyhow!(m.to_string())), Some(kind), "{m}");
        }
        // DB には届いている (問い合わせ自体の失敗)。 値に紛れた語でも反応しない
        for m in [
            "Database index `user_handle_idx` already contains 'mito'",
            "Parse error: Unexpected token",
            "IAM error: Not enough permissions to perform this action",
            "Found 'WebSocket' for field `url`, but field must conform to: string::starts_with($value, 'https://')",
        ] {
            assert!(!is_unavailable(&anyhow::anyhow!(m.to_string())), "{m}");
        }
    }

    #[tokio::test]
    async fn clones_share_one_connection() {
        let db = connect_mem("chronista", "t").await.unwrap();
        let a = db.clone();
        assert!(
            Arc::ptr_eq(&db.conn(), &a.conn()),
            "a clone must not open a new SDK session"
        );
        assert!(!db.is_remote());
    }

    #[tokio::test]
    async fn embedded_checks_pass_and_never_reconnect() {
        let db = connect_mem("chronista", "t").await.unwrap();
        assert!(db.health().checked_at.is_none());
        let h = db.check().await;
        assert!(h.ok, "{h:?}");
        assert!(h.checked_at.is_some());
        db.reconnect_from(db.generation()).await.unwrap();
        assert_eq!(db.generation(), 0, "embedded engines are never reopened");
        assert_eq!(db.health().reconnects, 0);
    }
}
