import { describe, expect, it } from 'bun:test'
import { parseKdl } from '@kdl-schema/parser'
import { emitSurql, toSnakeCase } from './index.js'

describe('toSnakeCase', () => {
  it('preserves snake_case', () => {
    expect(toSnakeCase('atlas_id')).toBe('atlas_id')
  })
  it('converts kebab-case to snake_case', () => {
    expect(toSnakeCase('memories-atlas')).toBe('memories_atlas')
    expect(toSnakeCase('vp-world')).toBe('vp_world')
  })
})

describe('emitSurql — basic', () => {
  it('emits DEFINE TABLE per resource-type', () => {
    const { document } = parseKdl(`
      resource-type "simple" {
        payload {
          field "name" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain(
      'DEFINE TABLE OVERWRITE simple TYPE NORMAL SCHEMAFULL'
    )
    expect(out).toContain('DEFINE FIELD OVERWRITE name ON simple TYPE string')
    expect(out).toContain('ASSERT $value != NONE')
    expect(out).toContain('PERMISSIONS FULL')
  })

  it('snake_case table name (default)', () => {
    const { document } = parseKdl(`
      resource-type "memories-atlas" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain('DEFINE TABLE OVERWRITE memories_atlas')
    expect(out).toContain('ON memories_atlas')
  })

  it('preserves kebab-case when tableNameCase=kebab', () => {
    const { document } = parseKdl(`
      resource-type "memories-atlas" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false, tableNameCase: 'kebab' })
    expect(out).toContain('DEFINE TABLE OVERWRITE memories-atlas')
  })

  it('optional field omits ASSERT', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "label" string
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain('DEFINE FIELD OVERWRITE label ON t TYPE string')
    expect(out).not.toMatch(/label[\s\S]*ASSERT/)
  })

  it('object → FLEXIBLE', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "meta" object
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain('TYPE object')
    expect(out).toContain('FLEXIBLE')
  })

  it('list → array<unknown>', () => {
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
    const out = emitSurql(document, { header: false })
    expect(out).toContain('TYPE array<unknown>')
  })

  it('description → COMMENT escaped', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "id" string required=#true description="primary key, can't be null"
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain("COMMENT 'primary key, can\\'t be null'")
  })

  it('type mapping: int / bool / datetime', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "count" int required=#true
          field "active" bool required=#true
          field "ts" datetime required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain('TYPE int')
    expect(out).toContain('TYPE bool')
    expect(out).toContain('TYPE datetime')
  })

  it('empty payload → table only, no fields', () => {
    const { document } = parseKdl(`
      resource-type "empty" {}
    `)
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document, { header: false })
    expect(out).toContain('DEFINE TABLE OVERWRITE empty')
    expect(out).not.toContain('DEFINE FIELD')
  })
})

describe('emitSurql — multiple resource-types', () => {
  it('emits separate table block per resource-type', () => {
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
    const out = emitSurql(document, { header: false })
    expect(out).toContain('DEFINE TABLE OVERWRITE user')
    expect(out).toContain('DEFINE TABLE OVERWRITE memories_atlas')
    expect(out).toContain('DEFINE FIELD OVERWRITE email ON user')
    expect(out).toContain('DEFINE FIELD OVERWRITE id ON memories_atlas')
  })
})

describe('emitSurql — header', () => {
  it('emits header by default', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitSurql(document)
    expect(out).toContain('AUTO-GENERATED')
    expect(out).toContain('codegen-surql')
  })
})
