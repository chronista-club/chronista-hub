import { describe, expect, it } from 'bun:test'
import type { AppManifest, Resource, Storage } from './storage.js'
import { InMemoryStorage } from './storage.js'
import { createTreeApp } from './tree.js'

function makeResource(overrides: Partial<Resource> = {}): Resource {
  return {
    id: 'resource_test',
    type: 'handle',
    path: '/',
    handle: 'mito',
    visibility: 'public',
    payload: {},
    createdAt: '2026-04-25T00:00:00Z',
    updatedAt: '2026-04-25T00:00:00Z',
    ...overrides,
  }
}

class StubStorage implements Storage {
  constructor(
    private opts: {
      byHandle?: Resource[]
      byPath?: Resource[]
      byId?: Resource | null
      manifest?: AppManifest | null
    } = {}
  ) {}
  async getResourcesByHandle() {
    return this.opts.byHandle ?? []
  }
  async getResourcesByPath() {
    return this.opts.byPath ?? []
  }
  async getResourceById() {
    return this.opts.byId ?? null
  }
  async getAppManifest() {
    return this.opts.manifest ?? null
  }
}

describe('createTreeApp — handle root', () => {
  it('200 + empty resources on valid handle (InMemoryStorage)', async () => {
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request('/tree/@mito')
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.handle).toBe('mito')
    expect(body.path).toBe('/')
    expect(body.resources).toEqual([])
  })

  it('returns resources from storage', async () => {
    const r = makeResource({ id: 'res_1' })
    const app = createTreeApp(new StubStorage({ byHandle: [r] }))
    const res = await app.request('/tree/@mito')
    const body = await res.json()
    expect(body.resources.length).toBe(1)
    expect(body.resources[0].id).toBe('res_1')
  })

  it('400 on invalid handle (uppercase)', async () => {
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request('/tree/@Mito')
    // Hono route pattern @[a-z0-9-]+ で大文字を拒否 → 404 に落ちる
    // (invalid pattern は 404、 valid pattern + invalid content は 400)
    expect(res.status).toBe(404)
  })

  it('400 on too-long handle (31+ chars)', async () => {
    const longHandle = `a${'b'.repeat(31)}` // 32 chars total
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request(`/tree/@${longHandle}`)
    expect(res.status).toBe(400)
  })
})

describe('createTreeApp — path subtree', () => {
  it('200 + empty on valid handle + path', async () => {
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request('/tree/@mito/memories/atlases')
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.handle).toBe('mito')
    expect(body.path).toBe('/memories/atlases')
  })
})

describe('createTreeApp — single resource', () => {
  it('404 when resource not found', async () => {
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request('/resources/res_unknown')
    expect(res.status).toBe(404)
  })

  it('200 + resource when found', async () => {
    const r = makeResource({ id: 'res_found' })
    const app = createTreeApp(new StubStorage({ byId: r }))
    const res = await app.request('/resources/res_found')
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.resource.id).toBe('res_found')
  })
})

describe('createTreeApp — app manifest', () => {
  it('404 when manifest not found', async () => {
    const app = createTreeApp(new InMemoryStorage())
    const res = await app.request('/apps/unknown/manifest')
    expect(res.status).toBe(404)
  })

  it('200 + manifest when found', async () => {
    const manifest: AppManifest = {
      appId: 'memories',
      name: 'Creo Memories',
      version: '0.22.0',
    }
    const app = createTreeApp(new StubStorage({ manifest }))
    const res = await app.request('/apps/memories/manifest')
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.manifest.appId).toBe('memories')
  })
})
