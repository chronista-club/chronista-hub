import { describe, expect, it } from 'bun:test'
import { parseKdl } from '@kdl-schema/parser'
import { emitZod, pascalCase } from './index.js'

describe('emitZod — basic', () => {
  it('emits z.object per resource-type', () => {
    const { document } = parseKdl(`
      resource-type "simple" {
        payload {
          field "name" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('export const SimpleSchema = z.object({')
    expect(out).toContain('name: z.string()')
  })

  it('optional field → .optional()', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "label" string
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('label: z.string().optional()')
  })

  it('required field → no .optional()', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "label" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('label: z.string()')
    expect(out).not.toContain('label: z.string().optional()')
  })

  it('type mapping: int → z.number().int(), bool → z.boolean(), datetime → z.string().datetime()', () => {
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
    const out = emitZod(document, { header: false })
    expect(out).toContain('count: z.number().int()')
    expect(out).toContain('active: z.boolean()')
    expect(out).toContain('created_at: z.string().datetime()')
  })

  it('object → z.record(z.string(), z.unknown())', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "meta" object
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('meta: z.record(z.string(), z.unknown()).optional()')
  })

  it('list → z.array(z.unknown())', () => {
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
    const out = emitZod(document, { header: false })
    expect(out).toContain('items: z.array(z.unknown()).optional()')
  })

  it('description → .describe("...")', () => {
    const { document } = parseKdl(`
      resource-type "t" {
        payload {
          field "id" string required=#true description="unique identifier"
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('.describe("unique identifier")')
  })

  it('kebab-case → PascalCase schema + inferred type', () => {
    const { document } = parseKdl(`
      resource-type "memories-atlas" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('export const MemoriesAtlasSchema')
    expect(out).toContain(
      'export type MemoriesAtlas = z.infer<typeof MemoriesAtlasSchema>'
    )
  })

  it('emits import { z } by default', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitZod(document)
    expect(out).toContain("import { z } from 'zod'")
  })

  it('omits import when importZod=false', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { importZod: false, header: false })
    expect(out).not.toContain('import { z }')
  })

  it('omits inferred type when emitInferred=false', () => {
    const { document } = parseKdl(
      'resource-type "x" { payload { field "a" string } }'
    )
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { emitInferred: false, header: false })
    expect(out).toContain('export const XSchema')
    expect(out).not.toContain('z.infer')
  })
})

describe('emitZod — multiple resource-types', () => {
  it('emits separate schema per resource-type', () => {
    const { document } = parseKdl(`
      resource-type "user" {
        payload {
          field "email" string required=#true
        }
      }
      resource-type "vp-world" {
        payload {
          field "id" string required=#true
        }
      }
    `)
    if (!document) throw new Error('parse failed')
    const out = emitZod(document, { header: false })
    expect(out).toContain('UserSchema = z.object({')
    expect(out).toContain('VpWorldSchema = z.object({')
  })
})

describe('pascalCase re-export', () => {
  it('works', () => {
    expect(pascalCase('memories-atlas')).toBe('MemoriesAtlas')
  })
})
