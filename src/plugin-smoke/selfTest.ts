import { nextTick } from 'vue'
import { usePluginStore } from '../stores/plugin'
import { tauriInvoke } from '../composables/useTauriCommand'

const fixtureId = 'com.easiflux.smoke'
const testId = (id: string) => `[data-testid="${id}"]`

async function waitFor(label: string, condition: () => boolean): Promise<void> {
  const deadline = Date.now() + 15_000
  while (true) {
    await nextTick()
    if (condition()) return
    if (Date.now() >= deadline) throw new Error(`Timed out: ${label}`)
    await new Promise(resolve => setTimeout(resolve, 50))
  }
}

function control(selector: string, scope: ParentNode = document): HTMLButtonElement | HTMLInputElement | null {
  const element = scope.querySelector(selector)
  return (element instanceof HTMLButtonElement || element instanceof HTMLInputElement)
    && element.isConnected && !element.disabled ? element : null
}

async function click(selector: string, scope: () => ParentNode = () => document): Promise<void> {
  await waitFor(`enabled control ${selector}`, () => control(selector, scope()) !== null)
  control(selector, scope())!.click()
}

function fixtureCard(): HTMLElement | null {
  // Production cards already display the full fixture ID; do not add harness selectors there.
  return [...document.querySelectorAll<HTMLElement>(testId('plugin-management-item'))]
    .find(card => [...card.querySelectorAll('dd')].some(item => item.textContent?.trim() === fixtureId)) ?? null
}

function requireCard(): HTMLElement {
  const card = fixtureCard()
  if (!card) throw new Error('Fixture card disappeared')
  return card
}

function detailWithinLimit(error: unknown): string {
  const detail = error instanceof Error ? error.message : String(error)
  let result = ''
  let bytes = 0
  const encoder = new TextEncoder()
  for (const character of detail) {
    const size = encoder.encode(character).length
    if (bytes + size > 2_000) break
    result += character
    bytes += size
  }
  return result
}

export async function confirmRemovalReload(): Promise<void> {
  const store = usePluginStore()
  // This production reload control has no test ID; use its exact visible label.
  const reload = [...document.querySelectorAll<HTMLButtonElement>('.plugin-marketplace-page__actions button')]
    .find(button => button.textContent?.trim() === '重新扫描本地插件')
  if (!reload || reload.disabled) throw new Error('Explicit reload control unavailable')
  reload.click()
  if (store.reloadStatus !== 'loading') throw new Error('Explicit reload did not start')
  await waitFor('explicit reload confirms absence', () => store.reloadStatus === 'ready'
    && !store.reloadError
    && !store.catalog.some(item => item.manifest.id === fixtureId) && !fixtureCard())
}

// Real rendered controls -> production component/store/service -> real native IPC.
// No source path is accepted or sent by this driver.
export async function runPluginSmokeSelfTest(): Promise<void> {
  const store = usePluginStore()
  let success = false
  let detail = ''
  try {
    const fixture = () => store.catalog.find(item => item.manifest.id === fixtureId)
    await waitFor('initial catalog readiness', () => store.loadStatus === 'ready' && store.availability === 'available')
    if (fixture() || fixtureCard()) throw new Error('Fixture unexpectedly existed before import')

    await click(testId('plugin-import-button'))
    await waitFor('first fixture preview', () => store.importStatus === 'preview' && store.importPreview?.manifest.id === fixtureId)
    await click(testId('plugin-import-cancel'))
    await waitFor('cancelled preview without import', () => store.importStatus === 'idle'
      && !document.querySelector(testId('plugin-import-confirm')) && !fixture() && !fixtureCard())

    await click(testId('plugin-import-button'))
    await waitFor('second fixture preview', () => store.importStatus === 'preview' && store.importPreview?.manifest.id === fixtureId)
    await click(testId('plugin-import-confirm'))
    await waitFor('managed disabled import', () => store.importResult?.status === 'imported'
      && fixture()?.management === 'managed' && fixture()?.status === 'disabled'
      && !!fixtureCard() && !!document.querySelector(testId('plugin-import-status')))
    await click(testId('plugin-import-dismiss'))
    await waitFor('import result dismissed', () => store.importStatus === 'idle')

    for (const enabled of [true, false]) {
      await click('input[role="switch"]', requireCard)
      await waitFor(enabled ? 'fixture enabled' : 'fixture disabled', () => {
        const card = fixtureCard()
        const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
        return fixture()?.status === (enabled ? 'enabled' : 'disabled')
          && !store.pendingIds.has(fixtureId) && !store.actionErrors[fixtureId]
          && toggle?.checked === enabled && !toggle.disabled
          && !!card?.querySelector(testId('plugin-status'))?.textContent?.includes(enabled ? '已启用' : '已停用')
      })
    }

    await click(testId('plugin-remove-button'), requireCard)
    await waitFor('first removal confirmation', () => store.removalStatus === 'confirming' && store.removalTarget?.plugin.manifest.id === fixtureId)
    await click(testId('plugin-removal-cancel'))
    await waitFor('cancelled removal retains fixture', () => store.removalStatus === 'idle'
      && !!fixtureCard() && fixture()?.status === 'disabled' && !document.querySelector(testId('plugin-removal-confirm')))

    await click(testId('plugin-remove-button'), requireCard)
    await waitFor('second removal confirmation', () => store.removalStatus === 'confirming' && store.removalTarget?.plugin.manifest.id === fixtureId)
    await click(testId('plugin-removal-confirm'))
    await waitFor('successful removal', () => store.removalResult?.status === 'removed'
      && !fixture() && !fixtureCard() && document.querySelector(testId('plugin-removal-result'))?.getAttribute('role') === 'status')

    await confirmRemovalReload()
    success = true
    detail = 'Real DOM: preview cancel, managed import, enable/disable, removal cancel, removal confirm, explicit reload verified'
  } catch (error) {
    detail = detailWithinLimit(error)
  }
  // Exactly one submission even if the transport fails or the host exits before its reply.
  try { await tauriInvoke('finish_plugin_smoke', { success, detail }) }
  catch (error) { console.error('Smoke completion transport failed; host deadline remains authoritative', error) }
}
