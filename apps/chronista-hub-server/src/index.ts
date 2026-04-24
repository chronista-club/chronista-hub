/**
 * Chronista Hub Server
 *
 * World Tree meta-registry backend。 Phase 進行:
 *   - AC-14: scaffold + /health (Done)
 *   - AC-15: Tree read API v1 (/v1/tree/@{handle}/... 等) (本 commit)
 *   - AC-16: Event-sourced ingestion (POST /v1/events + consumer)
 *   - AC-17: Auth middleware (Creo ID JWKS + product app-token)
 *   - AC-18: Memories hub-sync (pilot integration)
 */
import { Hono } from 'hono'
import { createHealthApp, type HealthInfo } from './health.js'
import { InMemoryStorage, type Storage } from './storage.js'
import { createTreeApp } from './tree.js'

const SERVICE_NAME = 'chronista-hub'
const VERSION = '0.0.1'

export interface AppOptions {
  info?: HealthInfo
  storage?: Storage
}

export function createApp(options: AppOptions = {}) {
  const info = options.info ?? { name: SERVICE_NAME, version: VERSION }
  const storage = options.storage ?? new InMemoryStorage()

  const app = new Hono()
  app.route('/health', createHealthApp(info))
  app.route('/v1', createTreeApp(storage))
  app.get('/', c => c.json({ service: info.name, version: info.version }))
  return app
}

// Bun runtime エントリ (test では import のみ、 実行はしない)
if (import.meta.main) {
  const port = Number(process.env.CHRONISTA_HUB_PORT ?? 3000)
  const app = createApp()
  console.log(`[${SERVICE_NAME}] listening on :${port}`)
  Bun.serve({ port, fetch: app.fetch })
}
