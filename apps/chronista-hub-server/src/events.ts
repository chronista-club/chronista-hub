/**
 * Events API (AC-16)
 *
 * POST /events — products から event envelope を受けて EventLog に append。
 * 202 Accepted で async 処理、 400 で validation 違反、 409 で idempotency 衝突。
 * 実際の storage 反映は consumer (consumer.ts) が行う。
 */
import { Hono } from 'hono'
import { type EventLog, validateEnvelope } from './event-log.js'

export function createEventsApp(log: EventLog) {
  const app = new Hono()

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

    const result = await log.append(validation.envelope)
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
