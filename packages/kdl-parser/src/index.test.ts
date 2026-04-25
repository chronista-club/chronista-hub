import { describe, expect, it } from 'bun:test'
import { findAllByName, findNodesByName, parseKdl } from './index.js'

describe('parseKdl', () => {
  it('parses a simple flat KDL document', () => {
    const src = `
      title "hello"
      version "1.0"
    `
    const { document, errors } = parseKdl(src)
    expect(errors).toEqual([])
    expect(document).not.toBeNull()
    expect(document?.nodes.length).toBe(2)
    expect(document?.nodes[0]?.name).toBe('title')
    expect(document?.nodes[0]?.values).toEqual(['hello'])
  })

  it('parses nested children (block)', () => {
    const src = `
      meta {
        version "0.1"
        domain "example.com"
      }
    `
    const { document, errors } = parseKdl(src)
    expect(errors).toEqual([])
    const metaNode = document?.nodes[0]
    expect(metaNode?.name).toBe('meta')
    expect(metaNode?.children.length).toBe(2)
    expect(metaNode?.children[0]?.name).toBe('version')
    expect(metaNode?.children[0]?.values).toEqual(['0.1'])
  })

  it('parses properties (key=value)', () => {
    // KDL v2 syntax: boolean は `#true` / `#false`
    const src = 'field type="string" required=#true'
    const { document } = parseKdl(src)
    const node = document?.nodes[0]
    expect(node?.name).toBe('field')
    expect(node?.properties.type).toBe('string')
    expect(node?.properties.required).toBe(true)
  })

  it('parses multiple values', () => {
    const src = 'coords 1.5 2.5 3.5'
    const { document } = parseKdl(src)
    expect(document?.nodes[0]?.values).toEqual([1.5, 2.5, 3.5])
  })

  it('returns empty doc for empty source', () => {
    const { document, errors } = parseKdl('')
    expect(errors).toEqual([])
    expect(document?.nodes).toEqual([])
  })

  it('reports parse errors with location', () => {
    const src = 'unclosed { child'
    const { errors } = parseKdl(src)
    expect(errors.length).toBeGreaterThan(0)
    expect(errors[0]?.message).toBeDefined()
  })
})

describe('findNodesByName', () => {
  it('returns top-level nodes matching name', () => {
    const { document } = parseKdl(`
      resource "a"
      resource "b"
      other "c"
    `)
    expect(document).not.toBeNull()
    if (!document) return
    const resources = findNodesByName(document, 'resource')
    expect(resources.length).toBe(2)
    expect(resources[0]?.values).toEqual(['a'])
  })

  it('does NOT recurse into children', () => {
    const { document } = parseKdl(`
      outer {
        inner "nested"
      }
    `)
    if (!document) return
    expect(findNodesByName(document, 'inner').length).toBe(0)
  })
})

describe('findAllByName', () => {
  it('recurses into children', () => {
    const { document } = parseKdl(`
      outer {
        inner "a"
        inner "b"
        deeper {
          inner "c"
        }
      }
    `)
    if (!document) return
    expect(findAllByName(document, 'inner').length).toBe(3)
  })
})

describe('World Tree spec smoke test', () => {
  // Chronista Hub の world-tree.kdl と同 shape の minimum 近似
  // (実 spec 全文 parse は integration test で別途、 本 test は parser が
  //  このスタイルを処理できる確認)
  const worldTreeLike = `
    meta {
      version "0.1"
      domain "chronista.club"
    }

    path-schema {
      root "/@{handle}"
      separator "/"
    }

    resource-type "handle" {
      payload {
        field "display_name" string
      }
    }
  `

  it('parses World Tree-like structure without errors', () => {
    const { document, errors } = parseKdl(worldTreeLike)
    expect(errors).toEqual([])
    expect(document).not.toBeNull()
  })

  it('locates top-level meta / path-schema / resource-type nodes', () => {
    const { document } = parseKdl(worldTreeLike)
    if (!document) return
    expect(findNodesByName(document, 'meta').length).toBe(1)
    expect(findNodesByName(document, 'path-schema').length).toBe(1)
    expect(findNodesByName(document, 'resource-type').length).toBe(1)
  })

  it('extracts resource-type payload fields via findAllByName', () => {
    const { document } = parseKdl(worldTreeLike)
    if (!document) return
    const fields = findAllByName(document, 'field')
    expect(fields.length).toBe(1)
    expect(fields[0]?.values[0]).toBe('display_name')
  })
})
