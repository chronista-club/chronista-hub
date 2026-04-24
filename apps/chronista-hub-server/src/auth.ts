/**
 * Auth middleware (AC-17)
 *
 * Pluggable verifier 設計 — Creo ID JWKS (CREO-118 後) は本 module を差し替えず
 * `Verifier` interface を実装するだけで移行可能。 現 phase は Stub を提供、
 * dev / test で auth layer の shape を固定しておく。
 *
 * User token: Authorization: Bearer <jwt>
 * App token:  X-App-Token: <token>        (products から Hub への write で使用)
 *
 * Write endpoints (events) は `requireAuth()` を挟む、 read endpoints (tree)
 * は継続 public。 visibility filter は将来 AC-17 Phase 2 で middleware 化。
 */
import type { MiddlewareHandler } from 'hono'

export interface UserContext {
  kind: 'user'
  userId: string
  handle?: string
  /** optional scopes - 現 stub では空 */
  scopes?: string[]
}

export interface AppContext {
  kind: 'app'
  appId: string
  scopes: string[]
}

export type AuthPrincipal = UserContext | AppContext

export interface Verifier {
  /** Bearer JWT を検証して UserContext を返す。 invalid なら null */
  verifyUserToken(token: string): Promise<UserContext | null>
  /** X-App-Token を検証して AppContext を返す。 invalid なら null */
  verifyAppToken(token: string): Promise<AppContext | null>
}

// ============================================================
// Stub verifier — test / dev only、 JWT signature 検証は行わず payload をそのまま採用
// ============================================================

/**
 * Base64url decode (padding-tolerant、 polyfill 無しで動く)
 */
function decodeBase64Url(s: string): string {
  const padded = s.replace(/-/g, '+').replace(/_/g, '/')
  const missing = padded.length % 4
  const padEnd = missing > 0 ? padded + '='.repeat(4 - missing) : padded
  return atob(padEnd)
}

function decodeJwtPayload(token: string): Record<string, unknown> | null {
  const parts = token.split('.')
  if (parts.length !== 3) return null
  try {
    const json = decodeBase64Url(parts[1] ?? '')
    return JSON.parse(json) as Record<string, unknown>
  } catch {
    return null
  }
}

/**
 * StubVerifier — JWT の payload を decode するだけで verify しない。
 * 本番では絶対使わず、 CREO-118 の JwksVerifier に差し替えること。
 */
export class StubVerifier implements Verifier {
  async verifyUserToken(token: string): Promise<UserContext | null> {
    const payload = decodeJwtPayload(token)
    if (!payload) return null
    const userId = typeof payload.sub === 'string' ? payload.sub : null
    if (!userId) return null
    const handle =
      typeof payload.handle === 'string' ? payload.handle : undefined
    const scopes = Array.isArray(payload.scopes)
      ? (payload.scopes.filter(s => typeof s === 'string') as string[])
      : undefined
    return { kind: 'user', userId, handle, scopes }
  }

  async verifyAppToken(token: string): Promise<AppContext | null> {
    // 簡易フォーマット: "app:<appId>:<scope1>,<scope2>"
    const parts = token.split(':')
    if (parts.length !== 3 || parts[0] !== 'app') return null
    const appId = parts[1]
    if (!appId) return null
    const scopes = (parts[2] ?? '').split(',').filter(Boolean)
    return { kind: 'app', appId, scopes }
  }
}

// ============================================================
// Middleware
// ============================================================

export interface AuthMiddlewareOptions {
  verifier: Verifier
  /** どちらか片方あれば pass、 なければ 401 */
  accept?: ('user' | 'app')[]
  /** 必要 scope (app token に対してのみ evaluate、 全て満たす必要あり) */
  requiredScopes?: string[]
}

/**
 * Honoに principal を attach する middleware。
 * c.get('principal') で UserContext | AppContext を取得可能。
 */
export function requireAuth(options: AuthMiddlewareOptions): MiddlewareHandler {
  const accept = options.accept ?? ['user', 'app']

  return async (c, next) => {
    const bearer =
      c.req.header('authorization') ?? c.req.header('Authorization')
    const appTokenHeader =
      c.req.header('x-app-token') ?? c.req.header('X-App-Token')

    let principal: AuthPrincipal | null = null

    if (accept.includes('app') && appTokenHeader) {
      principal = await options.verifier.verifyAppToken(appTokenHeader)
      if (principal && options.requiredScopes) {
        const missing = options.requiredScopes.filter(
          s =>
            !principal ||
            principal.kind !== 'app' ||
            !principal.scopes.includes(s)
        )
        if (missing.length > 0) {
          return c.json(
            { error: 'insufficient scope', missing_scopes: missing },
            403
          )
        }
      }
    }

    if (
      !principal &&
      accept.includes('user') &&
      bearer?.startsWith('Bearer ')
    ) {
      const token = bearer.slice('Bearer '.length)
      principal = await options.verifier.verifyUserToken(token)
    }

    if (!principal) {
      return c.json({ error: 'unauthorized' }, 401)
    }

    // Hono v4: c.set で context 拡張
    c.set('principal', principal)
    await next()
  }
}

/**
 * `c.get('principal')` で取得する際に型を絞れるヘルパー。
 * 呼出側は `requireAuth` を通過した後に使う前提、 通過前 → unreachable。
 */
export function getPrincipal(c: {
  get: (key: 'principal') => AuthPrincipal | undefined
}): AuthPrincipal {
  const p = c.get('principal')
  if (!p) {
    throw new Error('getPrincipal called without requireAuth middleware')
  }
  return p
}
