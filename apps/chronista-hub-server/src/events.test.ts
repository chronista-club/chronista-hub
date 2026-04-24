import { beforeEach, describe, expect, it } from 'bun:test'
import { startConsumer } from './consumer.js'
import {
  type EventEnvelope,
  InMemoryEventLog,
  validateEnvelope,
} from './event-log.js'
import { createEventsApp } from './events.js'
import { InMemoryStorage, type Resource } from './storage.js'

function makeResource(overrides: Partial<Resource> = {}): Resource {
  return {
    id: 'res_1',
    type: 'memories-atlas',
    path: '/memories/atlases/atlas_123',
    handle: 'mito',
    visibility: 'public',
    payload: { title: 'test atlas' },
    createdAt: '2026-04-25T00:00:00Z',
    updatedAt: '2026-04-25T00:00:00Z',
    ...overrides,
  }
}

function makeEvent(overrides: Partial<EventEnvelope> = {}): EventEnvelope {
  return {
    event_id: 'evt_1',
    app_id: 'memories',
    kind: 'resource.created',
    resource: makeResource(),
    idempotency: 'idem_1',
    emitted_at: '2026-04-25T00:00:00Z',
    ...overrides,
  }
}

// ================================================================
// validateEnvelope
// ================================================================

describe('validateEnvelope', () => {
  it('ok on valid envelope', () => {
    const result = validateEnvelope(makeEvent())
    expect(result.ok).toBe(true)
    expect(result.errors).toEqual([])
  })

  it('rejects non-object', () => {
    expect(validateEnvelope(null).ok).toBe(false)
    expect(validateEnvelope('string').ok).toBe(false)
    expect(validateEnvelope(42).ok).toBe(false)
  })

  it('reports missing event_id', () => {
    const { event_id: _, ...rest } = makeEvent()
    const result = validateEnvelope(rest)
    expect(result.ok).toBe(false)
    expect(result.errors.some(e => e.field === 'event_id')).toBe(true)
  })

  it('reports invalid kind', () => {
    const result = validateEnvelope({ ...makeEvent(), kind: 'bogus' })
    expect(result.ok).toBe(false)
    expect(result.errors.some(e => e.field === 'kind')).toBe(true)
  })

  it('reports nested resource.visibility', () => {
    const result = validateEnvelope({
      ...makeEvent(),
      resource: { ...makeResource(), visibility: 'invalid' },
    })
    expect(result.ok).toBe(false)
    expect(result.errors.some(e => e.field === 'resource.visibility')).toBe(
      true
    )
  })
})

// ================================================================
// InMemoryEventLog
// ================================================================

describe('InMemoryEventLog', () => {
  let log: InMemoryEventLog
  beforeEach(() => {
    log = new InMemoryEventLog()
  })

  it('accepts first event', async () => {
    const result = await log.append(makeEvent())
    expect(result.accepted).toBe(true)
    expect(log.size()).toBe(1)
  })

  it('rejects duplicate idempotency key', async () => {
    await log.append(makeEvent())
    const dup = await log.append(makeEvent({ event_id: 'evt_2' }))
    expect(dup.accepted).toBe(false)
    expect(dup.reason).toContain('idempotency')
    expect(log.size()).toBe(1)
  })

  it('rejects duplicate event_id (different idempotency)', async () => {
    await log.append(makeEvent())
    const dup = await log.append(
      makeEvent({ event_id: 'evt_1', idempotency: 'idem_2' })
    )
    expect(dup.accepted).toBe(false)
    expect(dup.reason).toContain('event_id')
  })

  it('unprocessed returns events in insert order', async () => {
    await log.append(makeEvent({ event_id: 'a', idempotency: 'a' }))
    await log.append(makeEvent({ event_id: 'b', idempotency: 'b' }))
    await log.append(makeEvent({ event_id: 'c', idempotency: 'c' }))
    const pending = await log.unprocessed()
    expect(pending.map(e => e.event_id)).toEqual(['a', 'b', 'c'])
  })

  it('markProcessed excludes from unprocessed', async () => {
    await log.append(makeEvent())
    await log.markProcessed('evt_1')
    const pending = await log.unprocessed()
    expect(pending.length).toBe(0)
  })
})

// ================================================================
// POST /events endpoint
// ================================================================

describe('POST /events', () => {
  it('202 on valid envelope', async () => {
    const log = new InMemoryEventLog()
    const app = createEventsApp(log)
    const res = await app.request('/events', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(makeEvent()),
    })
    expect(res.status).toBe(202)
    const body = await res.json()
    expect(body.accepted).toBe(true)
    expect(log.size()).toBe(1)
  })

  it('400 on invalid JSON', async () => {
    const log = new InMemoryEventLog()
    const app = createEventsApp(log)
    const res = await app.request('/events', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: 'not json',
    })
    expect(res.status).toBe(400)
  })

  it('400 on missing field', async () => {
    const log = new InMemoryEventLog()
    const app = createEventsApp(log)
    const { event_id: _, ...rest } = makeEvent()
    const res = await app.request('/events', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(rest),
    })
    expect(res.status).toBe(400)
    const body = await res.json()
    expect(
      body.details.some((e: { field: string }) => e.field === 'event_id')
    ).toBe(true)
  })

  it('409 on duplicate idempotency', async () => {
    const log = new InMemoryEventLog()
    const app = createEventsApp(log)
    await app.request('/events', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(makeEvent()),
    })
    const res2 = await app.request('/events', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(makeEvent({ event_id: 'evt_dup' })),
    })
    expect(res2.status).toBe(409)
  })
})

// ================================================================
// Consumer tick
// ================================================================

describe('consumer.tick', () => {
  it('applies resource.created → storage upsert', async () => {
    const log = new InMemoryEventLog()
    const storage = new InMemoryStorage()
    await log.append(makeEvent())
    const consumer = startConsumer(log, storage, { intervalMs: 3600_000 })
    try {
      const result = await consumer.tick()
      expect(result.processed).toBe(1)
      expect(result.errors).toBe(0)
      const r = await storage.getResourceById('res_1')
      expect(r).not.toBeNull()
      expect(r?.handle).toBe('mito')
    } finally {
      await consumer.stop()
    }
  })

  it('applies resource.deleted → storage remove', async () => {
    const log = new InMemoryEventLog()
    const storage = new InMemoryStorage()
    await storage.upsertResource(makeResource())
    await log.append(
      makeEvent({
        kind: 'resource.deleted',
        event_id: 'evt_del',
        idempotency: 'idem_del',
      })
    )
    const consumer = startConsumer(log, storage, { intervalMs: 3600_000 })
    try {
      await consumer.tick()
      const r = await storage.getResourceById('res_1')
      expect(r).toBeNull()
    } finally {
      await consumer.stop()
    }
  })

  it('markProcessed prevents re-apply', async () => {
    const log = new InMemoryEventLog()
    const storage = new InMemoryStorage()
    await log.append(makeEvent())
    const consumer = startConsumer(log, storage, { intervalMs: 3600_000 })
    try {
      const first = await consumer.tick()
      const second = await consumer.tick()
      expect(first.processed).toBe(1)
      expect(second.processed).toBe(0)
    } finally {
      await consumer.stop()
    }
  })
})
