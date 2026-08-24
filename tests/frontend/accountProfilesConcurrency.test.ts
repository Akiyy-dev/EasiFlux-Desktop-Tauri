import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountProfilesPanel from '../../src/components/account/AccountProfilesPanel.vue'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import QuickSetupDialog from '../../src/components/settings/QuickSetupDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import type { AccountProfile, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const config = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary'], riskEnabled: true,
  riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5', riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
} satisfies AppConfig

const staleProfile: AccountProfile = {
  accountId: 'primary', label: 'Stale', baseUrl: 'https://stale.example',
  credentialState: 'present', active: true,
}
const freshProfile: AccountProfile = {
  accountId: 'primary', label: 'Fresh', baseUrl: 'https://fresh.example',
  credentialState: 'present', active: true,
}
const dialogStub = {
  props: ['show'], template: '<section v-if="show"><slot/><slot name="footer"/></section>',
}

describe('account profile refresh concurrency', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockReset()
  })

  function startPanelThenSettings() {
    const older = deferred<AccountProfile[]>()
    const newer = deferred<AccountProfile[]>()
    let listCall = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command !== 'list_account_profiles') return Promise.resolve(undefined)
      listCall += 1
      return listCall === 1 ? older.promise : newer.promise
    })
    mount(AccountProfilesPanel, {
      global: { plugins: [pinia], stubs: { CredentialEditor: true, AppDialog: true } },
    })
    const settings = mount(QuickSetupDialog, {
      props: { show: true },
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
    expect(listCall).toBe(2)
    return { older, newer, settings, store: useAccountProfilesStore() }
  }

  function expectFreshSettings(
    settings: ReturnType<typeof startPanelThenSettings>['settings'],
  ): void {
    const store = useAccountProfilesStore()
    expect(store.profiles).toEqual([freshProfile])
    expect(store.listError).toBeNull()
    expect(store.loading).toBe(false)
    const edit = settings.findAll('button').find((button) => button.text() === '编辑凭据')
    expect(edit?.attributes('disabled')).toBeUndefined()
    expect(settings.getComponent(CredentialEditor).props()).toMatchObject({
      initialLabel: 'Fresh', initialBaseUrl: 'https://fresh.example',
    })
  }

  it('ignores an older success after Settings loads the fresh profile', async () => {
    const { older, newer, settings } = startPanelThenSettings()
    newer.resolve([{ ...freshProfile, apiKey: 'hidden-key', apiSecret: 'hidden-secret' }])
    await flushPromises()
    expectFreshSettings(settings)

    older.resolve([staleProfile])
    await flushPromises()
    expectFreshSettings(settings)
  })

  it('ignores and consumes an older rejection after the fresh profile loads', async () => {
    const { older, newer, settings } = startPanelThenSettings()
    newer.resolve([freshProfile])
    await flushPromises()
    expectFreshSettings(settings)

    older.reject(new Error('stale profile failure'))
    await flushPromises()
    expectFreshSettings(settings)
  })

  it('reports a normal current-request error and clears loading', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('current profile failure'))
    const store = useAccountProfilesStore()
    const refresh = store.refreshProfiles()
    expect(store.loading).toBe(true)
    await expect(refresh).rejects.toThrow('current profile failure')
    expect(store.listError).toBe('current profile failure')
    expect(store.loading).toBe(false)
  })

  it('blocks mutation during an older UI refresh and allows it after refresh', async () => {
    const older = deferred<AccountProfile[]>()
    const mutationRefresh = deferred<AccountProfile[]>()
    let listCall = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'save_credentials') return Promise.resolve(undefined)
      if (command === 'list_account_profiles') {
        listCall += 1
        return listCall === 1 ? older.promise : mutationRefresh.promise
      }
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const oldRefresh = store.refreshProfiles()
    const request = {
      accountId: 'primary', apiKey: '', apiSecret: '', label: 'Fresh',
      baseUrl: 'https://fresh.example',
    }

    await expect(store.saveCredentials(request)).rejects.toThrow(
      '请先刷新账户列表',
    )
    expect(tauriInvoke).not.toHaveBeenCalledWith('save_credentials', expect.anything())
    expect(listCall).toBe(1)
    older.resolve([staleProfile])
    await expect(oldRefresh).resolves.toEqual([staleProfile])

    await store.saveCredentials(request)
    expect(listCall).toBe(2)
    mutationRefresh.resolve([freshProfile])
    await flushPromises()
    expect(store.profiles).toEqual([freshProfile])
    expect(store.listError).toBeNull()
  })

  it('keeps a normal profile refresh owned when the same account advances epoch', async () => {
    const pending = deferred<AccountProfile[]>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return pending.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const refresh = store.refreshProfiles()
    expect(store.loading).toBe(true)

    store.adoptSessionEpoch(1)
    pending.resolve([freshProfile])
    await refresh

    expect(store.profiles).toEqual([freshProfile])
    expect(store.loading).toBe(false)
    expect(store.listError).toBeNull()
  })
})
