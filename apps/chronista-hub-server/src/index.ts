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
import { startConsumer } from './consumer.js'
import { type EventLog, InMemoryEventLog } from './event-log.js'
import { createEventsApp } from './events.js'
import { createHealthApp, type HealthInfo } from './health.js'
import { InMemoryStorage, type MutableStorage } from './storage.js'
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
  const port = Number(process.env.CHRONISTA_HUB_PORT ?? 3000)
  const { app, storage, eventLog } = createApp()

  const consumer = startConsumer(eventLog, storage, { intervalMs: 1000 })
  console.log(`[${SERVICE_NAME}] listening on :${port} + consumer running`)

  const shutdown = async () => {
    console.log(`[${SERVICE_NAME}] shutting down...`)
    await consumer.stop()
    process.exit(0)
  }
  process.on('SIGINT', shutdown)
  process.on('SIGTERM', shutdown)

  Bun.serve({ port, fetch: app.fetch })
}
