//! EventLog — `hub_event` table への append/poll。 TS `SurrealEventLog` と振る舞い互換。
//!
//! record id = type::record('hub_event', event_id)。 idempotency / event_id で dedup、
//! unprocessed は received_at 昇順。

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

use crate::db::Db;
use crate::model::{EventEnvelope, StoredEvent};

pub struct AppendResult {
    pub accepted: bool,
    pub reason: Option<String>,
}

/// CREATE 時の content (envelope + received_at。 processed_at は省略 = NONE)。
#[derive(Serialize)]
struct EventRowContent<'a> {
    #[serde(flatten)]
    envelope: &'a EventEnvelope,
    received_at: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct EventLog {
    db: Db,
}

impl EventLog {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn append(&self, event: &EventEnvelope) -> anyhow::Result<AppendResult> {
        if self.has_idempotency(&event.idempotency).await? {
            return Ok(AppendResult {
                accepted: false,
                reason: Some("duplicate idempotency key".into()),
            });
        }
        let existing: Vec<String> = self
            .db
            .query("SELECT VALUE event_id FROM hub_event WHERE event_id = $event_id LIMIT 1")
            .bind(("event_id", event.event_id.clone()))
            .await?
            .take(0)?;
        if !existing.is_empty() {
            return Ok(AppendResult {
                accepted: false,
                reason: Some("duplicate event_id".into()),
            });
        }

        let content = EventRowContent {
            envelope: event,
            received_at: now_ms(),
        };
        self.db
            .query("CREATE type::record('hub_event', $event_id) CONTENT $content")
            .bind(("event_id", event.event_id.clone()))
            .bind(("content", serde_json::to_value(&content)?))
            .await?
            .check()?;
        Ok(AppendResult {
            accepted: true,
            reason: None,
        })
    }

    pub async fn unprocessed(&self, limit: Option<usize>) -> anyhow::Result<Vec<StoredEvent>> {
        let limit_clause = match limit {
            Some(l) => format!(" LIMIT {l}"),
            None => String::new(),
        };
        let sql = format!(
            "SELECT * FROM hub_event WHERE processed_at IS NONE ORDER BY received_at{limit_clause}"
        );
        let rows: Vec<Value> = self.db.query(sql).await?.take(0)?;
        rows.into_iter()
            .map(|v| serde_json::from_value::<StoredEvent>(v).map_err(anyhow::Error::from))
            .collect()
    }

    pub async fn mark_processed(&self, event_id: &str) -> anyhow::Result<()> {
        self.db
            .query("UPDATE type::record('hub_event', $event_id) SET processed_at = $now")
            .bind(("event_id", event_id.to_string()))
            .bind(("now", now_ms()))
            .await?
            .check()?;
        Ok(())
    }

    pub async fn has_idempotency(&self, key: &str) -> anyhow::Result<bool> {
        let rows: Vec<String> = self
            .db
            .query("SELECT VALUE idempotency FROM hub_event WHERE idempotency = $key LIMIT 1")
            .bind(("key", key.to_string()))
            .await?
            .take(0)?;
        Ok(!rows.is_empty())
    }
}
