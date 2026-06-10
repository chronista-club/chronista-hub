/**
 * SurrealPool singleton。 storage / event-log が共有する 1 接続を提供する。
 */
import { type SurrealDbConfig, SurrealPool } from './surreal-pool.js'

let instance: SurrealPool | null = null

/** config から singleton を取得 (初回のみ生成)。 */
export function getDatabase(config: SurrealDbConfig): SurrealPool {
  if (!instance) {
    instance = new SurrealPool(config)
  }
  return instance
}

/** test 用: singleton を差し替え / リセット。 */
export function setDatabaseForTest(pool: SurrealPool | null): void {
  instance = pool
}
