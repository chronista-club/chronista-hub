/**
 * 軽量 flag parser — commander 等は使わず純 ESM。
 * Sub-command 1 つ (`gen`) + 数個の flag に絞った最小実装。
 *
 * Usage:
 *   kdl-schema gen <input.kdl> --target <ts|zod|surql|rust|all> [--output <file>] [--output-dir <dir>] [--no-header]
 */

export type Target = 'ts' | 'zod' | 'surql' | 'rust' | 'all'

export interface GenArgs {
  command: 'gen'
  input: string
  target: Target
  output?: string
  outputDir?: string
  noHeader: boolean
}

export interface HelpArgs {
  command: 'help'
}

export interface UnknownArgs {
  command: 'unknown'
  message: string
}

export type ParsedArgs = GenArgs | HelpArgs | UnknownArgs

const VALID_TARGETS: ReadonlySet<string> = new Set([
  'ts',
  'zod',
  'surql',
  'rust',
  'all',
])

function isValidTarget(s: string): s is Target {
  return VALID_TARGETS.has(s)
}

export function parseArgs(argv: readonly string[]): ParsedArgs {
  if (argv.length === 0 || argv[0] === '--help' || argv[0] === '-h') {
    return { command: 'help' }
  }

  const cmd = argv[0]
  if (cmd !== 'gen') {
    return { command: 'unknown', message: `unknown command: ${cmd}` }
  }

  const rest = argv.slice(1)
  let input: string | undefined
  let target: Target | undefined
  let output: string | undefined
  let outputDir: string | undefined
  let noHeader = false

  for (let i = 0; i < rest.length; i++) {
    const arg = rest[i]
    if (arg === '--target' || arg === '-t') {
      const value = rest[++i]
      if (!value) {
        return { command: 'unknown', message: '--target requires a value' }
      }
      if (!isValidTarget(value)) {
        return {
          command: 'unknown',
          message: `invalid target: ${value} (expected ts|zod|surql|rust|all)`,
        }
      }
      target = value
    } else if (arg === '--output' || arg === '-o') {
      output = rest[++i]
    } else if (arg === '--output-dir') {
      outputDir = rest[++i]
    } else if (arg === '--no-header') {
      noHeader = true
    } else if (arg && !arg.startsWith('-')) {
      if (input) {
        return {
          command: 'unknown',
          message: `multiple input files not supported: ${input} and ${arg}`,
        }
      }
      input = arg
    } else {
      return { command: 'unknown', message: `unknown flag: ${arg}` }
    }
  }

  if (!input) {
    return { command: 'unknown', message: 'gen requires <input.kdl>' }
  }
  if (!target) {
    return { command: 'unknown', message: 'gen requires --target' }
  }
  if (target === 'all' && !outputDir) {
    return {
      command: 'unknown',
      message: '--target all requires --output-dir',
    }
  }
  if (output && outputDir) {
    return {
      command: 'unknown',
      message: '--output and --output-dir are mutually exclusive',
    }
  }

  return { command: 'gen', input, target, output, outputDir, noHeader }
}

export const HELP_TEXT = `
kdl-schema — KDL spec → multi-target codegen

USAGE
  kdl-schema gen <input.kdl> --target <ts|zod|surql|rust|all> [options]

OPTIONS
  --target, -t      Emit target: ts | zod | surql | rust | all
  --output, -o      Write to file (default: stdout). Mutually exclusive with --output-dir.
  --output-dir      Write each target to <dir>/<basename>.<ext>. Required for --target all.
  --no-header       Omit AUTO-GENERATED header
  --help, -h        Show this help

EXAMPLES
  kdl-schema gen world-tree.kdl --target ts > types.ts
  kdl-schema gen world-tree.kdl --target zod -o schemas.ts
  kdl-schema gen world-tree.kdl --target all --output-dir ./generated/
`.trim()
