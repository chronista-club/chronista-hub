/**
 * Startup Auto-Migrator。
 *
 * creo-memories (`apps/creo-app-server/src/lib/startup-migrator.ts`) からの移植・trim 版。
 * Rails の `rake db:migrate` on boot 踏襲。 server が HTTP listen を始める **前** に
 * 未適用 migration を順次 apply する。
 *
 * 規約:
 * - forward-only (rollback 非対応、 戻すなら新 migration で undo)
 * - 冪等 (DEFINE ... OVERWRITE / IF NOT EXISTS / WHERE 既存で保証)
 * - fail-fast (失敗で throw → 呼び元が process.exit(1))
 * - partial-apply 検出: statement 単位で ERR を検査、 AlreadyExists のみ許容 (CREO-127 系の学び)
 *
 * migration ファイルは `USE NS/DB` を書かない。 runner が wrap 注入する。
 */
import fs from 'node:fs/promises'
import path from 'node:path'

export interface StartupMigrationConfig {
  /** SurrealDB HTTP endpoint, e.g. 'http://localhost:8000' (/sql 末尾は付けない) */
  surrealHttpUrl: string
  namespace: string
  database: string
  rootUser: string
  rootPassword: string
  migrationsDir: string
  /** migration 1 つあたりの timeout (ms)、 default 120_000 */
  perMigrationTimeoutMs?: number
}

interface SurqlResponse {
  status: string
  time?: string
  result?: unknown
  kind?: string
  detail?: unknown
}

function log(level: 'info' | 'warn' | 'error', msg: string, extra?: unknown) {
  const line = `[hub:migrator] ${msg}`
  if (level === 'error') console.error(line, extra ?? '')
  else if (level === 'warn') console.warn(line, extra ?? '')
  else console.log(line)
}

/**
 * 未適用 migration を検出して順次 apply。
 * 成功: `{ applied, skipped }`。 失敗: throw (呼び元は process.exit(1) すべき)。
 */
export async function runPendingMigrations(
  config: StartupMigrationConfig
): Promise<{ applied: string[]; skipped: string[] }> {
  const applied = await getAppliedMigrations(config)
  log('info', `migration baseline: ${applied.size} applied`)

  const allFiles = (await fs.readdir(config.migrationsDir))
    .filter(f => f.endsWith('.surql'))
    .sort()

  const pending = allFiles.filter(f => !applied.has(f.replace(/\.surql$/, '')))

  if (pending.length === 0) {
    log('info', 'no pending migrations')
    return { applied: [], skipped: allFiles }
  }

  log('warn', `pending migrations: ${pending.join(', ')}`)

  const appliedThisRun: string[] = []
  for (const file of pending) {
    const name = file.replace(/\.surql$/, '')
    const filePath = path.join(config.migrationsDir, file)
    const surql = await fs.readFile(filePath, 'utf8')

    const fullSurql = `USE NS ${config.namespace} DB ${config.database};
${surql}
USE NS ${config.namespace} DB ${config.database};
CREATE _migrations SET name = '${name}', applied_at = time::now();`

    log('info', `applying migration: ${name}`)
    try {
      const result = await runSurql(config, fullSurql)
      assertNoRealErrors(name, result)
      appliedThisRun.push(name)
      log('info', `migration applied: ${name}`)
    } catch (err) {
      log('error', `migration failed — aborting: ${name}`, err)
      throw new Error(
        `startup migration ${name} failed: ${
          err instanceof Error ? err.message : String(err)
        }`
      )
    }
  }

  log('info', `all pending migrations applied: ${appliedThisRun.join(', ')}`)
  return { applied: appliedThisRun, skipped: [] }
}

async function getAppliedMigrations(
  config: StartupMigrationConfig
): Promise<Set<string>> {
  const sql = `USE NS ${config.namespace} DB ${config.database}; SELECT VALUE name FROM _migrations;`
  const result = await runSurql(config, sql)
  const selectRes = Array.isArray(result) ? result[1] : undefined
  if (
    !selectRes ||
    selectRes.status !== 'OK' ||
    !Array.isArray(selectRes.result)
  ) {
    // _migrations table が未存在等は空集合として扱う
    return new Set()
  }
  return new Set(selectRes.result as string[])
}

/** Raw SurrealQL を HTTP POST /sql に送信。 SurqlResponse[] を返す。 HTTP error は throw。 */
async function runSurql(
  config: StartupMigrationConfig,
  surql: string
): Promise<SurqlResponse[]> {
  const timeoutMs = config.perMigrationTimeoutMs ?? 120_000
  const ctl = new AbortController()
  const timer = setTimeout(() => ctl.abort(), timeoutMs)
  try {
    const res = await fetch(`${config.surrealHttpUrl}/sql`, {
      method: 'POST',
      headers: {
        Accept: 'application/json',
        Authorization: `Basic ${btoa(`${config.rootUser}:${config.rootPassword}`)}`,
      },
      body: surql,
      signal: ctl.signal,
    })
    if (!res.ok) {
      throw new Error(
        `SurrealDB HTTP ${res.status}: ${await res.text().catch(() => '')}`
      )
    }
    return (await res.json()) as SurqlResponse[]
  } finally {
    clearTimeout(timer)
  }
}

/** Migration 結果の ERR を検査し、 AlreadyExists 以外があれば throw。 */
function assertNoRealErrors(name: string, result: SurqlResponse[]): void {
  if (!Array.isArray(result)) return
  const realErrors = result.filter(r => {
    if (r.status !== 'ERR') return false
    const msg = String(r.result ?? '') + String(r.kind ?? '')
    return !/AlreadyExists/i.test(msg)
  })
  if (realErrors.length > 0) {
    const detail = realErrors
      .map(e => `${e.kind ?? 'Error'}: ${e.result}`)
      .join('\n  ')
    throw new Error(`migration ${name} returned errors:\n  ${detail}`)
  }
}

/**
 * WebSocket URL から HTTP URL を導出。
 * `ws://host:port/rpc` → `http://host:port`、 `wss://…` → `https://…`
 */
export function deriveSurrealHttpUrl(wsUrl: string): string {
  return wsUrl
    .replace(/^wss:\/\//, 'https://')
    .replace(/^ws:\/\//, 'http://')
    .replace(/\/rpc\/?$/, '')
}
