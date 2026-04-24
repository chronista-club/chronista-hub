/**
 * Hub Storage interface + in-memory stub。
 *
 * 本 phase (AC-15) では DB 未接続、 空 stub を返す。 AC-16 (event ingestion)
 * で SurrealDB 実装に差し替える前提で、 consumer 側 (tree.ts) は Storage
 * interface にしか依存しない設計。
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

export interface Storage {
  /** handle 直下の resources (+ subtree root 相当) */
  getResourcesByHandle(
    handle: string,
    options?: TreeReadOptions
  ): Promise<Resource[]>
  /** handle + path prefix 下の resources */
  getResourcesByPath(
    handle: string,
    path: string,
    options?: TreeReadOptions
  ): Promise<Resource[]>
  /** 単一 resource */
  getResourceById(id: string): Promise<Resource | null>
  /** app manifest */
  getAppManifest(appId: string): Promise<AppManifest | null>
}

/**
 * In-memory stub — 空 result を返すだけ。
 * AC-16 で SurrealDB-backed 実装に差し替える。
 */
export class InMemoryStorage implements Storage {
  async getResourcesByHandle(_handle: string): Promise<Resource[]> {
    return []
  }

  async getResourcesByPath(
    _handle: string,
    _path: string
  ): Promise<Resource[]> {
    return []
  }

  async getResourceById(_id: string): Promise<Resource | null> {
    return null
  }

  async getAppManifest(_appId: string): Promise<AppManifest | null> {
    return null
  }
}
