/**
 * Chronista Hub Server (AC-14 Phase 1-1 scaffold)
 *
 * World Tree meta-registry backend。 現 phase は skeleton のみ、 後続 sub-issue で:
 *   - AC-15: Tree read API v1 (GET /v1/tree/@{handle}/... 等)
 *   - AC-16: Event-sourced ingestion (POST /v1/events + consumer)
 *   - AC-17: Auth middleware (Creo ID JWKS + product app-token)
 *   - AC-18: Memories hub-sync (pilot integration)
 */
import { Hono } from 'hono'
import { createHealthApp, type HealthInfo } from './health.js'

const SERVICE_NAME = 'chronista-hub'
const VERSION = '0.0.1'

export function createApp(
  info: HealthInfo = { name: SERVICE_NAME, version: VERSION }
) {
  const app = new Hono()
  app.route('/health', createHealthApp(info))
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
