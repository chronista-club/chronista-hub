/**
 * @kdl-schema/cli — unified CLI front-end for the codegen pipeline。
 *
 * Architecture:
 *   parseArgs (args.ts) → routeGen → 各 codegen-{ts,zod,surql,rust}.emit*
 *
 * Pure run() function (filesystem I/O も interface 経由) で test 容易化。
 * 実行 entry point は bin/kdl-schema.ts でこれを呼び出す。
 */

import { emitRust } from '@kdl-schema/codegen-rust'
import { emitSurql } from '@kdl-schema/codegen-surql'
import { emitTypeScript } from '@kdl-schema/codegen-ts'
import { emitZod } from '@kdl-schema/codegen-zod'
import { parseKdl } from '@kdl-schema/parser'
import { type GenArgs, HELP_TEXT, parseArgs, type Target } from './args.js'

export type { GenArgs, ParsedArgs, Target } from './args.js'
export { HELP_TEXT, parseArgs } from './args.js'

/**
 * Filesystem + console を inject 可能にした runtime adapter。
 * Bun.file / Bun.write / console.log を default、 test では in-memory に差替。
 */
export interface CliAdapter {
  readFile(path: string): Promise<string>
  writeFile(path: string, content: string): Promise<void>
  log(message: string): void
  error(message: string): void
}

export const defaultAdapter: CliAdapter = {
  async readFile(path) {
    return await Bun.file(path).text()
  },
  async writeFile(path, content) {
    await Bun.write(path, content)
  },
  log(msg) {
    console.log(msg)
  },
  error(msg) {
    console.error(msg)
  },
}

const TARGET_EXT: Record<Exclude<Target, 'all'>, string> = {
  ts: 'ts',
  zod: 'ts',
  surql: 'surql',
  rust: 'rs',
}

/** target ごとの emit 結果 — file 出力時の suffix も含む */
function emitForTarget(
  target: Exclude<Target, 'all'>,
  source: string,
  noHeader: boolean
): string {
  const { document, errors } = parseKdl(source)
  if (!document) {
    const summary = errors
      .map(e => `  ${e.message}${e.line ? ` (line ${e.line})` : ''}`)
      .join('\n')
    throw new Error(`KDL parse failed:\n${summary || '  (no error detail)'}`)
  }

  const opts = { header: !noHeader }
  switch (target) {
    case 'ts':
      return emitTypeScript(document, opts)
    case 'zod':
      return emitZod(document, opts)
    case 'surql':
      return emitSurql(document, opts)
    case 'rust':
      return emitRust(document, opts)
  }
}

/**
 * Pure async run — exit code を return、 side effect は adapter 経由。
 */
export async function run(
  argv: readonly string[],
  adapter: CliAdapter = defaultAdapter
): Promise<number> {
  const parsed = parseArgs(argv)

  if (parsed.command === 'help') {
    adapter.log(HELP_TEXT)
    return 0
  }

  if (parsed.command === 'unknown') {
    adapter.error(`error: ${parsed.message}\n`)
    adapter.error(HELP_TEXT)
    return 2
  }

  // command === 'gen'
  return await runGen(parsed, adapter)
}

async function runGen(args: GenArgs, adapter: CliAdapter): Promise<number> {
  let source: string
  try {
    source = await adapter.readFile(args.input)
  } catch (err) {
    adapter.error(
      `failed to read input: ${err instanceof Error ? err.message : err}`
    )
    return 1
  }

  if (args.target === 'all') {
    const dir = args.outputDir as string
    const baseName = basename(args.input).replace(/\.kdl$/, '')
    for (const target of ['ts', 'zod', 'surql', 'rust'] as const) {
      try {
        const content = emitForTarget(target, source, args.noHeader)
        const ext = TARGET_EXT[target]
        const suffix = target === 'zod' ? '.schema' : ''
        const out = `${dir.replace(/\/$/, '')}/${baseName}${suffix}.${ext}`
        await adapter.writeFile(out, content)
        adapter.log(`✓ ${target} → ${out}`)
      } catch (err) {
        adapter.error(
          `failed to emit ${target}: ${err instanceof Error ? err.message : err}`
        )
        return 1
      }
    }
    return 0
  }

  // single target
  let content: string
  try {
    content = emitForTarget(args.target, source, args.noHeader)
  } catch (err) {
    adapter.error(err instanceof Error ? err.message : String(err))
    return 1
  }

  if (args.output) {
    try {
      await adapter.writeFile(args.output, content)
      adapter.log(`✓ ${args.target} → ${args.output}`)
    } catch (err) {
      adapter.error(
        `failed to write: ${err instanceof Error ? err.message : err}`
      )
      return 1
    }
  } else {
    adapter.log(content)
  }
  return 0
}

function basename(path: string): string {
  const idx = path.lastIndexOf('/')
  return idx === -1 ? path : path.slice(idx + 1)
}
