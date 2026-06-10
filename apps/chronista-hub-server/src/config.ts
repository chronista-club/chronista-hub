/**
 * Server config — env var からの読み出しを 1 箇所に集約。
 *
 * SurrealDB 接続は creo-memories と同規約 (remote ws)。
 * env が揃っていなければ persistence なし (in-memory) で起動する。
 */
import type { SurrealDbConfig } from './db/surreal-pool.js'

export interface HubConfig {
  port: number
  /** SurrealDB 設定。 SURREALDB_URL 未設定なら null = in-memory モード */
  surreal: SurrealDbConfig | null
  /** root 認証 (migration runner 用)。 揃わなければ auto-migrate skip */
  rootUser?: string
  rootPassword?: string
  autoMigrate: boolean
  migrationsDir: string
}

function defaultMigrationsDir(): string {
  // repo root の migrations/。 app dir からの相対で解決。
  return process.env.MIGRATIONS_DIR ?? `${process.cwd()}/migrations`
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): HubConfig {
  const url = env.SURREALDB_URL
  const surreal: SurrealDbConfig | null = url
    ? {
        url,
        namespace: env.SURREALDB_NAMESPACE ?? 'chronista',
        database: env.SURREALDB_DATABASE ?? 'hub',
        username: env.SURREALDB_USERNAME,
        password: env.SURREALDB_PASSWORD,
        authLevel: (env.SURREALDB_AUTH_LEVEL as 'root' | 'database') ?? 'root',
      }
    : null

  return {
    port: Number(env.CHRONISTA_HUB_PORT ?? 3000),
    surreal,
    rootUser: env.SURREAL_ROOT_USER ?? env.SURREALDB_USERNAME,
    rootPassword: env.SURREAL_ROOT_PASSWORD ?? env.SURREALDB_PASSWORD,
    autoMigrate: env.AUTO_MIGRATE_ENABLED === 'true',
    migrationsDir: defaultMigrationsDir(),
  }
}
