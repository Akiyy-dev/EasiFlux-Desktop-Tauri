import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { flushPromises, mount } from '@vue/test-utils'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { prepareLocalManifestImport } from '../../src/services/pluginService'
import { usePluginStore } from '../../src/stores/plugin'
import PluginMarketplacePage from '../../src/components/plugins/PluginMarketplacePage.vue'
import type { PluginCatalogItem, PluginCatalogSnapshot } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const INVALID_RESPONSE_ERROR = '插件服务返回的数据无效，请重试。'

function manifest(id = 'com.example.notes', version = '1.1.0') {
  return {
    schemaVersion: 1,
    id,
    publisherId: 'com.example',
    publisher: 'Example',
    name: 'Notes',
    description: 'Metadata only',
    version,
    contributions: [],
    requestedCapabilities: [],
  }
}

function currentItem(id = 'com.example.notes') {
  return {
    manifest: manifest(id, '1.0.0'),
    source: 'localDeclarative',
    management: 'managed',
    canRemove: true,
    toggleBlockReasonCode: null,
    status: 'disabled',
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

function ready(assessment: unknown) {
  return {
    schemaVersion: 2,
    status: 'ready',
    token: 'a'.repeat(32),
    expiresInSeconds: 300,
    catalogGeneration: '1',
    manifest: manifest(),
    assessment,
  }
}

async function reject(value: unknown): Promise<void> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  await expect(prepareLocalManifestImport()).rejects.toThrow(INVALID_RESPONSE_ERROR)
}

describe('local manifest import assessment transport', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
  })

  it('accepts the exact schema-2 notInCatalog preview', async () => {
    const value = ready({ kind: 'notInCatalog' })
    vi.mocked(tauriInvoke).mockResolvedValueOnce(value)

    await expect(prepareLocalManifestImport()).resolves.toEqual(value)
  })

  it.each(['incomingLower', 'samePrecedence', 'incomingHigher'] as const)(
    'accepts existingId with a matching parsed current item and %s',
    async (versionRelation) => {
      const value = ready({ kind: 'existingId', current: currentItem(), versionRelation })
      vi.mocked(tauriInvoke).mockResolvedValueOnce(value)

      await expect(prepareLocalManifestImport()).resolves.toEqual(value)
    },
  )

  it.each([
    ['schema 1 ready', { ...ready({ kind: 'notInCatalog' }), schemaVersion: 1 }],
    ['missing assessment', (() => {
      const value = ready({ kind: 'notInCatalog' }) as Record<string, unknown>
      delete value.assessment
      return value
    })()],
    ['array assessment', ready([])],
    ['unknown kind', ready({ kind: 'unknown' })],
    ['extra notInCatalog key', ready({ kind: 'notInCatalog', current: currentItem() })],
    ['missing existing current', ready({ kind: 'existingId', versionRelation: 'incomingHigher' })],
    ['missing existing relation', ready({ kind: 'existingId', current: currentItem() })],
    ['unknown existing relation', ready({
      kind: 'existingId', current: currentItem(), versionRelation: 'equal',
    })],
    ['mismatched current id', ready({
      kind: 'existingId', current: currentItem('com.example.other'),
      versionRelation: 'incomingHigher',
    })],
    ['extra existing key', ready({
      kind: 'existingId', current: currentItem(), versionRelation: 'incomingHigher', extra: true,
    })],
  ])('rejects %s', async (_label, value) => {
    await reject(value)
  })
})

function snapshot(
  plugins: PluginCatalogItem[],
  catalogGeneration = '1',
  revision = '1',
): PluginCatalogSnapshot {
  return {
    schemaVersion: 3,
    revision,
    catalogGeneration,
    availability: 'available',
    availabilityReasonCode: null,
    localDiscovery: { status: 'available', rejectedPackageCount: 0 },
    managedOwnership: {
      status: 'available', conflictingEntryCount: 0, rollbackPendingCount: 0,
      cleanupPendingCount: 0,
    },
    plugins,
  }
}

function importedItem(sourceManifest: ReturnType<typeof manifest>): PluginCatalogItem {
  return {
    manifest: sourceManifest,
    source: 'localDeclarative',
    management: 'managed',
    canRemove: true,
    toggleBlockReasonCode: null,
    status: 'disabled',
    statusReasonCode: null,
    canToggle: true,
    grantedCapabilities: [],
  }
}

function nativeCallsFor(command: string): unknown[][] {
  return vi.mocked(tauriInvoke).mock.calls.filter(([name]) => name === command)
}

describe('existing-ID assessment integration', () => {
  let showModalDescriptor: PropertyDescriptor | undefined
  let closeDescriptor: PropertyDescriptor | undefined

  beforeEach(() => {
    showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal')
    closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close')
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
      configurable: true,
      value(this: HTMLDialogElement) { this.setAttribute('open', '') },
    })
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
      configurable: true,
      value(this: HTMLDialogElement) { this.removeAttribute('open') },
    })
  })

  afterEach(() => {
    if (showModalDescriptor) {
      Object.defineProperty(HTMLDialogElement.prototype, 'showModal', showModalDescriptor)
    } else delete HTMLDialogElement.prototype.showModal
    if (closeDescriptor) {
      Object.defineProperty(HTMLDialogElement.prototype, 'close', closeDescriptor)
    } else delete HTMLDialogElement.prototype.close
  })

  it('blocks click and direct commit, releases comparison, preserves new import, and rejects stale', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const current = currentItem() as PluginCatalogItem
    const existing = ready({
      kind: 'existingId', current, versionRelation: 'incomingHigher',
    })
    const newManifest = manifest('com.example.new')
    const newReady = {
      ...ready({ kind: 'notInCatalog' }),
      manifest: newManifest,
    }
    const staleManifest = manifest('com.example.stale')
    const staleReady = {
      ...ready({ kind: 'notInCatalog' }),
      catalogGeneration: '2',
      manifest: staleManifest,
    }
    const prepareResults = [existing, newReady, staleReady]
    const importedCatalog = snapshot([importedItem(newManifest), current], '2', '2')
    vi.mocked(tauriInvoke).mockImplementation(async (command) => {
      if (command === 'get_plugin_catalog') return snapshot([current])
      if (command === 'prepare_local_manifest_import') return prepareResults.shift()
      if (command === 'cancel_local_manifest_import') {
        return { schemaVersion: 1, status: 'cancelled' }
      }
      if (command === 'commit_local_manifest_import') {
        return {
          schemaVersion: 2,
          status: 'imported',
          pluginId: newManifest.id,
          snapshot: importedCatalog,
        }
      }
      if (command === 'reload_plugin_catalog') {
        return snapshot(importedCatalog.plugins, '3', '3')
      }
      throw new Error(`unexpected command: ${command}`)
    })
    const wrapper = mount(PluginMarketplacePage, {
      props: { section: 'installed' },
      global: { plugins: [pinia] },
    })
    await flushPromises()
    const store = usePluginStore()

    await wrapper.get('[data-testid="plugin-import-button"]').trigger('click')
    await flushPromises()
    const catalogBeforeComparison = JSON.parse(JSON.stringify(store.catalog))
    const comparisonConfirm = wrapper.get<HTMLButtonElement>('[data-testid="plugin-import-confirm"]')
    expect(comparisonConfirm.element.disabled).toBe(true)
    comparisonConfirm.element.disabled = false
    await comparisonConfirm.trigger('click')
    await store.commitImport()
    expect(nativeCallsFor('commit_local_manifest_import')).toHaveLength(0)
    expect(store.catalog).toEqual(catalogBeforeComparison)

    await wrapper.get('[data-testid="plugin-import-cancel"]').trigger('click')
    await flushPromises()
    expect(nativeCallsFor('cancel_local_manifest_import')).toHaveLength(1)
    expect(store.catalog).toEqual(catalogBeforeComparison)

    await wrapper.get('[data-testid="plugin-import-button"]').trigger('click')
    await flushPromises()
    await wrapper.get('[data-testid="plugin-import-confirm"]').trigger('click')
    await flushPromises()
    expect(nativeCallsFor('commit_local_manifest_import')).toHaveLength(1)
    expect(store.catalog.map((plugin) => plugin.manifest.id))
      .toEqual(['com.example.new', 'com.example.notes'])
    await wrapper.get('[data-testid="plugin-import-dismiss"]').trigger('click')
    await flushPromises()
    expect(store.importStatus).toBe('idle')

    await wrapper.get('[data-testid="plugin-import-button"]').trigger('click')
    await flushPromises()
    expect(store.importPreview?.catalogGeneration).toBe('2')
    expect(store.catalogGeneration).toBe('2')
    await store.reload()
    expect(store.catalogGeneration).toBe('3')
    expect(store.importPreviewStale).toBe(true)
    expect(wrapper.get<HTMLButtonElement>('[data-testid="plugin-import-confirm"]').element.disabled)
      .toBe(true)
    await store.commitImport()
    expect(nativeCallsFor('commit_local_manifest_import')).toHaveLength(1)
    await store.cancelImport()
    expect(nativeCallsFor('cancel_local_manifest_import')).toHaveLength(2)
    expect(store.catalog.map((plugin) => plugin.manifest.id))
      .toEqual(['com.example.new', 'com.example.notes'])
    wrapper.unmount()
  })
})
