// @vitest-environment node

import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { loadConfigFromFile } from 'vite'

describe('Vite file watching', () => {
  it('ignores Cargo artifacts emitted into the repository target directory', async () => {
    const loaded = await loadConfigFromFile(
      { command: 'serve', mode: 'development' },
      path.resolve('vite.config.ts'),
    )
    const ignored = loaded?.config.server?.watch?.ignored
    const patterns = Array.isArray(ignored) ? ignored : [ignored]
    const cargoArtifact = '/workspace/target/debug/build/easiflux/build_script_build.exe'

    expect(patterns.some(pattern =>
      typeof pattern === 'string' && path.posix.matchesGlob(cargoArtifact, pattern),
    )).toBe(true)
  })
})
