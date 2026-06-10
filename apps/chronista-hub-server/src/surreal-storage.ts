/**
 * SurrealStorage — MutableStorage の SurrealDB-backed 実装。
 *
 * backing table: `hub_resource` (migration 003)。
 * record id = type::record('hub_resource', resource.id)、 resource id は `rid` field に複製。
 * InMemoryStorage (storage.ts) と振る舞い互換 (cursor は未使用、 visibility/type/limit で絞込)。
 */
import type { SurrealPool } from './db/surreal-pool.js'
import type {
  AppManifest,
  MutableStorage,
  Resource,
  TreeReadOptions,
  Visibility,
} from './storage.js'

interface ResourceRow {
  rid: string
  type: string
  path: string
  handle: string
  visibility: Visibility
  payload: Record<string, unknown>
  createdAt: string
  updatedAt: string
}

function toResource(row: ResourceRow): Resource {
  return {
    id: row.rid,
    type: row.type,
    path: row.path,
    handle: row.handle,
    visibility: row.visibility,
    payload: row.payload ?? {},
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  }
}

/** options から WHERE 追加条件 + bind vars + LIMIT 文字列を組む。 */
function buildFilter(options?: TreeReadOptions): {
  clause: string
  vars: Record<string, unknown>
  limit: string
} {
  const conds: string[] = []
  const vars: Record<string, unknown> = {}
  if (options?.visibility) {
    conds.push('visibility = $visibility')
    vars.visibility = options.visibility
  }
  if (options?.type) {
    conds.push('type = $type')
    vars.type = options.type
  }
  const clause = conds.length > 0 ? ` AND ${conds.join(' AND ')}` : ''
  const limit =
    typeof options?.limit === 'number' ? ` LIMIT ${options.limit}` : ''
  return { clause, vars, limit }
}

export class SurrealStorage implements MutableStorage {
  constructor(private readonly db: SurrealPool) {}

  async getResourcesByHandle(
    handle: string,
    options?: TreeReadOptions
  ): Promise<Resource[]> {
    const { clause, vars, limit } = buildFilter(options)
    const rows = await this.db.query<ResourceRow>(
      `SELECT * FROM hub_resource WHERE handle = $handle${clause} ORDER BY createdAt${limit}`,
      { handle, ...vars }
    )
    return rows.map(toResource)
  }

  async getResourcesByPath(
    handle: string,
    path: string,
    options?: TreeReadOptions
  ): Promise<Resource[]> {
    const normalized = path.startsWith('/') ? path : `/${path}`
    const { clause, vars, limit } = buildFilter(options)
    const rows = await this.db.query<ResourceRow>(
      `SELECT * FROM hub_resource WHERE handle = $handle AND string::starts_with(path, $path)${clause} ORDER BY createdAt${limit}`,
      { handle, path: normalized, ...vars }
    )
    return rows.map(toResource)
  }

  async getResourceById(id: string): Promise<Resource | null> {
    const row = await this.db.queryOne<ResourceRow>(
      'SELECT * FROM hub_resource WHERE rid = $id LIMIT 1',
      { id }
    )
    return row ? toResource(row) : null
  }

  async getAppManifest(appId: string): Promise<AppManifest | null> {
    // manifest の専用 ingestion path はまだ無い (ADR-009 Phase 2)。
    // 暫定: type='app' の resource を hub_resource から引いて payload を mapping。
    const row = await this.db.queryOne<ResourceRow>(
      "SELECT * FROM hub_resource WHERE rid = $id AND type = 'app' LIMIT 1",
      { id: appId }
    )
    if (!row) return null
    const p = row.payload ?? {}
    return {
      appId,
      name: typeof p.name === 'string' ? p.name : appId,
      version:
        typeof p.manifest_version === 'string' ? p.manifest_version : '0.0.0',
      permissions: Array.isArray(p.scopes) ? (p.scopes as string[]) : undefined,
    }
  }

  async upsertResource(resource: Resource): Promise<void> {
    await this.db.query(
      `UPSERT type::record('hub_resource', $rid) CONTENT {
        rid: $rid,
        type: $type,
        path: $path,
        handle: $handle,
        visibility: $visibility,
        payload: $payload,
        createdAt: $createdAt,
        updatedAt: $updatedAt
      }`,
      {
        rid: resource.id,
        type: resource.type,
        path: resource.path,
        handle: resource.handle,
        visibility: resource.visibility,
        payload: resource.payload ?? {},
        createdAt: resource.createdAt,
        updatedAt: resource.updatedAt,
      }
    )
  }

  async deleteResource(id: string): Promise<void> {
    await this.db.query("DELETE type::record('hub_resource', $rid)", {
      rid: id,
    })
  }
}
