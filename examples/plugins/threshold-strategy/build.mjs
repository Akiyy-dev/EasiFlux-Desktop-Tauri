import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const directory = path.dirname(fileURLToPath(import.meta.url))
const repository = path.resolve(directory, '../../..')
const mode = process.argv[2] ?? '--check'

if (!['--check', '--write'].includes(mode) || process.argv.length > 3) {
  console.error('usage: node examples/plugins/threshold-strategy/build.mjs [--check | --write]')
  process.exit(2)
}

const result = spawnSync('cargo', [
  'run',
  '--quiet',
  '--locked',
  '--manifest-path',
  path.join(repository, 'src-tauri/Cargo.toml'),
  '--target-dir',
  path.join(repository, 'src-tauri/target'),
  '--example',
  'build_threshold_strategy_manifest',
  '--',
  mode,
], { cwd: repository, stdio: 'inherit' })

if (result.error) {
  console.error(`failed to start pinned Rust generator: ${result.error.message}`)
  process.exit(1)
}
process.exit(result.status ?? 1)
