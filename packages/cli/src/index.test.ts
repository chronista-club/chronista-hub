import { describe, expect, it } from 'bun:test'
import { type CliAdapter, parseArgs, run } from './index.js'

// =============================================================================
// parseArgs
// =============================================================================

describe('parseArgs', () => {
  it('returns help on empty argv', () => {
    expect(parseArgs([])).toEqual({ command: 'help' })
  })

  it('returns help on --help', () => {
    expect(parseArgs(['--help'])).toEqual({ command: 'help' })
    expect(parseArgs(['-h'])).toEqual({ command: 'help' })
  })

  it('rejects unknown command', () => {
    const result = parseArgs(['banana'])
    expect(result.command).toBe('unknown')
  })

  it('parses gen with input + target', () => {
    const result = parseArgs(['gen', 'spec.kdl', '--target', 'ts'])
    expect(result.command).toBe('gen')
    if (result.command === 'gen') {
      expect(result.input).toBe('spec.kdl')
      expect(result.target).toBe('ts')
      expect(result.output).toBeUndefined()
    }
  })

  it('accepts -t shorthand', () => {
    const result = parseArgs(['gen', 'spec.kdl', '-t', 'zod'])
    if (result.command === 'gen') {
      expect(result.target).toBe('zod')
    }
  })

  it('accepts --output / -o', () => {
    const r1 = parseArgs(['gen', 'spec.kdl', '-t', 'ts', '--output', 'out.ts'])
    if (r1.command === 'gen') {
      expect(r1.output).toBe('out.ts')
    }
    const r2 = parseArgs(['gen', 'spec.kdl', '-t', 'ts', '-o', 'out.ts'])
    if (r2.command === 'gen') {
      expect(r2.output).toBe('out.ts')
    }
  })

  it('accepts --no-header', () => {
    const result = parseArgs(['gen', 'spec.kdl', '-t', 'ts', '--no-header'])
    if (result.command === 'gen') {
      expect(result.noHeader).toBe(true)
    }
  })

  it('rejects invalid target', () => {
    const result = parseArgs(['gen', 'spec.kdl', '-t', 'foo'])
    expect(result.command).toBe('unknown')
    if (result.command === 'unknown') {
      expect(result.message).toContain('invalid target')
    }
  })

  it('rejects gen without input', () => {
    const result = parseArgs(['gen', '-t', 'ts'])
    expect(result.command).toBe('unknown')
  })

  it('rejects gen without --target', () => {
    const result = parseArgs(['gen', 'spec.kdl'])
    expect(result.command).toBe('unknown')
  })

  it('rejects --target all without --output-dir', () => {
    const result = parseArgs(['gen', 'spec.kdl', '-t', 'all'])
    expect(result.command).toBe('unknown')
    if (result.command === 'unknown') {
      expect(result.message).toContain('--output-dir')
    }
  })

  it('accepts --target all + --output-dir', () => {
    const result = parseArgs([
      'gen',
      'spec.kdl',
      '-t',
      'all',
      '--output-dir',
      './out',
    ])
    if (result.command === 'gen') {
      expect(result.target).toBe('all')
      expect(result.outputDir).toBe('./out')
    }
  })

  it('rejects multiple inputs', () => {
    const result = parseArgs(['gen', 'a.kdl', 'b.kdl', '-t', 'ts'])
    expect(result.command).toBe('unknown')
  })

  it('rejects --output + --output-dir together', () => {
    const result = parseArgs([
      'gen',
      'spec.kdl',
      '-t',
      'ts',
      '-o',
      'a',
      '--output-dir',
      'b',
    ])
    expect(result.command).toBe('unknown')
  })
})

// =============================================================================
// run — using stub CliAdapter
// =============================================================================

interface CapturedIO {
  reads: Map<string, string>
  writes: Map<string, string>
  logs: string[]
  errors: string[]
}

function makeStubAdapter(reads: Record<string, string> = {}): {
  adapter: CliAdapter
  io: CapturedIO
} {
  const io: CapturedIO = {
    reads: new Map(Object.entries(reads)),
    writes: new Map(),
    logs: [],
    errors: [],
  }
  const adapter: CliAdapter = {
    async readFile(path) {
      const content = io.reads.get(path)
      if (content === undefined) throw new Error(`file not found: ${path}`)
      return content
    },
    async writeFile(path, content) {
      io.writes.set(path, content)
    },
    log(msg) {
      io.logs.push(msg)
    },
    error(msg) {
      io.errors.push(msg)
    },
  }
  return { adapter, io }
}

const SAMPLE_KDL = `
resource-type "user" {
  payload {
    field "email" string required=#true
  }
}
`

describe('run', () => {
  it('prints help and returns 0 on no args', async () => {
    const { adapter, io } = makeStubAdapter()
    const code = await run([], adapter)
    expect(code).toBe(0)
    expect(io.logs.some(l => l.includes('USAGE'))).toBe(true)
  })

  it('returns 2 on unknown command', async () => {
    const { adapter, io } = makeStubAdapter()
    const code = await run(['banana'], adapter)
    expect(code).toBe(2)
    expect(io.errors.length).toBeGreaterThan(0)
  })

  it('emits TS to stdout', async () => {
    const { adapter, io } = makeStubAdapter({ 'spec.kdl': SAMPLE_KDL })
    const code = await run(['gen', 'spec.kdl', '-t', 'ts'], adapter)
    expect(code).toBe(0)
    expect(io.logs[0]).toContain('export interface User')
  })

  it('emits Zod to file', async () => {
    const { adapter, io } = makeStubAdapter({ 'spec.kdl': SAMPLE_KDL })
    const code = await run(
      ['gen', 'spec.kdl', '-t', 'zod', '-o', 'out.ts'],
      adapter
    )
    expect(code).toBe(0)
    expect(io.writes.get('out.ts')).toContain('export const UserSchema')
    expect(io.logs[0]).toContain('zod → out.ts')
  })

  it('emits all 4 targets with --target all', async () => {
    const { adapter, io } = makeStubAdapter({ 'spec.kdl': SAMPLE_KDL })
    const code = await run(
      ['gen', 'spec.kdl', '-t', 'all', '--output-dir', './gen'],
      adapter
    )
    expect(code).toBe(0)
    expect(io.writes.get('./gen/spec.ts')).toContain('export interface User')
    expect(io.writes.get('./gen/spec.schema.ts')).toContain(
      'export const UserSchema'
    )
    expect(io.writes.get('./gen/spec.surql')).toContain(
      'DEFINE TABLE OVERWRITE user'
    )
    expect(io.writes.get('./gen/spec.rs')).toContain('pub struct User')
  })

  it('--no-header skips AUTO-GENERATED header', async () => {
    const { adapter, io } = makeStubAdapter({ 'spec.kdl': SAMPLE_KDL })
    await run(['gen', 'spec.kdl', '-t', 'ts', '--no-header'], adapter)
    expect(io.logs[0]).not.toContain('AUTO-GENERATED')
  })

  it('returns 1 on missing input file', async () => {
    const { adapter, io } = makeStubAdapter()
    const code = await run(['gen', 'missing.kdl', '-t', 'ts'], adapter)
    expect(code).toBe(1)
    expect(io.errors.some(e => e.includes('failed to read'))).toBe(true)
  })

  it('returns 1 on KDL parse failure', async () => {
    const { adapter, io } = makeStubAdapter({
      'bad.kdl': 'this is not valid {{ kdl ',
    })
    const code = await run(['gen', 'bad.kdl', '-t', 'ts'], adapter)
    expect(code).toBe(1)
    expect(io.errors.some(e => e.includes('parse failed'))).toBe(true)
  })
})
