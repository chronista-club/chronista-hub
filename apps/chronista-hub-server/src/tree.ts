/**
 * Tree read API v1 (AC-15)
 *
 * 疎結合 sub-app — Storage interface にのみ依存、 DB 実装は caller 側で inject。
 * 本 phase は 4 endpoint の shape + validation + status code を確定、
 * AC-16 で storage を SurrealDB-backed に差し替え。
 *
 * Endpoints:
 *   GET /tree/@:handle              - handle の root subtree
 *   GET /tree/@:handle/*path        - 指定 path の subtree
 *   GET /resources/:id              - 単一 resource
 *   GET /apps/:appId/manifest       - app manifest
 */
import { Hono } from 'hono'
import type { Storage, Visibility } from './storage.js'

/** world-tree.kdl spec 準拠: ^[a-z0-9][a-z0-9-]{0,30}$ */
const HANDLE_PATTERN = /^[a-z0-9][a-z0-9-]{0,30}$/

/** visibility query param を safe に parse */
function parseVisibility(v: string | undefined): Visibility | undefined {
  if (v === 'public' || v === 'shared' || v === 'private') return v
  return undefined
}

export function createTreeApp(storage: Storage) {
  const app = new Hono()

  // GET /tree/@:handle
  app.get('/tree/:handleSlug{@[a-z0-9-]+}', async c => {
    const handle = c.req.param('handleSlug').slice(1) // strip `@`
    if (!HANDLE_PATTERN.test(handle)) {
      return c.json({ error: 'invalid handle' }, 400)
    }
    const resources = await storage.getResourcesByHandle(handle, {
      visibility: parseVisibility(c.req.query('visibility')),
      type: c.req.query('type'),
    })
    return c.json({ handle, path: '/', resources })
  })

  // GET /tree/@:handle/*path
  app.get('/tree/:handleSlug{@[a-z0-9-]+}/:path{.+}', async c => {
    const handle = c.req.param('handleSlug').slice(1)
    const path = c.req.param('path')
    if (!HANDLE_PATTERN.test(handle)) {
      return c.json({ error: 'invalid handle' }, 400)
    }
    const resources = await storage.getResourcesByPath(handle, path, {
      visibility: parseVisibility(c.req.query('visibility')),
      type: c.req.query('type'),
    })
    return c.json({ handle, path: `/${path}`, resources })
  })

  // GET /resources/:id
  app.get('/resources/:id', async c => {
    const id = c.req.param('id')
    const resource = await storage.getResourceById(id)
    if (!resource) return c.json({ error: 'not found' }, 404)
    return c.json({ resource })
  })

  // GET /apps/:appId/manifest
  app.get('/apps/:appId/manifest', async c => {
    const appId = c.req.param('appId')
    const manifest = await storage.getAppManifest(appId)
    if (!manifest) return c.json({ error: 'not found' }, 404)
    return c.json({ manifest })
  })

  return app
}
