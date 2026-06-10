/**
 * SurrealStorage integration test (TEST_INTEGRATION=1)。
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
import type { Resource } from './storage.js'
import { SurrealStorage } from './surreal-storage.js'

const describeIntegration = process.env.TEST_INTEGRATION
  ? describe
  : describe.skip

const WS_URL = process.env.TEST_SURREALDB_URL ?? 'ws://127.0.0.1:8000/rpc'
const HTTP_URL = deriveSurrealHttpUrl(WS_URL)
const ROOT_USER = process.env.TEST_SURREALDB_ROOT_USER ?? 'root'
const ROOT_PASS = process.env.TEST_SURREALDB_ROOT_PASS ?? 'root'
const REPO_MIGRATIONS = join(import.meta.dir, '../../../migrations')
const TEST_DB = `hub_store_${Date.now()}`

function makeResource(over: Partial<Resource> = {}): Resource {
  const now = new Date().toISOString()
  return {
    id: 'atlas_1',
    type: 'memories-atlas',
    path: '/creo-memories/atlas_1',
    handle: 'mito',
    visibility: 'public',
    payload: { title: 'My Atlas' },
    createdAt: now,
    updatedAt: now,
    ...over,
  }
}

describeIntegration('SurrealStorage (integration)', () => {
  let pool: SurrealPool
  let storage: SurrealStorage

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
    storage = new SurrealStorage(pool)
  })

  afterAll(async () => {
    await pool.disconnect()
  })

  beforeEach(async () => {
    await pool.query('DELETE hub_resource')
  })

  test('upsert → getResourceById で取り出せる', async () => {
    const r = makeResource()
    await storage.upsertResource(r)
    const got = await storage.getResourceById('atlas_1')
    expect(got).not.toBeNull()
    expect(got?.id).toBe('atlas_1')
    expect(got?.handle).toBe('mito')
    expect(got?.payload.title).toBe('My Atlas')
  })

  test('getResourcesByHandle が handle 一致を返す + visibility filter', async () => {
    await storage.upsertResource(
      makeResource({ id: 'a', visibility: 'public' })
    )
    await storage.upsertResource(
      makeResource({ id: 'b', visibility: 'private' })
    )
    const all = await storage.getResourcesByHandle('mito')
    expect(all.length).toBe(2)
    const pub = await storage.getResourcesByHandle('mito', {
      visibility: 'public',
    })
    expect(pub.length).toBe(1)
    expect(pub[0].id).toBe('a')
  })

  test('getResourcesByPath が prefix 一致を返す', async () => {
    await storage.upsertResource(
      makeResource({ id: 'a', path: '/creo-memories/atlas_1' })
    )
    await storage.upsertResource(makeResource({ id: 'b', path: '/vp/world_1' }))
    const got = await storage.getResourcesByPath('mito', '/creo-memories')
    expect(got.length).toBe(1)
    expect(got[0].id).toBe('a')
  })

  test('upsert で同 id を更新できる', async () => {
    await storage.upsertResource(makeResource({ payload: { title: 'v1' } }))
    await storage.upsertResource(makeResource({ payload: { title: 'v2' } }))
    const got = await storage.getResourceById('atlas_1')
    expect(got?.payload.title).toBe('v2')
    const all = await storage.getResourcesByHandle('mito')
    expect(all.length).toBe(1)
  })

  test('delete で消える', async () => {
    await storage.upsertResource(makeResource())
    await storage.deleteResource('atlas_1')
    const got = await storage.getResourceById('atlas_1')
    expect(got).toBeNull()
  })
})
