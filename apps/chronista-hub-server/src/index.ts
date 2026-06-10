/**
 * Chronista Hub Server
 *
 * World Tree meta-registry backend。 Phase 進行:
 *   - AC-14: scaffold + /health (Done)
 *   - AC-15: Tree read API v1 (Done)
 *   - AC-16: Event-sourced ingestion (Done)
 *   - AC-17: Auth middleware (pluggable Verifier + StubVerifier) (本 commit)
 *   - AC-18: Memories hub-sync (pilot integration)
 */
import { Hono } from 'hono'
import { StubVerifier, type Verifier } from './auth.js'
import { loadConfig } from './config.js'
import { startConsumer } from './consumer.js'
import { getDatabase } from './db/connection.js'
import { deriveSurrealHttpUrl, runPendingMigrations } from './db/migrator.js'
import { type EventLog, InMemoryEventLog } from './event-log.js'
import { createEventsApp } from './events.js'
import { createHealthApp, type HealthInfo } from './health.js'
import { InMemoryStorage, type MutableStorage } from './storage.js'
import { SurrealEventLog } from './surreal-event-log.js'
import { SurrealStorage } from './surreal-storage.js'
import { createTreeApp } from './tree.js'

const SERVICE_NAME = 'chronista-hub'
const VERSION = '0.0.1'

export interface AppOptions {
  info?: HealthInfo
  storage?: MutableStorage
  eventLog?: EventLog
  verifier?: Verifier
}

export function createApp(options: AppOptions = {}) {
  const info = options.info ?? { name: SERVICE_NAME, version: VERSION }
  const storage = options.storage ?? new InMemoryStorage()
  const eventLog = options.eventLog ?? new InMemoryEventLog()
  const verifier = options.verifier ?? new StubVerifier()

  const app = new Hono()
  app.route('/health', createHealthApp(info))
  app.route('/v1', createTreeApp(storage))
  app.route('/v1', createEventsApp({ log: eventLog, verifier }))
  app.get('/', c => c.json({ service: info.name, version: info.version }))
  return { app, storage, eventLog, verifier }
}

// Bun runtime エントリ
if (import.meta.main) {
  const config = loadConfig()

  // 1. 起動時 auto-migrate (HTTP listen 前、 失敗で fail-fast)
  if (config.autoMigrate) {
    if (!config.surreal || !config.rootUser || !config.rootPassword) {
      console.warn(
        `[${SERVICE_NAME}] AUTO_MIGRATE_ENABLED but SurrealDB url/root creds missing — skipping migrate`
      )
    } else {
      try {
        await runPendingMigrations({
          surrealHttpUrl: deriveSurrealHttpUrl(config.surreal.url),
          namespace: config.surreal.namespace,
          database: config.surreal.database,
          rootUser: config.rootUser,
          rootPassword: config.rootPassword,
          migrationsDir: config.migrationsDir,
        })
      } catch (err) {
        console.error(
          `[${SERVICE_NAME}] startup auto-migrate FAILED — refusing to listen`,
          err
        )
        process.exit(1)
      }
    }
  }

  // 2. storage / eventLog: SurrealDB 設定が揃えば永続化、 無ければ in-memory
  let appOptions: AppOptions = {}
  if (config.surreal) {
    const pool = getDatabase(config.surreal)
    appOptions = {
      storage: new SurrealStorage(pool),
      eventLog: new SurrealEventLog(pool),
    }
    console.log(
      `[${SERVICE_NAME}] persistence: SurrealDB (${config.surreal.namespace}/${config.surreal.database})`
    )
  } else {
    console.log(
      `[${SERVICE_NAME}] persistence: in-memory (SURREALDB_URL unset)`
    )
  }

  const { app, storage, eventLog } = createApp(appOptions)

  const consumer = startConsumer(eventLog, storage, { intervalMs: 1000 })
  console.log(
    `[${SERVICE_NAME}] listening on :${config.port} + consumer running`
  )

  const shutdown = async () => {
    console.log(`[${SERVICE_NAME}] shutting down...`)
    await consumer.stop()
    process.exit(0)
  }
  process.on('SIGINT', shutdown)
  process.on('SIGTERM', shutdown)

  Bun.serve({ port: config.port, fetch: app.fetch })
}
