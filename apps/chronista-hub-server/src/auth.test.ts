import { describe, expect, it } from 'bun:test'
import { Hono } from 'hono'
import {
  type AuthPrincipal,
  getPrincipal,
  requireAuth,
  StubVerifier,
} from './auth.js'

/** Minimal JWT helper — header.payload.signature の unsigned 構造だけ作る */
function makeStubJwt(payload: Record<string, unknown>): string {
  const header = btoa(JSON.stringify({ alg: 'none', typ: 'JWT' }))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  const body = btoa(JSON.stringify(payload))
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '')
  return `${header}.${body}.stub`
}

describe('StubVerifier.verifyUserToken', () => {
  const v = new StubVerifier()

  it('decodes sub from JWT payload', async () => {
    const token = makeStubJwt({ sub: 'user_123', handle: 'mito' })
    const ctx = await v.verifyUserToken(token)
    expect(ctx?.kind).toBe('user')
    expect(ctx?.userId).toBe('user_123')
    expect(ctx?.handle).toBe('mito')
  })

  it('returns null on missing sub', async () => {
    const token = makeStubJwt({ handle: 'only-handle' })
    expect(await v.verifyUserToken(token)).toBeNull()
  })

  it('returns null on malformed JWT (not 3 parts)', async () => {
    expect(await v.verifyUserToken('not.a.valid.jwt.too.many.parts')).toBeNull()
    expect(await v.verifyUserToken('only-one-part')).toBeNull()
  })

  it('extracts scopes if present', async () => {
    const token = makeStubJwt({
      sub: 'user_1',
      scopes: ['read:public', 'write:own'],
    })
    const ctx = await v.verifyUserToken(token)
    expect(ctx?.scopes).toEqual(['read:public', 'write:own'])
  })
})

describe('StubVerifier.verifyAppToken', () => {
  const v = new StubVerifier()

  it('decodes simple app:id:scope format', async () => {
    const ctx = await v.verifyAppToken(
      'app:memories:register:resource,read:public'
    )
    // Note: split(':') makes ambiguity — 現 stub は 3 segment まで見る
    expect(ctx).toBeNull() // 上は 5 segment で stub format 違反
  })

  it('accepts valid app:id:scopes', async () => {
    const ctx = await v.verifyAppToken(
      'app:memories:register_resource,read_public'
    )
    expect(ctx?.kind).toBe('app')
    expect(ctx?.appId).toBe('memories')
    expect(ctx?.scopes).toEqual(['register_resource', 'read_public'])
  })

  it('rejects non-app prefix', async () => {
    expect(await v.verifyAppToken('user:foo:bar')).toBeNull()
  })
})

describe('requireAuth middleware', () => {
  function makeTestApp(options: Parameters<typeof requireAuth>[0]) {
    const app = new Hono<{ Variables: { principal: AuthPrincipal } }>()
    app.use('/secure/*', requireAuth(options))
    app.get('/secure/me', c => {
      const principal = getPrincipal({ get: k => c.get(k) })
      return c.json({ principal })
    })
    app.get('/public/health', c => c.json({ ok: true }))
    return app
  }

  it('401 on missing header', async () => {
    const app = makeTestApp({ verifier: new StubVerifier() })
    const res = await app.request('/secure/me')
    expect(res.status).toBe(401)
  })

  it('401 on malformed Bearer', async () => {
    const app = makeTestApp({ verifier: new StubVerifier() })
    const res = await app.request('/secure/me', {
      headers: { authorization: 'NotBearer xxx' },
    })
    expect(res.status).toBe(401)
  })

  it('200 + principal attached on valid user JWT', async () => {
    const app = makeTestApp({ verifier: new StubVerifier() })
    const token = makeStubJwt({ sub: 'user_1', handle: 'mito' })
    const res = await app.request('/secure/me', {
      headers: { authorization: `Bearer ${token}` },
    })
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.principal.kind).toBe('user')
    expect(body.principal.userId).toBe('user_1')
  })

  it('200 + app principal via X-App-Token', async () => {
    const app = makeTestApp({ verifier: new StubVerifier() })
    const res = await app.request('/secure/me', {
      headers: { 'x-app-token': 'app:memories:register_resource' },
    })
    expect(res.status).toBe(200)
    const body = await res.json()
    expect(body.principal.kind).toBe('app')
    expect(body.principal.appId).toBe('memories')
  })

  it('403 on app token missing required scope', async () => {
    const app = makeTestApp({
      verifier: new StubVerifier(),
      requiredScopes: ['register_resource', 'admin_privilege'],
    })
    const res = await app.request('/secure/me', {
      headers: { 'x-app-token': 'app:memories:register_resource' }, // admin_privilege 不在
    })
    expect(res.status).toBe(403)
    const body = await res.json()
    expect(body.missing_scopes).toContain('admin_privilege')
  })

  it('accept=[user] のみ指定時 app-token は reject (→ user 401)', async () => {
    const app = makeTestApp({
      verifier: new StubVerifier(),
      accept: ['user'],
    })
    const res = await app.request('/secure/me', {
      headers: { 'x-app-token': 'app:memories:register_resource' },
    })
    expect(res.status).toBe(401)
  })

  it('does not affect public routes', async () => {
    const app = makeTestApp({ verifier: new StubVerifier() })
    const res = await app.request('/public/health')
    expect(res.status).toBe(200)
  })
})
