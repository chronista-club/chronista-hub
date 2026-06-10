/**
 * SurrealEventLog integration test (TEST_INTEGRATION=1)。
 *   surreal start --user root --pass root --bind 127.0.0.1:8000 memory
 *   TEST_INTEGRATION=1 TEST_SURREALDB_URL=ws://127.0.0.1:8000/rpc bun test
 */
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  test,
} from 'bun:test'
import { join } from 'node:path'
import { deriveSurrealHttpUrl, runPendingMigrations } from './db/migrator.js'
import { SurrealPool } from './db/surreal-pool.js'
import type { EventEnvelope } from './event-log.js'
import { SurrealEventLog } from './surreal-event-log.js'

const describeIntegration = process.env.TEST_INTEGRATION
  ? describe
  : describe.skip

const WS_URL = process.env.TEST_SURREALDB_URL ?? 'ws://127.0.0.1:8000/rpc'
const HTTP_URL = deriveSurrealHttpUrl(WS_URL)
const ROOT_USER = process.env.TEST_SURREALDB_ROOT_USER ?? 'root'
const ROOT_PASS = process.env.TEST_SURREALDB_ROOT_PASS ?? 'root'
const REPO_MIGRATIONS = join(import.meta.dir, '../../../migrations')
const TEST_DB = `hub_event_${Date.now()}`

function makeEvent(over: Partial<EventEnvelope> = {}): EventEnvelope {
  const now = new Date().toISOString()
  return {
    event_id: 'ev_1',
    app_id: 'creo-memories',
    kind: 'resource.created',
    idempotency: 'idem_1',
    emitted_at: now,
    resource: {
      id: 'atlas_1',
      type: 'memories-atlas',
      path: '/creo-memories/atlas_1',
      handle: 'mito',
      visibility: 'public',
      payload: {},
      createdAt: now,
      updatedAt: now,
    },
    ...over,
  }
}

describeIntegration('SurrealEventLog (integration)', () => {
  let pool: SurrealPool
  let logStore: SurrealEventLog

  beforeAll(async () => {
    await runPendingMigrations({
      surrealHttpUrl: HTTP_URL,
      namespace: 'chronista',
      database: TEST_DB,
      rootUser: ROOT_USER,
      rootPassword: ROOT_PASS,
      migrationsDir: REPO_MIGRATIONS,
    })
    pool = new SurrealPool({
      url: WS_URL,
      namespace: 'chronista',
      database: TEST_DB,
      username: ROOT_USER,
      password: ROOT_PASS,
      authLevel: 'root',
    })
    logStore = new SurrealEventLog(pool)
  })

  afterAll(async () => {
    await pool.disconnect()
  })

  beforeEach(async () => {
    await pool.query('DELETE hub_event')
  })

  test('append → unprocessed で取れる', async () => {
    const res = await logStore.append(makeEvent())
    expect(res.accepted).toBe(true)
    const pending = await logStore.unprocessed()
    expect(pending.length).toBe(1)
    expect(pending[0].event_id).toBe('ev_1')
    expect(pending[0].resource.handle).toBe('mito')
  })

  test('idempotency 重複は reject', async () => {
    await logStore.append(makeEvent())
    const dup = await logStore.append(
      makeEvent({ event_id: 'ev_2', idempotency: 'idem_1' })
    )
    expect(dup.accepted).toBe(false)
    expect(dup.reason).toContain('idempotency')
  })

  test('event_id 重複は reject', async () => {
    await logStore.append(makeEvent())
    const dup = await logStore.append(
      makeEvent({ event_id: 'ev_1', idempotency: 'idem_2' })
    )
    expect(dup.accepted).toBe(false)
    expect(dup.reason).toContain('event_id')
  })

  test('markProcessed 後は unprocessed から外れる', async () => {
    await logStore.append(makeEvent())
    await logStore.markProcessed('ev_1')
    const pending = await logStore.unprocessed()
    expect(pending.length).toBe(0)
  })

  test('unprocessed は received_at 昇順', async () => {
    await logStore.append(makeEvent({ event_id: 'ev_1', idempotency: 'i1' }))
    await logStore.append(makeEvent({ event_id: 'ev_2', idempotency: 'i2' }))
    const pending = await logStore.unprocessed()
    expect(pending.map(e => e.event_id)).toEqual(['ev_1', 'ev_2'])
  })
})
