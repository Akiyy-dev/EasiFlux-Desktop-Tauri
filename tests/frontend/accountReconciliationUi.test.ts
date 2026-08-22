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

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary', 'backup'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}
const profiles: AccountProfile[] = [
  {
    accountId: 'primary', label: 'Primary account', baseUrl: 'https://primary.example',
    credentialState: 'present', active: true,
  },
  {
    accountId: 'backup', label: 'Backup account', baseUrl: 'https://backup.example',
    credentialState: 'present', active: false,
  },
]
const dialogStub = {
  props: ['show'], template: '<section v-if="show"><slot/><slot name="footer"/></section>',
}

describe('post-switch reconciliation UI', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockReset()
  })

  function mountPanel() {
    return mount(AccountProfilesPanel, {
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
  }

  function mountQuickSetup() {
    return mount(QuickSetupDialog, {
      props: { show: true },
      global: { plugins: [pinia], stubs: { AppDialog: dialogStub } },
    })
  }

  it('shows shared loading/error/retry state and never offers the old account editor', async () => {
    const firstConfig = deferred<AppConfig>()
    let switched = false
    let configCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        return Promise.resolve(profiles.map((profile) => ({
          ...profile, active: switched ? profile.accountId === 'backup' : profile.active,
        })))
      }
      if (command === 'switch_account') {
        switched = true
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') {
        configCalls += 1
        return configCalls === 1 ? firstConfig.promise : Promise.resolve({
          ...config, activeAccountId: 'backup',
        })
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const panel = mountPanel()
    await flushPromises()
    const backupRow = panel.findAll('li').find((row) => row.text().includes('backup'))
    const switchButton = backupRow!.findAll('button').find((button) => button.text() === '切换')
    await switchButton!.trigger('click')
    await vi.waitFor(() => expect(useAccountProfilesStore().reconciliationLoading).toBe(true))

    const settings = mountQuickSetup()
    await flushPromises()
    expect(panel.text()).toContain('正在同步已切换的账户')
    expect(settings.text()).toContain('正在同步已切换的账户')
    expect(settings.text()).toContain('Backup account')
    expect(settings.text()).not.toContain('Primary account')
    const editorButtons = settings.findAll('button').filter((button) =>
      button.text() === '编辑凭据' || button.text() === '保存凭据并连接')
    expect(editorButtons.every((button) => button.attributes('disabled') !== undefined)).toBe(true)

    firstConfig.reject(new Error('apiKey=raw-key keyring raw-secret'))
    await vi.waitFor(() => expect(useAccountProfilesStore().reconciliationError).not.toBeNull())
    expect(panel.get('[data-testid="reconciliation-error"]').text())
      .toContain('配置刷新失败')
    expect(settings.get('[data-testid="reconciliation-error"]').text())
      .toContain('配置刷新失败')
    expect(panel.text()).not.toContain('raw-secret')
    expect(settings.text()).not.toContain('raw-key')
    expect(panel.findAll('[role="alert"]')).toHaveLength(1)
    expect(settings.findAll('[role="alert"]')).toHaveLength(1)

    await settings.get('[data-testid="reconciliation-retry"]').trigger('click')
    await vi.waitFor(() => {
      expect(useAccountProfilesStore().reconciliationError).toBeNull()
      expect(useAccountProfilesStore().reconciliationLoading).toBe(false)
    })
    expect(panel.find('[data-testid="reconciliation-error"]').exists()).toBe(false)
    expect(settings.find('[data-testid="reconciliation-error"]').exists()).toBe(false)
    const enabledEditorButtons = settings.findAll('button').filter((button) =>
      button.text() === '编辑凭据' || button.text() === '保存凭据并连接')
    expect(enabledEditorButtons
      .every((button) => button.attributes('disabled') === undefined)).toBe(true)
    expect(settings.getComponent(CredentialEditor).props('accountId')).toBe('backup')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'switch_account')).toHaveLength(1)
  })

  it('requires a stale profile error to be refreshed before a new backend switch', async () => {
    const nextSwitch = deferred<{
      activeAccountId: string
      connected: boolean
      sessionEpoch: number
    }>()
    let listCalls = 0
    let switchCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        listCalls += 1
        return listCalls === 3
          ? Promise.reject(new Error('apiKey=stale-raw-key'))
          : Promise.resolve(profiles)
      }
      if (command === 'switch_account') {
        switchCalls += 1
        return switchCalls === 1
          ? Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
          : nextSwitch.promise
      }
      if (command === 'get_config') return Promise.resolve({
        ...config, activeAccountId: switchCalls === 1 ? 'backup' : 'primary',
      })
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const settings = mountQuickSetup()
    const panel = mountPanel()
    await flushPromises()
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')
    expect(store.listError).toContain('stale-raw-key')
    const backupRow = panel.findAll('li').find((row) => row.text().includes('backup'))
    const editInPanel = backupRow!.findAll('button').find((button) => button.text() === '编辑')
    expect(editInPanel?.attributes('disabled')).toBeDefined()
    await editInPanel!.trigger('click')
    expect(panel.getComponent(CredentialEditor).props('show')).toBe(false)

    expect(panel.get('[data-testid="reconciliation-retry"]').exists()).toBe(true)
    await panel.get('[data-testid="reconciliation-retry"]').trigger('click')
    await flushPromises()
    expect(store.listError).toBeNull()

    const switching = store.switchAccount('primary')
    await flushPromises()
    expect(settings.text()).not.toContain('stale-raw-key')
    expect(settings.text()).toContain('正在切换账户')
    const edit = settings.findAll('button').find((button) => button.text() === '编辑凭据')
    expect(edit?.attributes('disabled')).toBeDefined()
    await edit!.trigger('click')
    expect(settings.getComponent(CredentialEditor).props('show')).toBe(false)
    expect(panel.getComponent(CredentialEditor).props('show')).toBe(false)

    nextSwitch.resolve({ activeAccountId: 'primary', connected: false, sessionEpoch: 2 })
    await switching
  })

  it('shows a persistent recovery block after an incomplete backend rollback', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve(profiles)
      if (command === 'switch_account') {
        return Promise.reject(new Error(
          'ACCOUNT_SWITCH_RECOVERY_REQUIRED: target failed; rollback failed: restore failed',
        ))
      }
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const panel = mountPanel()
    await flushPromises()
    const backupRow = panel.findAll('li').find((row) => row.text().includes('backup'))
    const switchButton = backupRow!.findAll('button').find((button) => button.text() === '切换')

    await switchButton!.trigger('click')
    await flushPromises()

    expect(panel.get('[data-testid="account-recovery-required"]').text())
      .toContain('账户切换需要恢复')
    expect(panel.text()).not.toContain('restore failed')
    expect(panel.find('[data-testid="reconciliation-retry"]').exists()).toBe(false)
    expect(panel.findAll('.actions button')
      .every((button) => button.attributes('disabled') !== undefined)).toBe(true)
  })
})
