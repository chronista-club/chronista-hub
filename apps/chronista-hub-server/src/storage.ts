/**
 * Hub Storage interface + in-memory stub。
 *
 * 本 phase (AC-15/16) では DB 未接続。 AC-16 で in-memory 実装を extend し
 * `upsertResource` / `deleteResource` を追加、 event consumer がこれらを呼ぶ。
 * 実 SurrealDB 差替は後続 (AC-16b / 将来 Issue)。
 */

export type Visibility = 'public' | 'shared' | 'private'

export interface Resource {
  id: string
  type: string
  path: string
  handle: string
  visibility: Visibility
  payload: Record<string, unknown>
  createdAt: string
  updatedAt: string
}

export interface AppManifest {
  appId: string
  name: string
  version: string
  permissions?: string[]
}

export interface TreeReadOptions {
  visibility?: Visibility
  type?: string
  limit?: number
  cursor?: string
}

/** Read-only side of Storage (AC-15 で利用) */
export interface Storage {
  getResourcesByHandle(
    handle: string,
    options?: TreeReadOptions
  ): Promise<Resource[]>
  getResourcesByPath(
    handle: string,
    path: string,
    options?: TreeReadOptions
  ): Promise<Resource[]>
  getResourceById(id: string): Promise<Resource | null>
  getAppManifest(appId: string): Promise<AppManifest | null>
}

/** Write-side extension (AC-16 event consumer が利用) */
export interface MutableStorage extends Storage {
  upsertResource(resource: Resource): Promise<void>
  deleteResource(id: string): Promise<void>
}

/**
 * In-memory stub / reference implementation。
 * event consumer + read API 両方の backing store として動く。
 * AC-16b で SurrealDB-backed 実装に差し替え予定。
 */
export class InMemoryStorage implements MutableStorage {
  private resources = new Map<string, Resource>()
  private manifests = new Map<string, AppManifest>()

  async getResourcesByHandle(
    handle: string,
    options?: TreeReadOptions
  ): Promise<Resource[]> {
    return this.filter(r => r.handle === handle, options)
  }

  async getResourcesByPath(
    handle: string,
    path: string,
    options?: TreeReadOptions
  ): Promise<Resource[]> {
    const normalized = path.startsWith('/') ? path : `/${path}`
    return this.filter(
      r => r.handle === handle && r.path.startsWith(normalized),
      options
    )
  }

  async getResourceById(id: string): Promise<Resource | null> {
    return this.resources.get(id) ?? null
  }

  async getAppManifest(appId: string): Promise<AppManifest | null> {
    return this.manifests.get(appId) ?? null
  }

  async upsertResource(resource: Resource): Promise<void> {
    this.resources.set(resource.id, resource)
  }

  async deleteResource(id: string): Promise<void> {
    this.resources.delete(id)
  }

  /** test convenience — manifest も manual に登録可能 */
  setManifest(manifest: AppManifest): void {
    this.manifests.set(manifest.appId, manifest)
  }

  /** test / debug */
  size(): number {
    return this.resources.size
  }

  private filter(
    predicate: (r: Resource) => boolean,
    options?: TreeReadOptions
  ): Resource[] {
    const all = Array.from(this.resources.values()).filter(predicate)
    const filtered = options?.visibility
      ? all.filter(r => r.visibility === options.visibility)
      : all
    const typed = options?.type
      ? filtered.filter(r => r.type === options.type)
      : filtered
    const limit = options?.limit ?? Number.POSITIVE_INFINITY
    return typed.slice(0, limit)
  }
}
