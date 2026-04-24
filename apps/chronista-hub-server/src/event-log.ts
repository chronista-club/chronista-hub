/**
 * EventLog — products から受けた event envelope の buffer。
 *
 * MVP: in-memory、 idempotency key で dedup、 append / unprocessed / mark の
 * 3 操作のみ。 consumer (poll loop) が unprocessed を拾って storage に適用。
 *
 * AC-16b で SurrealDB `hub_event` table backed に差替予定。
 */
import type { Resource } from './storage.js'

export type EventKind =
  | 'resource.created'
  | 'resource.updated'
  | 'resource.deleted'

export interface EventEnvelope {
  event_id: string
  app_id: string
  kind: EventKind
  resource: Resource
  idempotency: string
  emitted_at: string
}

export interface StoredEvent extends EventEnvelope {
  /** Unix ms — append された時刻 (insert order proxy) */
  received_at: number
  processed_at?: number
}

export interface EventLog {
  /** append — idempotency 衝突時は `{ accepted: false }` を返す */
  append(event: EventEnvelope): Promise<{ accepted: boolean; reason?: string }>
  /** 未処理 event を insert 順に返す */
  unprocessed(limit?: number): Promise<StoredEvent[]>
  /** 処理済とマーク */
  markProcessed(event_id: string): Promise<void>
  /** idempotency key が既に存在するか */
  hasIdempotency(key: string): Promise<boolean>
}

export class InMemoryEventLog implements EventLog {
  private events = new Map<string, StoredEvent>()
  private idempotencyKeys = new Set<string>()

  async append(
    event: EventEnvelope
  ): Promise<{ accepted: boolean; reason?: string }> {
    if (this.idempotencyKeys.has(event.idempotency)) {
      return { accepted: false, reason: 'duplicate idempotency key' }
    }
    if (this.events.has(event.event_id)) {
      return { accepted: false, reason: 'duplicate event_id' }
    }
    this.events.set(event.event_id, {
      ...event,
      received_at: Date.now(),
    })
    this.idempotencyKeys.add(event.idempotency)
    return { accepted: true }
  }

  async unprocessed(limit?: number): Promise<StoredEvent[]> {
    const pending = Array.from(this.events.values())
      .filter(e => e.processed_at === undefined)
      .sort((a, b) => a.received_at - b.received_at)
    return typeof limit === 'number' ? pending.slice(0, limit) : pending
  }

  async markProcessed(event_id: string): Promise<void> {
    const event = this.events.get(event_id)
    if (event) {
      event.processed_at = Date.now()
    }
  }

  async hasIdempotency(key: string): Promise<boolean> {
    return this.idempotencyKeys.has(key)
  }

  /** test / debug */
  size(): number {
    return this.events.size
  }
}

// --- Envelope validation ---

export interface ValidationError {
  field: string
  message: string
}

export function validateEnvelope(input: unknown): {
  ok: boolean
  envelope?: EventEnvelope
  errors: ValidationError[]
} {
  const errors: ValidationError[] = []
  if (input === null || typeof input !== 'object') {
    return { ok: false, errors: [{ field: '', message: 'must be object' }] }
  }
  const obj = input as Record<string, unknown>

  const eventId = typeof obj.event_id === 'string' ? obj.event_id : null
  if (!eventId) errors.push({ field: 'event_id', message: 'required string' })

  const appId = typeof obj.app_id === 'string' ? obj.app_id : null
  if (!appId) errors.push({ field: 'app_id', message: 'required string' })

  const kind =
    obj.kind === 'resource.created' ||
    obj.kind === 'resource.updated' ||
    obj.kind === 'resource.deleted'
      ? obj.kind
      : null
  if (!kind)
    errors.push({
      field: 'kind',
      message: 'must be resource.created | resource.updated | resource.deleted',
    })

  const idempotency =
    typeof obj.idempotency === 'string' ? obj.idempotency : null
  if (!idempotency)
    errors.push({ field: 'idempotency', message: 'required string' })

  const emittedAt = typeof obj.emitted_at === 'string' ? obj.emitted_at : null
  if (!emittedAt)
    errors.push({ field: 'emitted_at', message: 'required ISO 8601 string' })

  const resource = validateResource(obj.resource)
  if (!resource.ok) {
    errors.push(
      ...resource.errors.map(e => ({ ...e, field: `resource.${e.field}` }))
    )
  }

  if (errors.length > 0) {
    return { ok: false, errors }
  }

  // TypeScript narrowing: at this point all required fields exist
  return {
    ok: true,
    envelope: {
      event_id: eventId as string,
      app_id: appId as string,
      kind: kind as EventKind,
      idempotency: idempotency as string,
      emitted_at: emittedAt as string,
      resource: resource.resource as Resource,
    },
    errors: [],
  }
}

function validateResource(input: unknown): {
  ok: boolean
  resource?: Resource
  errors: ValidationError[]
} {
  const errors: ValidationError[] = []
  if (input === null || typeof input !== 'object') {
    return { ok: false, errors: [{ field: '', message: 'must be object' }] }
  }
  const r = input as Record<string, unknown>
  const required = ['id', 'type', 'path', 'handle', 'visibility'] as const
  for (const k of required) {
    if (typeof r[k] !== 'string') {
      errors.push({ field: k, message: 'required string' })
    }
  }
  if (
    r.visibility !== 'public' &&
    r.visibility !== 'shared' &&
    r.visibility !== 'private'
  ) {
    errors.push({
      field: 'visibility',
      message: 'must be public | shared | private',
    })
  }
  if (typeof r.payload !== 'object' || r.payload === null) {
    errors.push({ field: 'payload', message: 'must be object' })
  }
  if (typeof r.createdAt !== 'string') {
    errors.push({ field: 'createdAt', message: 'required ISO 8601 string' })
  }
  if (typeof r.updatedAt !== 'string') {
    errors.push({ field: 'updatedAt', message: 'required ISO 8601 string' })
  }
  if (errors.length > 0) return { ok: false, errors }
  return { ok: true, resource: r as unknown as Resource, errors: [] }
}
