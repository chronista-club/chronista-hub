import { describe, expect, it } from 'bun:test'
import { parseKdl } from '@kdl-schema/parser'
import { camelCase, emitTypeScript, pascalCase } from './index.js'

describe('naming helpers', () => {
  it('pascalCase: kebab-case → PascalCase', () => {
    expect(pascalCase('memories-atlas')).toBe('MemoriesAtlas')
    expect(pascalCase('vp-world')).toBe('VpWorld')
    expect(pascalCase('single')).toBe('Single')
  })

  it('pascalCase: snake_case → PascalCase', () => {
    expect(pascalCase('user_profile')).toBe('UserProfile')
    expect(pascalCase('creo_memories')).toBe('CreoMemories')
  })

  it('camelCase: snake_case → camelCase', () => {
    expect(camelCase('display_name')).toBe('displayName')
    expect(camelCase('email')).toBe('email')
  })
})

describe('emitTypeScript — basic', () => {
  it('emits empty string on empty document', () => {
    const { document } = parseKdl('')
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out.trim()).toBe('')
  })

  it('emits interface per resource-type', () => {
    const src = `
      resource-type "simple" {
        payload {
          field "name" string required=#true
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('export interface Simple {')
    expect(out).toContain('name: string')
  })

  it('optional field when required not specified', () => {
    const src = `
      resource-type "opt-demo" {
        payload {
          field "label" string
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('label?: string')
  })

  it('kebab-case type name → PascalCase interface', () => {
    const src = `
      resource-type "memories-atlas" {
        payload {
          field "atlas_id" string required=#true
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('export interface MemoriesAtlas {')
  })

  it('type mapping: int → number, bool → boolean, datetime → string', () => {
    const src = `
      resource-type "types" {
        payload {
          field "count" int required=#true
          field "active" bool required=#true
          field "created_at" datetime required=#true
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('count: number')
    expect(out).toContain('active: boolean')
    expect(out).toContain('created_at: string')
  })

  it('object → Record<string, unknown>', () => {
    const src = `
      resource-type "meta" {
        payload {
          field "extra" object
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('extra?: Record<string, unknown>')
  })

  it('list { element ... } → unknown[]', () => {
    const src = `
      resource-type "with-list" {
        payload {
          field "items" list {
            element ref target="other"
          }
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('items?: unknown[]')
  })

  it('includes description as JSDoc comment', () => {
    const src = `
      resource-type "doc-demo" {
        payload {
          field "id" string required=#true description="unique identifier"
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('/** unique identifier */')
  })

  it('emits header by default', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document)
    expect(out).toContain('AUTO-GENERATED')
    expect(out).toContain('do not edit by hand')
  })
})

describe('emitTypeScript — multiple resource-types', () => {
  it('emits separate interface per resource-type', () => {
    const src = `
      resource-type "handle" {
        payload {
          field "display_name" string
        }
      }
      resource-type "memories-atlas" {
        payload {
          field "atlas_id" string required=#true
        }
      }
    `
    const { document } = parseKdl(src)
    if (!document) throw new Error('parse failed')
    const out = emitTypeScript(document, { header: false })
    expect(out).toContain('export interface Handle {')
    expect(out).toContain('export interface MemoriesAtlas {')
  })
})
