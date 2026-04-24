/**
 * Event consumer (AC-16)
 *
 * EventLog から unprocessed event を poll し、 kind に応じて storage に apply。
 * Poll interval は option で指定、 startConsumer は { stop } を返して停止可能。
 *
 * MVP は simple poll + sequential apply (1 event ずつ)。 後続 PR で Live Query
 * 化 / batch / retry-with-backoff を追加可能。
 */
import type { EventLog, StoredEvent } from './event-log.js'
import type { MutableStorage } from './storage.js'

export interface ConsumerOptions {
  /** poll 間隔 ms (default: 1000) */
  intervalMs?: number
  /** 1 poll で処理する最大件数 (default: 100) */
  batchSize?: number
  /** エラー logger (default: console.error) */
  onError?: (error: unknown, event?: StoredEvent) => void
}

export interface ConsumerHandle {
  /** 停止 (pending poll は完走してから return) */
  stop(): Promise<void>
  /** 手動で 1 回 poll + apply (test 用) */
  tick(): Promise<{ processed: number; errors: number }>
}

export function startConsumer(
  log: EventLog,
  storage: MutableStorage,
  options: ConsumerOptions = {}
): ConsumerHandle {
  const intervalMs = options.intervalMs ?? 1000
  const batchSize = options.batchSize ?? 100
  const onError =
    options.onError ??
    ((err, ev) =>
      console.error('[hub-consumer] apply failed', ev?.event_id, err))

  let running = true
  let inflight: Promise<unknown> | null = null
  // wake signal — stop() 呼出時に sleep を即座に resolve できるよう
  let wakeResolve: (() => void) | null = null

  async function tick(): Promise<{ processed: number; errors: number }> {
    const pending = await log.unprocessed(batchSize)
    let processed = 0
    let errors = 0
    for (const event of pending) {
      try {
        await applyEvent(event, storage)
        await log.markProcessed(event.event_id)
        processed += 1
      } catch (err) {
        errors += 1
        onError(err, event)
      }
    }
    return { processed, errors }
  }

  async function loop() {
    while (running) {
      try {
        await tick()
      } catch (err) {
        onError(err)
      }
      if (!running) break
      await new Promise<void>(resolve => {
        wakeResolve = resolve
        const timer = setTimeout(resolve, intervalMs)
        // timer 掃除: wake called 時は即 resolve、 重複 resolve は無害
        wakeResolve = () => {
          clearTimeout(timer)
          resolve()
        }
      })
      wakeResolve = null
    }
  }

  inflight = loop()

  return {
    async stop() {
      running = false
      wakeResolve?.()
      await inflight
    },
    tick,
  }
}

async function applyEvent(
  event: StoredEvent,
  storage: MutableStorage
): Promise<void> {
  switch (event.kind) {
    case 'resource.created':
    case 'resource.updated':
      await storage.upsertResource(event.resource)
      return
    case 'resource.deleted':
      await storage.deleteResource(event.resource.id)
      return
  }
}
