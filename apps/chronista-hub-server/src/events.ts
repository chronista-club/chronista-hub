/**
 * Events API (AC-16 + AC-17)
 *
 * POST /events — products から event envelope を受けて EventLog に append。
 * 202 Accepted で async 処理、 400 で validation 違反、 409 で idempotency 衝突。
 * 401 で auth 不在、 403 で scope 不足。
 *
 * 実際の storage 反映は consumer (consumer.ts) が行う。
 */
import { Hono } from 'hono'
import { requireAuth, type Verifier } from './auth.js'
import { type EventLog, validateEnvelope } from './event-log.js'

export interface EventsAppOptions {
  log: EventLog
  /** optional: 指定時は auth middleware を掛ける (推奨) */
  verifier?: Verifier
  /** app token に要求する scope (default: ['register_resource']) */
  requiredScopes?: string[]
}

export function createEventsApp(options: EventsAppOptions | EventLog) {
  const app = new Hono()

  // Back-compat: EventLog を直接渡された場合は auth なしで動く (dev only)
  const opts: EventsAppOptions =
    'append' in options ? { log: options } : options

  if (opts.verifier) {
    app.use(
      '/events',
      requireAuth({
        verifier: opts.verifier,
        accept: ['user', 'app'],
        requiredScopes: opts.requiredScopes ?? ['register_resource'],
      })
    )
  }

  app.post('/events', async c => {
    let body: unknown
    try {
      body = await c.req.json()
    } catch {
      return c.json({ error: 'invalid JSON' }, 400)
    }

    const validation = validateEnvelope(body)
    if (!validation.ok || !validation.envelope) {
      return c.json(
        { error: 'validation failed', details: validation.errors },
        400
      )
    }

    const result = await opts.log.append(validation.envelope)
    if (!result.accepted) {
      return c.json(
        { error: 'conflict', reason: result.reason ?? 'duplicate' },
        409
      )
    }

    return c.json(
      { accepted: true, event_id: validation.envelope.event_id },
      202
    )
  })

  return app
}
