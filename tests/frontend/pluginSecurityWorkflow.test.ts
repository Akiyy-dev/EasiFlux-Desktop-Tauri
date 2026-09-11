import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, it } from 'vitest'

it('declares all three OS runners and all import security suites', () => {
  const workflow = readFileSync(resolve(process.cwd(), '.github/workflows/ci.yml'), 'utf8')
  const match = workflow.match(
    /(?:^|\r?\n)[ ]{2}plugin-security:\r?\n[\s\S]*?(?=\r?\n[ ]{2}[a-zA-Z0-9_-]+:\r?\n|$)/,
  )
  if (!match) throw new Error('plugin-security job required')
  const job = match[0]
  expect(job).toContain('os: [ubuntu-latest, windows-latest, macos-latest]')
  expect(job).toContain('runs-on: ${{ matrix.os }}')
  for (const suite of [
    'plugin::discovery::safe_fs::tests',
    'plugin::import',
    'storage::local_plugin_import',
    'storage::plugin_state::tests',
    'plugin::runtime::import::tests',
  ]) {
    expect(job).toContain(
      `cargo test --locked --manifest-path src-tauri/Cargo.toml ${suite} --lib`,
    )
  }
})
