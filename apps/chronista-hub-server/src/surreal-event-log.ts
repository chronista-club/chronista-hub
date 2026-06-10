/**
 * SurrealEventLog — EventLog の SurrealDB-backed 実装。
 *
 * backing table: `hub_event` (migration 003)。
 * record id = type::record('hub_event', event_id)。
 * InMemoryEventLog (event-log.ts) と振る舞い互換 (idempotency / event_id で dedup、
 * unprocessed は received_at 昇順)。
 */
import type { SurrealPool } from './db/surreal-pool.js'
import type { EventEnvelope, EventLog, StoredEvent } from './event-log.js'

interface EventRow extends EventEnvelope {
  received_at: number
  processed_at?: number | null
}

function toStoredEvent(row: EventRow): StoredEvent {
  const stored: StoredEvent = {
    event_id: row.event_id,
    app_id: row.app_id,
    kind: row.kind,
    resource: row.resource,
    idempotency: row.idempotency,
    emitted_at: row.emitted_at,
    received_at: row.received_at,
  }
  if (typeof row.processed_at === 'number') {
    stored.processed_at = row.processed_at
  }
  return stored
}

export class SurrealEventLog implements EventLog {
  constructor(private readonly db: SurrealPool) {}

  async append(
    event: EventEnvelope
  ): Promise<{ accepted: boolean; reason?: string }> {
    if (await this.hasIdempotency(event.idempotency)) {
      return { accepted: false, reason: 'duplicate idempotency key' }
    }
    const existingId = await this.db.queryOne<EventRow>(
      'SELECT event_id FROM hub_event WHERE event_id = $event_id LIMIT 1',
      { event_id: event.event_id }
    )
    if (existingId) {
      return { accepted: false, reason: 'duplicate event_id' }
    }

    await this.db.query(
      `CREATE type::record('hub_event', $event_id) CONTENT {
        event_id: $event_id,
        app_id: $app_id,
        kind: $kind,
        resource: $resource,
        idempotency: $idempotency,
        emitted_at: $emitted_at,
        received_at: $received_at,
        processed_at: NONE
      }`,
      {
        event_id: event.event_id,
        app_id: event.app_id,
        kind: event.kind,
        resource: event.resource,
        idempotency: event.idempotency,
        emitted_at: event.emitted_at,
        received_at: Date.now(),
      }
    )
    return { accepted: true }
  }

  async unprocessed(limit?: number): Promise<StoredEvent[]> {
    const limitClause = typeof limit === 'number' ? ` LIMIT ${limit}` : ''
    const rows = await this.db.query<EventRow>(
      `SELECT * FROM hub_event WHERE processed_at IS NONE ORDER BY received_at${limitClause}`
    )
    return rows.map(toStoredEvent)
  }

  async markProcessed(event_id: string): Promise<void> {
    await this.db.query(
      "UPDATE type::record('hub_event', $event_id) SET processed_at = $now",
      { event_id, now: Date.now() }
    )
  }

  async hasIdempotency(key: string): Promise<boolean> {
    const row = await this.db.queryOne<{ idempotency: string }>(
      'SELECT idempotency FROM hub_event WHERE idempotency = $key LIMIT 1',
      { key }
    )
    return row !== null
  }
}
