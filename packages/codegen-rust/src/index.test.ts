import { describe, expect, it } from 'bun:test'
import { parseKdl } from '@kdl-schema/parser'
import { emitRust, escapeRustIdent, pascalCase } from './index.js'

describe('helpers', () => {
  it('pascalCase: kebab → PascalCase', () => {
    expect(pascalCase('memories-atlas')).toBe('MemoriesAtlas')
    expect(pascalCase('vp-world')).toBe('VpWorld')
  })

  it('escapeRustIdent: reserved keyword → r#prefix', () => {
    expect(escapeRustIdent('type')).toBe('r#type')
    expect(escapeRustIdent('mod')).toBe('r#mod')
    expect(escapeRustIdent('normal_name')).toBe('normal_name')
  })
})

describe('emitRust — basic', () => {
  it('emits struct + derive per resource-type', () => {
    const { document } = parseKdl(`
      resource-type "simple" {
        payload {
          field "name" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('#[derive(Debug, Clone, Serialize, Deserialize)]')
    expect(out).toContain('pub struct Simple {')
    expect(out).toContain('pub name: String,')
  })

  it('optional field → Option<T> + skip_serializing_if', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "label" string
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('Option<String>')
    expect(out).toContain('#[serde(skip_serializing_if = "Option::is_none")]')
  })

  it('required field → bare type without Option', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('pub id: String,')
    expect(out).not.toContain('Option<String>')
  })

  it('type mapping: int → i64, bool → bool, datetime → String', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "count" int required=#true
          field "active" bool required=#true
          field "created_at" datetime required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('pub count: i64,')
    expect(out).toContain('pub active: bool,')
    expect(out).toContain('pub created_at: String,')
  })

  it('object → serde_json::Value', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "meta" object
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('Option<serde_json::Value>')
  })

  it('list → Vec<serde_json::Value>', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "items" list {
            element ref target="other"
          }
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('Vec<serde_json::Value>')
  })

  it('description → /// doc comment', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "id" string required=#true description="primary key"
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('/// primary key')
  })

  it('reserved keyword field → r#prefix', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "type" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('pub r#type: String,')
  })

  it('kebab → PascalCase struct name', () => {
    const { document } = parseKdl(`
      resource-type "memories-atlas" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('pub struct MemoriesAtlas {')
  })

  it('emits use serde imports by default', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitRust(document)
    expect(out).toContain('use serde::{Deserialize, Serialize};')
  })

  it('omits serde import when importSerde=false', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { importSerde: false, header: false })
    expect(out).not.toContain('use serde')
  })

  it('custom derives', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string required=#true } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, {
      header: false,
      importSerde: false,
      derives: ['Debug', 'PartialEq', 'Eq'],
    })
    expect(out).toContain('#[derive(Debug, PartialEq, Eq)]')
    // serde import 抜きで custom derives → Serialize 文字は struct attr に出ない
    expect(out).not.toContain('Serialize')
  })
})

describe('emitRust — multiple resource-types', () => {
  it('emits separate struct per resource-type', () => {
    const { document } = parseKdl(`
      resource-type "user" {
        payload {
          field "email" string required=#true
        }
      }
      resource-type "memories-atlas" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitRust(document, { header: false })
    expect(out).toContain('pub struct User {')
    expect(out).toContain('pub struct MemoriesAtlas {')
  })
})
