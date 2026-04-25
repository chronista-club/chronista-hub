#!/usr/bin/env bun
/**
 * kdl-schema CLI entry point。 src/index.ts の `run` を呼び出すだけ。
 */
import { run } from '../src/index.js'

const exitCode = await run(process.argv.slice(2))
process.exit(exitCode)
