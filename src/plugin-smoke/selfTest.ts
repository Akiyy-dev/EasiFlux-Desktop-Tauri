import { nextTick } from 'vue'
import { usePluginStore } from '../stores/plugin'
import { tauriInvoke } from '../composables/useTauriCommand'
import { pluginErrorCode } from '../services/pluginService'

const fixtureId = 'com.easiflux.examples.series-sma'
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

function textControl(selector: string): HTMLInputElement | HTMLTextAreaElement | null {
  const element = document.querySelector(selector)
  return (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)
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

async function expectPluginRejection(
  label: string,
  invocation: Promise<unknown>,
  expectedCode: string,
): Promise<void> {
  let caught: unknown
  let rejected = false
  try {
    await invocation
  } catch (error) {
    rejected = true
    caught = error
  }
  if (!rejected) throw new Error(`${label} unexpectedly succeeded`)
  const code = pluginErrorCode(caught)
  if (code === expectedCode) return
  const error = new Error(`${label} returned unexpected safe code: ${code ?? 'none'}`)
  Object.defineProperty(error, 'cause', { value: caught })
  throw error
}

async function confirmForbiddenWebviewIpc(): Promise<void> {
  try {
    await tauriInvoke('list_account_profiles')
  } catch {
    return
  }
  throw new Error('Forbidden account IPC unexpectedly succeeded')
}

function computeRequest(store: ReturnType<typeof usePluginStore>, requestId: string) {
  return {
    requestId,
    pluginId: fixtureId,
    contributionId: 'series.sma',
    expectedCatalogGeneration: store.catalogGeneration,
    expectedRevision: store.revision,
    values: [1, 2, 3, 4, 5],
    parameter: 3,
  }
}

export function isExpectedSmaOutput(result: string, provenance: string): boolean {
  return result.trim() === '结果：4'
    && provenance.includes(fixtureId)
    && provenance.includes('series.sma')
    && provenance.includes('输入 5 个')
    && provenance.includes('Period 3')
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
  let detail: string
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

    await expectPluginRejection(
      'disabled compute before enable',
      tauriInvoke('execute_plugin_compute', {
        request: computeRequest(store, 'smoke-disabled-before'),
      }),
      'plugin_compute_disabled',
    )

    await click('input[role="switch"]', requireCard)
    await waitFor('fixture enabled', () => {
      const card = fixtureCard()
      const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
      return fixture()?.status === 'enabled'
        && !store.pendingIds.has(fixtureId) && !store.actionErrors[fixtureId]
        && toggle?.checked === true && !toggle.disabled
        && !!card?.querySelector(testId('plugin-status'))?.textContent?.includes('已启用')
    })

    await click(testId('plugin-command-button'), requireCard)
    await waitFor('compute form opens without execution', () => {
      return !!document.querySelector(testId('plugin-compute-dialog'))
        && textControl(testId('plugin-compute-input')) !== null
    })
    const input = textControl(testId('plugin-compute-input'))!
    input.value = '1,2,3,4,5'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await click(testId('plugin-compute-run'))
    await waitFor('guest SMA returns exact result and provenance', () => {
      const result = document.querySelector(testId('plugin-compute-result'))?.textContent
      const provenance = document.querySelector(testId('plugin-compute-provenance'))?.textContent
      return result !== undefined && result !== null
        && provenance !== undefined && provenance !== null
        && isExpectedSmaOutput(result, provenance)
    })
    await click(testId('plugin-compute-close'))
    await waitFor('completed compute form closes', () => !document.querySelector(testId('plugin-compute-dialog')))

    await click('input[role="switch"]', requireCard)
    await waitFor('fixture disabled', () => {
      const card = fixtureCard()
      const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
      return fixture()?.status === 'disabled'
        && !store.pendingIds.has(fixtureId) && !store.actionErrors[fixtureId]
        && toggle?.checked === false && !toggle.disabled
        && !!card?.querySelector(testId('plugin-status'))?.textContent?.includes('已停用')
    })
    await expectPluginRejection(
      'disabled compute after disable',
      tauriInvoke('execute_plugin_compute', {
        request: computeRequest(store, 'smoke-disabled-after'),
      }),
      'plugin_compute_disabled',
    )
    await confirmForbiddenWebviewIpc()

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
    detail = 'Real DOM: preview cancel, managed v4 import, disabled rejections, enable, guest SMA 4, disable, forbidden IPC, removal and reload verified'
  } catch (error) {
    detail = detailWithinLimit(error)
  }
  // Exactly one submission even if the transport fails or the host exits before its reply.
  try { await tauriInvoke('finish_plugin_smoke', { success, detail }) }
  catch (error) { console.error('Smoke completion transport failed; host deadline remains authoritative', error) }
}
