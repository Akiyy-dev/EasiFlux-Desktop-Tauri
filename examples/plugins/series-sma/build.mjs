import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const directory = path.dirname(fileURLToPath(import.meta.url))
const repository = path.resolve(directory, '../../..')
const mode = process.argv[2] ?? '--check'

if (!['--check', '--write'].includes(mode) || process.argv.length > 3) {
  console.error('usage: node examples/plugins/series-sma/build.mjs [--check | --write]')
  process.exit(2)
}

const result = spawnSync('cargo', [
  'run',
  '--locked',
  '--manifest-path',
  path.join(repository, 'src-tauri/Cargo.toml'),
  '--example',
  'build_series_sma_manifest',
  '--',
  mode,
], { cwd: repository, stdio: 'inherit' })

if (result.error) {
  console.error(`failed to start pinned Rust generator: ${result.error.message}`)
  process.exit(1)
}
process.exit(result.status ?? 1)
