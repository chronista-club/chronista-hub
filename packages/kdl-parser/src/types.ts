/**
 * Typed AST for KDL documents.
 *
 * kdljs の内部 shape を wrap して、 codegen target (TS / Zod / Rust / SurrealQL) が
 * 扱いやすい型で露出する。 kdljs v0.3.0 の node shape:
 *
 *   { name: string, values: Value[], properties: Record<string, Value>, children: Node[], tags: {...} }
 *
 * 本 module は同 shape を export + 名前付き type alias で消費側を安定化させる。
 */

export type KdlPrimitive = string | number | boolean | null | bigint

export interface KdlNode {
  /** Node name (e.g. `DEFINE FIELD` の `DEFINE`, `FIELD`) */
  name: string
  /** 位置引数 (node name 後のスペース区切り値) */
  values: KdlPrimitive[]
  /** Key=value 形式の属性 */
  properties: Record<string, KdlPrimitive>
  /** 子ノード (`{` `}` ブロック内) */
  children: KdlNode[]
  /** kdljs が扱う type tag 情報 (`(typeTag)value` 構文) */
  tags?: {
    name?: string | null
    values?: (string | null)[]
    properties?: Record<string, string | null>
  }
}

export interface KdlDocument {
  nodes: KdlNode[]
}

/** Parser error — location info を保持 */
export interface KdlParseError {
  message: string
  line?: number
  column?: number
}
