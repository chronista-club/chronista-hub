/**
 * @kdl-schema/parser — KDL source を typed AST に変換する薄い wrapper。
 *
 * 設計:
 *   - kdljs (v0.3.0) を実 parser として使う
 *   - 返り値を疎結合 KdlDocument 型に normalize
 *   - Error は KdlParseError[] として返す (throw しない)、 codegen tool 側が
 *     diagnostic 表示できるよう構造化
 */
import { parse as kdljsParse } from 'kdljs'
import type {
  KdlDocument,
  KdlNode,
  KdlParseError,
  KdlPrimitive,
} from './types.js'

export type {
  KdlDocument,
  KdlNode,
  KdlParseError,
  KdlPrimitive,
} from './types.js'

export interface ParseResult {
  document: KdlDocument | null
  errors: KdlParseError[]
}

type RawKdlValue = string | number | boolean | bigint | null

interface RawKdlNode {
  name: string
  values?: RawKdlValue[]
  properties?: Record<string, RawKdlValue>
  children?: RawKdlNode[]
  tags?: {
    name?: string | null
    values?: (string | null)[]
    properties?: Record<string, string | null>
  }
}

function normalizePrimitive(v: RawKdlValue): KdlPrimitive {
  if (
    v === null ||
    typeof v === 'string' ||
    typeof v === 'number' ||
    typeof v === 'boolean' ||
    typeof v === 'bigint'
  ) {
    return v
  }
  return String(v)
}

function normalizeNode(raw: RawKdlNode): KdlNode {
  return {
    name: raw.name,
    values: (raw.values ?? []).map(normalizePrimitive),
    properties: Object.fromEntries(
      Object.entries(raw.properties ?? {}).map(([k, v]) => [
        k,
        normalizePrimitive(v),
      ])
    ),
    children: (raw.children ?? []).map(normalizeNode),
    tags: raw.tags,
  }
}

/**
 * KDL source を parse する。 error 発生時は `document: null` + `errors: [...]`。
 */
export function parseKdl(source: string): ParseResult {
  const result = kdljsParse(source) as {
    output?: RawKdlNode[]
    errors?: Array<{
      message?: string
      recoveryToken?: { startLine?: number; startColumn?: number }
      token?: { startLine?: number; startColumn?: number }
    }>
  }

  const errors: KdlParseError[] = (result.errors ?? []).map(e => ({
    message: e.message ?? 'parse error',
    line: e.token?.startLine ?? e.recoveryToken?.startLine,
    column: e.token?.startColumn ?? e.recoveryToken?.startColumn,
  }))

  if (!result.output) {
    return { document: null, errors }
  }

  return {
    document: { nodes: result.output.map(normalizeNode) },
    errors,
  }
}

/**
 * `document.nodes` から指定 name の top-level node を全て取得。
 * codegen tool が頻繁に使う path (`meta`, `path-schema`, `resource-type` 等)。
 */
export function findNodesByName(doc: KdlDocument, name: string): KdlNode[] {
  return doc.nodes.filter(n => n.name === name)
}

/** 子 node からも含めて指定 name を再帰的に収集 */
export function findAllByName(doc: KdlDocument, name: string): KdlNode[] {
  const result: KdlNode[] = []
  function walk(nodes: KdlNode[]) {
    for (const n of nodes) {
      if (n.name === name) result.push(n)
      if (n.children.length > 0) walk(n.children)
    }
  }
  walk(doc.nodes)
  return result
}
