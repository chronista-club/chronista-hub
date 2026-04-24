/**
 * Health check sub-app — docker healthcheck + external monitoring 用。
 * 依存を持たず独立 test 可能 (疎結合)。
 */
import { Hono } from 'hono'

export interface HealthInfo {
  name: string
  version: string
}

export function createHealthApp(info: HealthInfo) {
  const app = new Hono()
  app.get('/', c =>
    c.json({
      status: 'ok',
      service: info.name,
      version: info.version,
      timestamp: new Date().toISOString(),
    })
  )
  return app
}
