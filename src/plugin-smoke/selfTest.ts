import { nextTick } from 'vue'
import { usePluginStore } from '../stores/plugin'
import { tauriInvoke } from '../composables/useTauriCommand'
import { pluginErrorCode } from '../services/pluginService'

const legacyFixtureId = 'com.easiflux.examples.series-sma'
const accountFixtureId = 'com.easiflux.examples.account-workflow'
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

function fixtureCard(fixtureId = legacyFixtureId): HTMLElement | null {
  // Production cards already display the full fixture ID; do not add harness selectors there.
  return [...document.querySelectorAll<HTMLElement>(testId('plugin-management-item'))]
    .find(card => [...card.querySelectorAll('dd')].some(item => item.textContent?.trim() === fixtureId)) ?? null
}

function requireCard(fixtureId = legacyFixtureId): HTMLElement {
  const card = fixtureCard(fixtureId)
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

function commandButton(title: string, fixtureId: string): HTMLButtonElement | null {
  return [...requireCard(fixtureId).querySelectorAll<HTMLButtonElement>(testId('plugin-command-button'))]
    .find(button => button.textContent?.includes(title) && !button.disabled) ?? null
}

async function clickCommand(title: string, fixtureId: string): Promise<void> {
  await waitFor(`command ${title}`, () => commandButton(title, fixtureId) !== null)
  commandButton(title, fixtureId)!.click()
}

function setText(selector: string, value: string): void {
  const input = textControl(selector)
  if (!input) throw new Error(`Text control unavailable: ${selector}`)
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
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
    pluginId: legacyFixtureId,
    contributionId: 'series.sma',
    expectedCatalogGeneration: store.catalogGeneration,
    expectedRevision: store.revision,
    values: [1, 2, 3, 4, 5],
    parameter: 3,
  }
}

export function isExpectedSmaOutput(result: string, provenance: string): boolean {
  return result.trim() === '结果：4'
    && provenance.includes(legacyFixtureId)
    && provenance.includes('series.sma')
    && provenance.includes('输入 5 个')
    && provenance.includes('Period 3')
}

export async function confirmRemovalReload(
  removedFixtureIds: string[] = [legacyFixtureId],
): Promise<void> {
  const store = usePluginStore()
  // This production reload control has no test ID; use its exact visible label.
  const reload = [...document.querySelectorAll<HTMLButtonElement>('.plugin-marketplace-page__actions button')]
    .find(button => button.textContent?.trim() === '重新扫描本地插件')
  if (!reload || reload.disabled) throw new Error('Explicit reload control unavailable')
  reload.click()
  if (store.reloadStatus !== 'loading') throw new Error('Explicit reload did not start')
  await waitFor('explicit reload confirms absence', () => store.reloadStatus === 'ready'
    && !store.reloadError
    && removedFixtureIds.every(fixtureId => (
      !store.catalog.some(item => item.manifest.id === fixtureId) && !fixtureCard(fixtureId)
    )))
}

// Real rendered controls -> production component/store/service -> real native IPC.
// No source path is accepted or sent by this driver.
export async function runPluginSmokeSelfTest(): Promise<void> {
  const store = usePluginStore()
  let success = false
  let detail: string
  try {
    const fixture = (fixtureId = legacyFixtureId) => (
      store.catalog.find(item => item.manifest.id === fixtureId)
    )
    await waitFor('initial catalog readiness', () => store.loadStatus === 'ready' && store.availability === 'available')
    if (fixture() || fixtureCard() || fixture(accountFixtureId) || fixtureCard(accountFixtureId)) {
      throw new Error('Fixture unexpectedly existed before import')
    }

    await click(testId('plugin-import-button'))
    await waitFor('first fixture preview', () => store.importStatus === 'preview' && store.importPreview?.manifest.id === legacyFixtureId)
    await click(testId('plugin-import-cancel'))
    await waitFor('cancelled preview without import', () => store.importStatus === 'idle'
      && !document.querySelector(testId('plugin-import-confirm')) && !fixture() && !fixtureCard())

    await click(testId('plugin-import-button'))
    await waitFor('second fixture preview', () => store.importStatus === 'preview' && store.importPreview?.manifest.id === legacyFixtureId)
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

    await click('input[role="switch"]', () => requireCard())
    await waitFor('fixture enabled', () => {
      const card = fixtureCard()
      const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
      return fixture()?.status === 'enabled'
        && !store.pendingIds.has(legacyFixtureId) && !store.actionErrors[legacyFixtureId]
        && toggle?.checked === true && !toggle.disabled
        && !!card?.querySelector(testId('plugin-status'))?.textContent?.includes('已启用')
    })

    await click(testId('plugin-command-button'), () => requireCard())
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

    await click('input[role="switch"]', () => requireCard())
    await waitFor('fixture disabled', () => {
      const card = fixtureCard()
      const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
      return fixture()?.status === 'disabled'
        && !store.pendingIds.has(legacyFixtureId) && !store.actionErrors[legacyFixtureId]
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

    await click(testId('plugin-remove-button'), () => requireCard())
    await waitFor('first removal confirmation', () => store.removalStatus === 'confirming' && store.removalTarget?.plugin.manifest.id === legacyFixtureId)
    await click(testId('plugin-removal-cancel'))
    await waitFor('cancelled removal retains fixture', () => store.removalStatus === 'idle'
      && !!fixtureCard() && fixture()?.status === 'disabled' && !document.querySelector(testId('plugin-removal-confirm')))

    await click(testId('plugin-remove-button'), () => requireCard())
    await waitFor('second removal confirmation', () => store.removalStatus === 'confirming' && store.removalTarget?.plugin.manifest.id === legacyFixtureId)
    await click(testId('plugin-removal-confirm'))
    await waitFor('successful removal', () => store.removalResult?.status === 'removed'
      && !fixture() && !fixtureCard() && document.querySelector(testId('plugin-removal-result'))?.getAttribute('role') === 'status')

    await confirmRemovalReload([legacyFixtureId])
    await click(testId('plugin-removal-result-dismiss'))
    await waitFor('legacy removal result dismissed', () => store.removalStatus === 'idle')

    await click(testId('plugin-import-button'))
    await waitFor('account workflow preview', () => store.importStatus === 'preview'
      && store.importPreview?.manifest.id === accountFixtureId)
    await click(testId('plugin-import-confirm'))
    await waitFor('account workflow managed disabled import', () => (
      store.importResult?.status === 'imported'
      && fixture(accountFixtureId)?.management === 'managed'
      && fixture(accountFixtureId)?.status === 'disabled'
      && !!fixtureCard(accountFixtureId)
    ))
    await click(testId('plugin-import-dismiss'))
    await waitFor('account workflow import result dismissed', () => store.importStatus === 'idle')

    await click('input[role="switch"]', () => requireCard(accountFixtureId))
    await waitFor('account workflow enabled', () => {
      const card = fixtureCard(accountFixtureId)
      const toggle = card?.querySelector<HTMLInputElement>('input[role="switch"]')
      return fixture(accountFixtureId)?.status === 'enabled'
        && !store.pendingIds.has(accountFixtureId) && !store.actionErrors[accountFixtureId]
        && toggle?.checked === true && !toggle.disabled
    })

    await clickCommand('Prepare example limit order', accountFixtureId)
    await waitFor('workflow access with no grants', () => {
      const capabilities = [...document.querySelectorAll<HTMLInputElement>(testId('workflow-capability'))]
      return !!document.querySelector(testId('workflow-account'))
        && capabilities.length === 5
        && capabilities.every(capability => !capability.checked)
        && control(testId('workflow-run')) === null
    })
    for (const capability of [
      'account.read', 'balances.read', 'orders.read', 'trade.place', 'trade.cancel',
    ]) {
      const checkbox = document.querySelector<HTMLInputElement>(
        `${testId('workflow-capability')}[value="${capability}"]`,
      )
      if (!checkbox || checkbox.disabled) throw new Error(`Capability unavailable: ${capability}`)
      checkbox.click()
    }
    await click(testId('workflow-save-grants'))
    await waitFor('workflow grants saved', () => (
      [...document.querySelectorAll<HTMLInputElement>(testId('workflow-capability'))]
        .every(capability => capability.checked)
      && control(testId('workflow-run')) !== null
    ))
    setText(testId('workflow-input'), JSON.stringify({
      kind: 'placeOrder',
      order: {
        symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', qty: '0.002',
        price: '49000', timeInForce: 'GTC', positionIdx: 1, reduceOnly: false,
      },
    }))
    await click(testId('workflow-run'))
    await waitFor('place proposal without receipt', () => (
      !!document.querySelector(testId('workflow-snapshot'))
      && !!document.querySelector(testId('workflow-confirmation'))
      && !document.querySelector(testId('workflow-receipt-accepted'))
    ))
    await click(testId('workflow-confirm'))
    await waitFor('place accepted exactly once', () => (
      !!document.querySelector(testId('workflow-receipt-accepted'))
      && !document.querySelector(testId('workflow-confirm'))
    ))
    await click(testId('workflow-close'))
    await waitFor('place workflow closes', () => !document.querySelector(testId('plugin-workflow-dialog')))

    await clickCommand('Prepare cancellation of first captured order', accountFixtureId)
    await waitFor('cancel workflow keeps session grants', () => (
      !!document.querySelector(testId('workflow-account'))
      && control(testId('workflow-run')) !== null
    ))
    await click(testId('workflow-run'))
    await waitFor('cancel proposal without receipt', () => (
      document.querySelector(testId('workflow-confirmation'))?.textContent
        ?.includes('plugin-smoke-open-order') === true
      && !document.querySelector(testId('workflow-receipt-accepted'))
    ))
    await click(testId('workflow-confirm'))
    await waitFor('cancel accepted exactly once', () => (
      !!document.querySelector(testId('workflow-receipt-accepted'))
      && !document.querySelector(testId('workflow-confirm'))
    ))
    await click(testId('workflow-revoke'))
    await waitFor('revocation blocks another run', () => (
      [...document.querySelectorAll<HTMLInputElement>(testId('workflow-capability'))]
        .every(capability => !capability.checked)
      && control(testId('workflow-run')) === null
      && !document.querySelector(testId('workflow-snapshot'))
    ))
    await click(testId('workflow-close'))
    await waitFor('revoked workflow closes', () => !document.querySelector(testId('plugin-workflow-dialog')))

    await click('input[role="switch"]', () => requireCard(accountFixtureId))
    await waitFor('account workflow disabled', () => (
      fixture(accountFixtureId)?.status === 'disabled'
      && fixtureCard(accountFixtureId)?.querySelector<HTMLInputElement>('input[role="switch"]')
        ?.checked === false
    ))
    await confirmForbiddenWebviewIpc()
    await click(testId('plugin-remove-button'), () => requireCard(accountFixtureId))
    await waitFor('account workflow removal confirmation', () => (
      store.removalStatus === 'confirming'
      && store.removalTarget?.plugin.manifest.id === accountFixtureId
    ))
    await click(testId('plugin-removal-confirm'))
    await waitFor('account workflow removed', () => store.removalResult?.status === 'removed'
      && !fixture(accountFixtureId) && !fixtureCard(accountFixtureId))
    await confirmRemovalReload([legacyFixtureId, accountFixtureId])
    success = true
    detail = 'Real DOM: v4 SMA preserved; v5 no-grant, explicit grant, place/cancel separate confirmations, revoke, disable, forbidden IPC, removal and reload verified against injected synthetic host'
  } catch (error) {
    detail = detailWithinLimit(error)
  }
  // Exactly one submission even if the transport fails or the host exits before its reply.
  try { await tauriInvoke('finish_plugin_smoke', { success, detail }) }
  catch (error) { console.error('Smoke completion transport failed; host deadline remains authoritative', error) }
}
