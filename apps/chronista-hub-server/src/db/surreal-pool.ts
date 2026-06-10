/**
 * SurrealPool — 復元力のある共有 SurrealDB 接続。
 *
 * creo-memories (`packages/creo-memories/src/db/surreal-pool.ts`) からの移植・trim 版。
 * @creo/core logger / MemoryConfig 依存を外し、 console logging + 自前 config 型にした。
 *
 * heartbeat (60s) + proactive JWT refresh (50min) + exponential backoff 再接続で、
 * WebSocket 切断後も自動的に認証を復元する。
 */
import { Surreal } from 'surrealdb'

const HEARTBEAT_INTERVAL_MS = 60_000
/** JWT は SurrealDB 既定で 1h expire。 50min 毎に明示 re-signin して silent-degrade を予防。 */
const JWT_REFRESH_INTERVAL_MS = 50 * 60_000
const MAX_RECONNECT_ATTEMPTS = 5
const RECONNECT_BASE_DELAY_MS = 1_000

export interface SurrealDbConfig {
  /** ws://host:port/rpc or wss://… */
  url: string
  namespace: string
  database: string
  username?: string
  password?: string
  /** 'root' (default) signs in at root, otherwise namespace/database level */
  authLevel?: 'root' | 'database'
}

function log(level: 'info' | 'warn' | 'error', msg: string, extra?: unknown) {
  const line = `[hub:surreal-pool] ${msg}`
  if (level === 'error') console.error(line, extra ?? '')
  else if (level === 'warn') console.warn(line, extra ?? '')
  else console.log(line)
}

export class SurrealPool {
  private _client: Surreal | null = null
  private config: SurrealDbConfig
  private state: 'disconnected' | 'connecting' | 'connected' | 'error' =
    'disconnected'
  private connectionPromise: Promise<Surreal> | null = null
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null
  private lastSigninAt = 0

  constructor(config: SurrealDbConfig) {
    this.config = config
  }

  async getClient(): Promise<Surreal> {
    if (this.state === 'connected' && this._client) {
      return this._client
    }
    if (this.connectionPromise) {
      return this.connectionPromise
    }
    this.state = 'connecting'
    this.connectionPromise = this.doConnect()
    return this.connectionPromise
  }

  async connect(): Promise<void> {
    await this.getClient()
  }

  async disconnect(): Promise<void> {
    this.stopHeartbeat()
    if (this._client) {
      try {
        await this._client.close()
      } catch {
        // ignore
      }
      this._client = null
    }
    this.connectionPromise = null
    this.state = 'disconnected'
  }

  getState() {
    return this.state
  }

  /** クエリ実行（接続エラー時に自動再接続 + 1回リトライ）。結果は flat 化して返す。 */
  async query<T = unknown>(
    surql: string,
    vars?: Record<string, unknown>
  ): Promise<T[]> {
    try {
      const client = await this.getClient()
      const result = await client.query<T[]>(surql, vars)
      return result.flat() as T[]
    } catch (error) {
      if (this.isConnectionError(error)) {
        log('warn', 'connection error in query, reconnecting...')
        await this.forceReconnect()
        const client = await this.getClient()
        const result = await client.query<T[]>(surql, vars)
        return result.flat() as T[]
      }
      throw error
    }
  }

  async queryOne<T = unknown>(
    surql: string,
    vars?: Record<string, unknown>
  ): Promise<T | null> {
    const results = await this.query<T>(surql, vars)
    return results[0] ?? null
  }

  async healthCheck(): Promise<boolean> {
    try {
      const client = await this.getClient()
      await client.query('SELECT * FROM ONLY 1')
      return true
    } catch {
      return false
    }
  }

  private async doConnect(): Promise<Surreal> {
    try {
      const client = new Surreal()
      await client.connect(this.config.url)
      await this.signinWithConfig(client)
      await client.use({
        namespace: this.config.namespace,
        database: this.config.database,
      })

      this._client = client
      this.state = 'connected'
      this.connectionPromise = null
      this.startHeartbeat()

      log(
        'info',
        `connected to ${this.config.url} (${this.config.namespace}/${this.config.database})`
      )
      return client
    } catch (error) {
      this.state = 'error'
      this.connectionPromise = null
      log('error', 'connection failed', error)
      throw error
    }
  }

  private async signinWithConfig(client: Surreal): Promise<void> {
    if (!this.config.username || !this.config.password) return

    if (this.config.authLevel === 'database') {
      await client.signin({
        namespace: this.config.namespace,
        database: this.config.database,
        username: this.config.username,
        password: this.config.password,
      })
    } else {
      await client.signin({
        username: this.config.username,
        password: this.config.password,
      })
    }
    this.lastSigninAt = Date.now()
  }

  private startHeartbeat(): void {
    this.stopHeartbeat()
    this.heartbeatTimer = setInterval(() => {
      this.heartbeatTick().catch(err =>
        log('error', 'heartbeat tick (uncaught)', err)
      )
    }, HEARTBEAT_INTERVAL_MS)
    // Bun/Node: timer が event loop を pin しないように
    this.heartbeatTimer.unref?.()
  }

  private async heartbeatTick(): Promise<void> {
    if (!this._client) return

    if (Date.now() - this.lastSigninAt >= JWT_REFRESH_INTERVAL_MS) {
      try {
        await this.signinWithConfig(this._client)
        log('info', 'proactive auth refresh: signin renewed')
      } catch (refreshError) {
        log('warn', 'proactive auth refresh failed (will fall through)', {
          error:
            refreshError instanceof Error
              ? refreshError.message
              : String(refreshError),
        })
      }
    }

    try {
      await this._client.query('SELECT * FROM ONLY 1')
    } catch (error) {
      log('warn', 'heartbeat failed, reconnecting...', {
        error: error instanceof Error ? error.message : String(error),
      })
      try {
        await this.forceReconnect()
      } catch (reconnectError) {
        log('error', 'heartbeat reconnect failed', reconnectError)
      }
    }
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer)
      this.heartbeatTimer = null
    }
  }

  isConnectionError(error: unknown): boolean {
    if (!(error instanceof Error)) return false
    const msg = error.message.toLowerCase()
    const kind = (error as { kind?: string }).kind?.toLowerCase() ?? ''
    return (
      msg.includes('not allowed') ||
      msg.includes('anonymous access') ||
      msg.includes('not enough permissions') ||
      msg.includes('connection') ||
      msg.includes('websocket') ||
      msg.includes('socket') ||
      kind === 'notallowed' ||
      kind === 'connectionunavailable'
    )
  }

  async forceReconnect(): Promise<void> {
    this.stopHeartbeat()

    if (this._client) {
      try {
        await this._client.close()
      } catch {
        // ignore
      }
      this._client = null
    }
    this.connectionPromise = null
    this.state = 'disconnected'

    for (let attempt = 1; attempt <= MAX_RECONNECT_ATTEMPTS; attempt++) {
      try {
        log('info', `reconnect attempt ${attempt}/${MAX_RECONNECT_ATTEMPTS}...`)
        await this.connect()
        log('info', 'reconnected successfully')
        return
      } catch (error) {
        const delay = RECONNECT_BASE_DELAY_MS * 2 ** (attempt - 1)
        log(
          'error',
          `reconnect attempt ${attempt} failed, retry in ${delay}ms`,
          error
        )
        if (attempt < MAX_RECONNECT_ATTEMPTS) {
          await new Promise(resolve => setTimeout(resolve, delay))
        }
      }
    }

    throw new Error(
      `[SurrealPool] failed to reconnect after ${MAX_RECONNECT_ATTEMPTS} attempts`
    )
  }
}
