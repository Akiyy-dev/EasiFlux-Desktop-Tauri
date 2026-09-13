import { readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, it } from 'vitest'

const read = (path: string) => readFileSync(resolve(process.cwd(), path), 'utf8')

// Exclude cfg(test) items/blocks, including inline native fixture modules.
// Keep non-test platform branches: every supported primitive is part of this guard.
function productionRust(path: string): string {
  let source = read(path)
  const marker = /#\[cfg\((?:test|all\(test,\s*unix\))\)\]/
  for (let match = marker.exec(source); match; match = marker.exec(source)) {
    const start = match.index
    let index = start + match[0].length
    let depth = 0
    let opened = false
    let quoted = false
    const field = /^\s*(?:pub(?:\([^)]*\))?\s+)?\w+\s*:(?!:)/.test(source.slice(index))
    let typeDepth = 0
    for (; index < source.length; index++) {
      const char = source[index]
      if (quoted) {
        if (char === '\\') index++
        else if (char === '"') quoted = false
        continue
      }
      if (source.slice(index, index + 2) === '//') {
        const end = source.indexOf('\n', index)
        index = end === -1 ? source.length : end
        continue
      }
      if (char === '"') quoted = true
      else if (field && (char === '<' || char === '(' || char === '[')) typeDepth++
      else if (field && (char === '>' || char === ')' || char === ']')) typeDepth--
      else if (field && char === ',' && typeDepth === 0) { index++; break }
      else if (char === '{') { depth++; opened = true }
      else if (char === '}' && --depth === 0 && opened) { index++; break }
      else if (char === ';' && !opened) { index++; break }
    }
    source = source.slice(0, start) + source.slice(index)
  }
  return source.replace(/\/\/[^\n]*/g, '').replace(/\/\*[\s\S]*?\*\//g, '')
}

const roots = ['src-tauri/src/plugin', 'src-tauri/src/storage/local_plugin_package', 'src-tauri/src/storage/safe_plugin_document']
function rustFiles(root: string): string[] {
  return readdirSync(resolve(process.cwd(), root), { withFileTypes: true }).flatMap((entry) => {
    if (entry.name === 'tests' || entry.name === 'tests.rs' || entry.name === 'test_support.rs') return []
    const path = `${root}/${entry.name}`
    return entry.isDirectory() ? rustFiles(path) : entry.name.endsWith('.rs') ? [path] : []
  })
}
const lifecycleFiles = [
  ...roots.flatMap(rustFiles),
  ...['local_plugin_import', 'local_plugin_package', 'safe_plugin_document', 'managed_plugin_ownership', 'plugin_state'].map((name) => `src-tauri/src/storage/${name}.rs`),
]

it('removal sends only the fixed id and generation payload', () => {
  const service = read('src/services/pluginService.ts')
  const payload = service.match(/tauriInvoke<unknown>\('remove_managed_local_plugin',\s*\{([^}]*)\}\)/)?.[1]
  expect(payload).toBeDefined()
  expect(payload?.split(',').map((key) => key.trim()).filter(Boolean)).toEqual(['id', 'expectedCatalogGeneration'])
})

it('production lifecycle code has no destructive fallback or execution/download expansion', () => {
  for (const path of lifecycleFiles) {
    const source = productionRust(path)
    expect(source, path).not.toMatch(/\b(?:remove_dir_all|MoveFileExW|CopyFileW|copy_nonoverlapping_file|ShellExecuteW|CreateProcessW|LoadLibraryW|dlopen)\b/)
    expect(source, path).not.toMatch(/\b(?:std|tokio)::(?:fs::(?:remove_file|remove_dir|rename|copy)|process)|\b(?:Command::new|reqwest|ureq|libloading|download|invoke_handler|generate_handler)\s*(?:::|!|\()/)
    expect(source, path).not.toMatch(/\b(?:use\s+(?:std|tokio)::process|fs::(?:remove_file|remove_dir|rename|copy)\s*\()/)
    expect(source, path).not.toMatch(/(?:dispatch|execute|invoke)_?(?:dynamic|command)\s*\(/i)
  }
})

it('package renames are exclusive; replacement authority belongs only to secure documents', () => {
  const packageSource = productionRust('src-tauri/src/storage/local_plugin_package/platform.rs')
  expect(packageSource).toContain('fs::RenameFlags::NOREPLACE')
  expect(packageSource).toContain('libc::RENAME_EXCL')
  expect(packageSource).toContain('(*info).Anonymous.ReplaceIfExists = false;')
  expect(packageSource).not.toMatch(/ReplaceIfExists\s*=\s*true|FILE_RENAME_REPLACE_IF_EXISTS|fs::renameat\(/)
  expect(packageSource.match(/native::promote_exclusive\(/g)?.length).toBe(2)
  for (const path of lifecycleFiles.filter((path) => !path.includes('/safe_plugin_document/'))) {
    expect(productionRust(path), path).not.toMatch(/FILE_RENAME_REPLACE_IF_EXISTS|ReplaceIfExists\s*=\s*true/)
  }
  const documentSource = productionRust('src-tauri/src/storage/safe_plugin_document/platform.rs')
  expect(documentSource).toContain('FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS')
  expect(documentSource).toContain('FileRenameInformationEx')
})

it('capability union adds no filesystem, dialog, shell, remote, or new opener authority', () => {
  for (const name of readdirSync(resolve(process.cwd(), 'src-tauri/capabilities')).filter((name) => name.endsWith('.json'))) {
    const capability = JSON.parse(read(`src-tauri/capabilities/${name}`))
    expect(capability.remote, name).toBeUndefined()
    expect(capability.local, name).not.toBe(false)
    for (const permission of capability.permissions) {
      expect(typeof permission, name).toBe('string') // no scoped object grants
      expect(permission, name).not.toMatch(/^(?:fs|dialog|shell):/)
      if (permission.startsWith('opener:')) {
        expect(name).toBe('default.json')
        expect(permission).toBe('opener:default') // unchanged application baseline
      }
    }
    if (name === 'plugin-runtime.json') {
      expect(capability.permissions).toEqual([
        'allow-get-plugin-catalog', 'allow-reload-plugin-catalog', 'allow-set-plugin-enabled',
        'allow-prepare-local-manifest-import', 'allow-cancel-local-manifest-import',
        'allow-commit-local-manifest-import', 'allow-remove-managed-local-plugin',
      ])
      expect(capability.webviews).toEqual(['main'])
    }
  }
})
