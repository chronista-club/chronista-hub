/**
 * migrator integration test (TEST_INTEGRATION=1 で有効化)。
 *
 * 実 SurrealDB が必要:
 *   surreal start --user root --pass root --bind 127.0.0.1:8000 memory
 *   TEST_INTEGRATION=1 TEST_SURREALDB_URL=ws://127.0.0.1:8000/rpc bun test
 */
import { afterAll, beforeAll, describe, expect, test } from 'bun:test'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { deriveSurrealHttpUrl, runPendingMigrations } from './migrator.js'

const describeIntegration = process.env.TEST_INTEGRATION
  ? describe
  : describe.skip

const WS_URL = process.env.TEST_SURREALDB_URL ?? 'ws://127.0.0.1:8000/rpc'
const HTTP_URL = deriveSurrealHttpUrl(WS_URL)
const ROOT_USER = process.env.TEST_SURREALDB_ROOT_USER ?? 'root'
const ROOT_PASS = process.env.TEST_SURREALDB_ROOT_PASS ?? 'root'
const REPO_MIGRATIONS = join(import.meta.dir, '../../../../migrations')

// 各 test run を分離するため uniq な DB を使う
const TEST_DB = `hub_test_${Date.now()}`

function cfg(migrationsDir: string) {
  return {
    surrealHttpUrl: HTTP_URL,
    namespace: 'chronista',
    database: TEST_DB,
    rootUser: ROOT_USER,
    rootPassword: ROOT_PASS,
    migrationsDir,
  }
}

async function rawSql(sql: string): Promise<unknown[]> {
  const res = await fetch(`${HTTP_URL}/sql`, {
    method: 'POST',
    headers: {
      Accept: 'application/json',
      Authorization: `Basic ${btoa(`${ROOT_USER}:${ROOT_PASS}`)}`,
    },
    body: `USE NS chronista DB ${TEST_DB};\n${sql}`,
  })
  return (await res.json()) as unknown[]
}

describeIntegration('startup migrator (integration)', () => {
  beforeAll(async () => {
    // 接続確認
    const res = await fetch(`${HTTP_URL}/health`).catch(() => null)
    if (!res) throw new Error(`SurrealDB not reachable at ${HTTP_URL}`)
  })

  afterAll(async () => {
    await rawSql(`REMOVE DATABASE IF EXISTS ${TEST_DB};`).catch(() => {})
  })

  test('repo の全 migration を順次適用し _migrations に記録する', async () => {
    const result = await runPendingMigrations(cfg(REPO_MIGRATIONS))
    // 001..004 の 4 本
    expect(result.applied).toContain('001_bootstrap')
    expect(result.applied).toContain('002_resource_types_from_spec')
    expect(result.applied).toContain('003_hub_internal_tables')
    expect(result.applied).toContain('004_reserved_handles_seed')
    expect(result.applied.length).toBe(4)
  })

  test('2 回目は冪等 — pending 0', async () => {
    const result = await runPendingMigrations(cfg(REPO_MIGRATIONS))
    expect(result.applied.length).toBe(0)
  })

  test('reserved handle が 12 件 seed される', async () => {
    const res = (await rawSql(
      "SELECT count() AS c FROM user WHERE account_type = 'reserved' GROUP ALL;"
    )) as Array<{ status: string; result: Array<{ c: number }> }>
    const last = res[res.length - 1]
    expect(last.status).toBe('OK')
    expect(last.result[0].c).toBeGreaterThanOrEqual(12)
  })

  test('壊れた migration は throw し fail-fast する', async () => {
    const dir = await mkdtemp(join(tmpdir(), 'hub-mig-'))
    await writeFile(
      join(dir, '900_broken.surql'),
      'THIS IS NOT VALID SURQL ;;;'
    )
    const badDb = `hub_bad_${Date.now()}`
    await expect(
      runPendingMigrations({
        surrealHttpUrl: HTTP_URL,
        namespace: 'chronista',
        database: badDb,
        rootUser: ROOT_USER,
        rootPassword: ROOT_PASS,
        migrationsDir: dir,
      })
    ).rejects.toThrow()
  })
})
