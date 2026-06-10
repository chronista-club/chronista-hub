//! Event consumer — EventLog から unprocessed を poll し storage に適用。 TS `consumer.ts` 相当。
//!
//! MVP: 単純 poll + 逐次 apply。 後続で Live Query / batch / retry-with-backoff に拡張可能。

use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::event_log::EventLog;
use crate::model::{EventKind, StoredEvent};
use crate::storage::Storage;

/// apply をこの回数失敗したら dead-letter 化する (poison pill 回避)。
const MAX_RETRIES: i64 = 5;

/// 1 event を kind に応じて storage に適用。
pub async fn apply_event(event: &StoredEvent, storage: &Storage) -> anyhow::Result<()> {
    match event.envelope.kind {
        EventKind::ResourceCreated | EventKind::ResourceUpdated => {
            storage.upsert_resource(&event.envelope.resource).await
        }
        EventKind::ResourceDeleted => storage.delete_resource(&event.envelope.resource.id).await,
    }
}

/// 1 回の poll + apply。 (processed, errors) を返す。 test から直接呼べる。
pub async fn tick(log: &EventLog, storage: &Storage, batch: usize) -> (usize, usize) {
    let pending = match log.unprocessed(Some(batch)).await {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(error = %err, "consumer: unprocessed poll failed");
            return (0, 1);
        }
    };
    let mut processed = 0;
    let mut errors = 0;
    for ev in pending {
        match apply_event(&ev, storage).await {
            Ok(()) => match log.mark_processed(&ev.envelope.event_id).await {
                Ok(()) => processed += 1,
                Err(err) => {
                    tracing::error!(event_id = %ev.envelope.event_id, error = %err, "consumer: mark_processed failed");
                    errors += 1;
                }
            },
            Err(err) => {
                tracing::error!(event_id = %ev.envelope.event_id, error = %err, "consumer: apply failed");
                // 失敗を記録 (retry_count++、 上限で dead-letter 化して poison pill を防ぐ)
                if let Err(e) = log
                    .record_failure(&ev.envelope.event_id, &err.to_string(), MAX_RETRIES)
                    .await
                {
                    tracing::error!(event_id = %ev.envelope.event_id, error = %e, "consumer: record_failure failed");
                }
                errors += 1;
            }
        }
    }
    (processed, errors)
}

pub struct ConsumerHandle {
    stop: watch::Sender<bool>,
    join: JoinHandle<()>,
}

impl ConsumerHandle {
    /// 停止 (pending poll は完走してから return)。
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.join.await;
    }
}

/// poll loop を spawn して handle を返す。
pub fn spawn_consumer(log: EventLog, storage: Storage, interval_ms: u64) -> ConsumerHandle {
    let (stop, mut rx) = watch::channel(false);
    let join = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_millis(interval_ms));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let _ = tick(&log, &storage, 100).await;
                }
                _ = rx.changed() => {
                    if *rx.borrow() {
                        break;
                    }
                }
            }
        }
    });
    ConsumerHandle { stop, join }
}
