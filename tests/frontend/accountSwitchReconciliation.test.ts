import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useAccountStore } from '../../src/stores/account'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import { useOrderStore } from '../../src/stores/order'
import type { AccountProfile, AccountSwitchResult, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
const errorServiceMocks = vi.hoisted(() => ({
  reportError: vi.fn(() => 'safe aggregate fallback'),
  notifyWarning: vi.fn(),
}))
vi.mock('../../src/services/errorService', () => errorServiceMocks)

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const primaryConfig: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary', 'backup'],
  riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
  riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
}
const backupConfig = { ...primaryConfig, activeAccountId: 'backup', windowWidth: 1440 }
const backupProfile: AccountProfile = {
  accountId: 'backup', label: 'Backup', baseUrl: 'https://backup.example',
  credentialState: 'present', active: true,
}

describe('post-switch reconciliation', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    useConfigStore().config = primaryConfig
    vi.mocked(tauriInvoke).mockReset()
    errorServiceMocks.reportError.mockClear()
    errorServiceMocks.notifyWarning.mockClear()
  })

  it('publishes one closed reconciliation aggregate with a run-scoped UUID', async () => {
    let websocketCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 7 })
      }
      if (command === 'get_websocket_status' && ++websocketCalls === 1) {
        return Promise.resolve('connected')
      }
      if (command === 'get_config' || command === 'list_account_profiles'
        || command === 'get_connection_status' || command === 'get_websocket_status'
        || command === 'scheduler_run_task') {
        return Promise.reject(new Error('untrusted raw phase failure'))
      }
      if (command === 'create_client_notification') {
        return Promise.resolve({
          notification: {
            id: '00000000-0000-4000-8000-000000000001',
            scope: { type: 'account', accountId: 'backup' },
            category: 'riskAccount',
            kind: 'accountReconciliationFailed',
            severity: 'critical',
            content: {
              messageKey: 'account.reconciliationFailed',
              params: { failedSteps: 'config,profiles,connection,bootstrap' },
              fallbackTitle: '账户对账失败',
              fallbackBody: '账户状态对账未完成，请检查账户配置。',
            },
            dedupeKey: 'backup:attempt:reconciliation',
            occurrenceCount: 1,
            createdAtMs: 1,
            updatedAtMs: 1,
          },
          unreadCount: 1,
          revision: '1',
        })
      }
      return Promise.resolve(undefined)
    })

    await useAccountProfilesStore().switchAccount('backup')

    const calls = vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'create_client_notification')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toEqual({
      request: {
        accountId: 'backup',
        sessionEpoch: 7,
        attemptId: expect.stringMatching(
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
        ),
        kind: 'accountReconciliationFailed',
        failedSteps: ['config', 'profiles', 'connection', 'bootstrap'],
      },
    })
    expect(errorServiceMocks.reportError).not.toHaveBeenCalled()
  })

  it('publishes recovery with controlled steps and uses one safe fallback on bridge rejection', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.reject(new Error(
          'ACCOUNT_SWITCH_RECOVERY_REQUIRED: raw keyring path C:\\secret',
        ))
      }
      if (command === 'get_connection_status' || command === 'get_websocket_status') {
        return Promise.resolve('disconnected')
      }
      if (command === 'create_client_notification') {
        return Promise.reject({ message: 'bridge raw secret must not be parsed' })
      }
      return Promise.resolve(undefined)
    })

    await expect(useAccountProfilesStore().switchAccount('backup')).rejects.toThrow(
      'ACCOUNT_SWITCH_RECOVERY_REQUIRED',
    )

    const calls = vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'create_client_notification')
    expect(calls).toHaveLength(1)
    expect(calls[0][1]).toEqual({
      request: {
        accountId: 'primary',
        sessionEpoch: 0,
        attemptId: expect.stringMatching(
          /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
        ),
        kind: 'accountRecoveryFailed',
        failedSteps: ['connection'],
      },
    })
    expect(errorServiceMocks.reportError).toHaveBeenCalledTimes(1)
    expect(errorServiceMocks.reportError).toHaveBeenCalledWith(
      expect.any(Error),
      '账户恢复失败通知提交失败',
    )
    expect((errorServiceMocks.reportError.mock.calls[0][0] as Error).message)
      .not.toContain('raw secret')
  })

  it('ignores a late reconciliation result after the session epoch advances', async () => {
    const lateConfig = deferred<AppConfig>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return lateConfig.promise
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    await vi.waitFor(() => expect(store.reconciliationLoading).toBe(true))

    expect(store.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 2, payload: 'connected' },
      () => undefined,
    )).toBe(true)
    lateConfig.reject(new Error('late config failure'))
    await switching

    expect(store.sessionEpoch).toBe(2)
    expect(store.reconciliationFailedSteps).toEqual([])
    expect(store.reconciliationLoading).toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'create_client_notification')).toHaveLength(0)
    expect(errorServiceMocks.reportError).not.toHaveBeenCalled()
  })

  it('ignores a late bridge rejection after a newer session generation takes ownership', async () => {
    const lateBridge = deferred<never>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.reject(new Error('config failed'))
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'get_websocket_status') return Promise.resolve('connected')
      if (command === 'create_client_notification') return lateBridge.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    await vi.waitFor(() => expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'create_client_notification')).toHaveLength(1))

    expect(store.handleSessionEvent(
      { accountId: 'backup', sessionEpoch: 2, payload: 'connected' },
      () => undefined,
    )).toBe(true)
    lateBridge.reject({ message: 'stale raw bridge secret' })
    await switching

    expect(store.sessionEpoch).toBe(2)
    expect(store.reconciliationFailedSteps).toEqual([])
    expect(store.reconciliationLoading).toBe(false)
    expect(errorServiceMocks.reportError).not.toHaveBeenCalled()
  })

  it.each(['api', 'websocket'] as const)(
    'suppresses a same-context stale %s rejection after a newer status refresh succeeds',
    async (lateChannel) => {
      const lateApi = deferred<'connecting'>()
      const lateWebsocket = deferred<'connecting'>()
      let apiCalls = 0
      let websocketCalls = 0
      vi.mocked(tauriInvoke).mockImplementation((command) => {
        if (command === 'switch_account') {
          return Promise.reject(new Error('target failed and rolled back'))
        }
        if (command === 'get_connection_status') {
          apiCalls += 1
          return apiCalls === 1 ? lateApi.promise : Promise.resolve('connected')
        }
        if (command === 'get_websocket_status') {
          websocketCalls += 1
          return websocketCalls === 1
            ? lateWebsocket.promise
            : Promise.resolve('connected')
        }
        return Promise.resolve(undefined)
      })
      const store = useAccountProfilesStore()
      const connection = useConnectionStore()
      const switching = store.switchAccount('backup')
      await vi.waitFor(() => {
        expect(apiCalls).toBe(1)
        expect(websocketCalls).toBe(1)
      })

      await Promise.all([connection.refreshStatus(), connection.refreshWsStatus()])
      if (lateChannel === 'api') {
        lateApi.reject(new Error('stale api rejection'))
        lateWebsocket.resolve('connecting')
      } else {
        lateApi.resolve('connecting')
        lateWebsocket.reject(new Error('stale websocket rejection'))
      }
      await expect(switching).rejects.toThrow('target failed and rolled back')

      expect(connection.status).toBe('connected')
      expect(connection.wsStatus).toBe('connected')
      expect(store.recoveryRequired).toBe(false)
      expect(store.switching).toBe(false)
      expect(vi.mocked(tauriInvoke).mock.calls
        .filter(([command]) => command === 'create_client_notification')).toHaveLength(0)
      expect(errorServiceMocks.reportError).not.toHaveBeenCalled()
    },
  )

  it.each(['fetch', 'save'] as const)(
    'does not let an old reconciliation config response overwrite a newer config %s',
    async (newerOwner) => {
      const oldReconciliation = deferred<AppConfig>()
      let getConfigCalls = 0
      vi.mocked(tauriInvoke).mockImplementation((command) => {
        if (command === 'switch_account') {
          return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
        }
        if (command === 'get_websocket_status') return Promise.resolve('connected')
        if (command === 'get_config') {
          getConfigCalls += 1
          return getConfigCalls === 1
            ? oldReconciliation.promise
            : Promise.resolve({ ...backupConfig, windowWidth: 1777 })
        }
        if (command === 'save_config') {
          return Promise.resolve({ ...backupConfig, windowWidth: 1888 })
        }
        if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
        if (command === 'get_connection_status') return Promise.resolve('connected')
        return Promise.resolve(undefined)
      })
      const store = useAccountProfilesStore()
      const configStore = useConfigStore()
      const switching = store.switchAccount('backup')
      await vi.waitFor(() => expect(getConfigCalls).toBe(1))

      if (newerOwner === 'fetch') {
        await configStore.fetchConfig()
      } else {
        await configStore.saveConfig({ ...backupConfig, windowWidth: 1888 })
      }
      oldReconciliation.resolve({ ...backupConfig, windowWidth: 900 })
      await switching

      expect(configStore.config?.windowWidth).toBe(newerOwner === 'fetch' ? 1777 : 1888)
      expect(store.reconciliationFailedSteps).toEqual([])
    },
  )

  it('classifies local account-state clearing as the bootstrap recovery phase', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
      }
      return Promise.resolve(undefined)
    })
    const account = useAccountStore()
    vi.spyOn(account, 'clearAccountData').mockImplementationOnce(() => {
      throw new Error('local clear failed')
    })

    await expect(useAccountProfilesStore().switchAccount('backup')).rejects.toThrow(
      'local clear failed',
    )

    const bridgeCall = vi.mocked(tauriInvoke).mock.calls
      .find(([command]) => command === 'create_client_notification')
    expect(bridgeCall?.[1]).toMatchObject({
      request: {
        accountId: 'backup',
        sessionEpoch: 1,
        kind: 'accountRecoveryFailed',
        failedSteps: ['bootstrap'],
      },
    })
  })

  it('releases its transition token when its run is invalidated during the backend switch', async () => {
    const pendingSwitch = deferred<AccountSwitchResult>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pendingSwitch.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    store.adoptSessionEpoch(1)
    pendingSwitch.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 2 })

    await switching

    expect(store.switching).toBe(false)
    expect(store.tradingBlocked).toBe(false)
  })

  it('releases its transition token when invalidated during initial websocket refresh', async () => {
    const initialWebsocket = deferred<'connected'>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_websocket_status') return initialWebsocket.promise
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_websocket_status'))
    store.adoptSessionEpoch(2)
    initialWebsocket.resolve('connected')

    await switching

    expect(store.switching).toBe(false)
    expect(store.tradingBlocked).toBe(false)
  })

  const failures = [
    ['config', 'get_config', '配置刷新失败'],
    ['profiles', 'list_account_profiles', '账户配置刷新失败'],
    ['connection', 'get_connection_status', '连接状态刷新失败'],
    ['bootstrap', 'scheduler_run_task', '账户数据初始化失败'],
  ] as const

  it.each(failures)(
    'keeps a successful switch committed when %s reconciliation fails',
    async (step, failedCommand, visibleMessage) => {
      useAccountStore().applySnapshot({ accountId: 'primary', balances: [], totalEquity: '10' })
      vi.mocked(tauriInvoke).mockImplementation((command) => {
        if (command === 'switch_account') {
          return Promise.resolve({ activeAccountId: ' backup ', connected: true, sessionEpoch: 1 })
        }
        if (command === failedCommand) {
          return Promise.reject(new Error('apiKey=raw-key keyring raw-secret'))
        }
        if (command === 'get_config') return Promise.resolve(backupConfig)
        if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
        if (command === 'get_connection_status') return Promise.resolve('connected')
        return Promise.resolve(undefined)
      })
      const store = useAccountProfilesStore()

      await expect(store.switchAccount('backup')).resolves.toMatchObject({
        activeAccountId: ' backup ', connected: true, sessionEpoch: 1,
      })

      expect(store.activeAccountId).toBe('backup')
      expect(useConfigStore().config?.activeAccountId).toBe('backup')
      expect(useAccountStore().summary).toBeNull()
      expect(store.switchError).toBeNull()
      expect(store.reconciliationFailedSteps).toEqual([step])
      expect(store.reconciliationError).toContain(visibleMessage)
      expect(store.reconciliationError).not.toContain('raw-key')
      expect(store.reconciliationError).not.toContain('raw-secret')
      expect(store.switching).toBe(false)
      const shouldBlockTrading = step === 'connection' || step === 'bootstrap'
      expect(store.tradingBlocked).toBe(shouldBlockTrading)
      if (shouldBlockTrading) {
        expect(store.tradingBlockedMessage).toContain('账户同步失败')
      } else {
        expect(store.tradingBlockedMessage).toBeNull()
      }
    },
  )

  it('adopts the result immediately and keeps all mutation guards during reconciliation', async () => {
    const configResult = deferred<AppConfig>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: ' backup ', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return configResult.promise
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')

    await vi.waitFor(() => expect(store.reconciliationLoading).toBe(true))
    expect(store.activeAccountId).toBe('backup')
    expect(useConfigStore().config?.activeAccountId).toBe('backup')
    expect(store.switching).toBe(true)
    expect(store.mutating).toBe(true)

    configResult.resolve(backupConfig)
    await switching
    expect(store.reconciliationLoading).toBe(false)
    expect(store.switching).toBe(false)
  })

  it('prevents an older get_config response from overwriting post-switch state', async () => {
    const stale = deferred<AppConfig>()
    const current = deferred<AppConfig>()
    let configCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_config') return ++configCalls === 1 ? stale.promise : current.promise
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
      }
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const configStore = useConfigStore()
    const staleFetch = configStore.fetchConfig()
    const switching = useAccountProfilesStore().switchAccount('backup')
    await vi.waitFor(() => expect(configCalls).toBe(2))
    current.resolve(backupConfig)
    await switching
    stale.resolve({ ...primaryConfig, windowWidth: 900 })
    await staleFetch

    expect(configStore.config?.activeAccountId).toBe('backup')
    expect(configStore.config?.windowWidth).toBe(1440)
  })

  it('retries reconciliation once without repeating the committed switch', async () => {
    const retryConfig = deferred<AppConfig>()
    let configCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') {
        return ++configCalls === 1
          ? Promise.reject(new Error('temporary config failure'))
          : retryConfig.promise
      }
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')
    expect(store.reconciliationError).not.toBeNull()

    const firstRetry = store.retryReconciliation()
    const secondRetry = store.retryReconciliation()
    expect(store.reconciliationLoading).toBe(true)
    expect(configCalls).toBe(2)
    retryConfig.resolve(backupConfig)
    await Promise.all([firstRetry, secondRetry])
    await flushPromises()

    expect(store.reconciliationError).toBeNull()
    expect(store.reconciliationFailedSteps).toEqual([])
    expect(configCalls).toBe(2)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'switch_account')).toHaveLength(1)
  })

  it('does not strand an older normal profile refresh when reconciliation becomes newer', async () => {
    const normalProfiles = deferred<AccountProfile[]>()
    const retryProfiles = deferred<AccountProfile[]>()
    const freshProfile = { ...backupProfile, label: 'Fresh backup' }
    let configCalls = 0
    let listCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') {
        configCalls += 1
        return configCalls === 1
          ? Promise.reject(new Error('temporary config failure'))
          : Promise.resolve(backupConfig)
      }
      if (command === 'list_account_profiles') {
        listCalls += 1
        if (listCalls === 1) return Promise.resolve([backupProfile])
        return listCalls === 2 ? normalProfiles.promise : retryProfiles.promise
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')

    const normalRefresh = store.refreshProfiles()
    const retry = store.retryReconciliation()
    await vi.waitFor(() => expect(listCalls).toBe(3))
    retryProfiles.resolve([backupProfile])
    await retry
    normalProfiles.resolve([freshProfile])
    await normalRefresh

    expect(store.profiles).toEqual([backupProfile])
    expect(store.loading).toBe(false)
    expect(store.listError).toBeNull()
  })

  it('does not let older reconciliation profiles overwrite a newer normal success', async () => {
    const reconciliationProfiles = deferred<AccountProfile[]>()
    const freshProfile = { ...backupProfile, label: 'Fresh backup' }
    let listCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.resolve(backupConfig)
      if (command === 'list_account_profiles') {
        listCalls += 1
        return listCalls === 1
          ? reconciliationProfiles.promise
          : Promise.resolve([freshProfile])
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    await vi.waitFor(() => expect(listCalls).toBe(1))

    await store.refreshProfiles()
    expect(store.profiles).toEqual([freshProfile])
    reconciliationProfiles.resolve([backupProfile])
    await switching

    expect(store.profiles).toEqual([freshProfile])
    expect(store.listError).toBeNull()
  })

  it('does not let older reconciliation success clear a newer normal failure', async () => {
    const reconciliationProfiles = deferred<AccountProfile[]>()
    let listCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.resolve(backupConfig)
      if (command === 'list_account_profiles') {
        listCalls += 1
        return listCalls === 1
          ? reconciliationProfiles.promise
          : Promise.reject(new Error('newer normal profile failure'))
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const switching = store.switchAccount('backup')
    await vi.waitFor(() => expect(listCalls).toBe(1))

    await expect(store.refreshProfiles()).rejects.toThrow('newer normal profile failure')
    expect(store.listError).toBe('newer normal profile failure')
    reconciliationProfiles.resolve([backupProfile])
    await switching

    expect(store.profiles).toEqual([])
    expect(store.listError).toBe('newer normal profile failure')
  })

  it('preserves the successful reconciliation command order', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.resolve(backupConfig)
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    await useAccountProfilesStore().switchAccount('backup')
    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toEqual([
      'switch_account', 'get_websocket_status', 'get_config', 'list_account_profiles',
      'get_connection_status', 'get_websocket_status', 'scheduler_run_task',
    ])
  })

  it('uses the closed reconciliation bootstrap after switching to a disconnected account', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
      }
      if (command === 'get_config') return Promise.resolve(backupConfig)
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })

    await useAccountProfilesStore().switchAccount('backup')

    expect(vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)).toEqual([
      'switch_account', 'get_websocket_status', 'get_config', 'list_account_profiles',
      'get_connection_status', 'get_websocket_status', 'scheduler_run_task',
    ])
    expect(tauriInvoke).toHaveBeenLastCalledWith('scheduler_run_task', {
      task: 'reconciliationBootstrap', force: true,
    })
  })

  it('keeps a backend recovery marker blocked across refreshes, retries, and events', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        return Promise.reject(new Error(
          'ACCOUNT_SWITCH_RECOVERY_REQUIRED: rollback could not restore a safe session',
        ))
      }
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      if (command === 'get_websocket_status') return Promise.resolve('disconnected')
      if (command === 'list_account_profiles') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const connection = useConnectionStore()
    connection.setWsStatus('connected')

    await expect(store.switchAccount('backup')).rejects.toThrow(
      'ACCOUNT_SWITCH_RECOVERY_REQUIRED',
    )
    expect(store.recoveryRequired).toBe(true)
    expect(store.accountMutationsBlocked).toBe(true)
    expect(store.tradingBlocked).toBe(true)
    expect(store.tradingBlockedMessage).toContain('需要恢复')
    expect(connection.wsStatus).toBe('disconnected')

    await store.refreshProfiles()
    await store.retryReconciliation()
    const accepted = store.handleSessionEvent(
      { accountId: 'primary', sessionEpoch: 0, payload: 'connected' },
      (status) => connection.setWsStatus(status),
    )

    expect(store.recoveryRequired).toBe(true)
    expect(accepted).toBe(false)
    expect(connection.wsStatus).toBe('disconnected')
    expect(store.accountMutationsBlocked).toBe(true)
    await expect(store.deleteAccount('backup')).rejects.toThrow('需要恢复')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'delete_account')).toHaveLength(0)
  })

  it('fails closed when connection status cannot be refreshed after a failed switch', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return Promise.reject(new Error('target failed'))
      if (command === 'get_connection_status') {
        return Promise.reject(new Error('status unavailable'))
      }
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    const connection = useConnectionStore()

    await expect(store.switchAccount('backup')).rejects.toThrow('target failed')

    expect(connection.status).toBe('error')
    expect(store.recoveryRequired).toBe(true)
    expect(store.accountMutationsBlocked).toBe(true)
    expect(store.tradingBlocked).toBe(true)
  })

  it('restores the former reconciliation context after a normal rollback', async () => {
    let switchCalls = 0
    let statusCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') {
        switchCalls += 1
        return switchCalls === 1
          ? Promise.resolve({ activeAccountId: 'backup', connected: true, sessionEpoch: 1 })
          : Promise.reject(new Error('target failed and rolled back'))
      }
      if (command === 'get_config') return Promise.resolve(backupConfig)
      if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
      if (command === 'get_connection_status') {
        statusCalls += 1
        return Promise.resolve('connected')
      }
      return Promise.resolve(undefined)
    })
    const store = useAccountProfilesStore()
    await store.switchAccount('backup')
    expect(store.reconciliationFailedSteps).toEqual([])
    useAccountStore().applySnapshot({ accountId: 'backup', balances: [], totalEquity: '20' })
    useOrderStore().setOpenOrders([{ orderId: 'backup-order' } as never])

    await expect(store.switchAccount('primary')).rejects.toThrow('target failed and rolled back')

    expect(statusCalls).toBe(2)
    expect(store.activeAccountId).toBe('backup')
    expect(store.reconciliationFailedSteps).toEqual([])
    expect(useAccountStore().summary?.accountId).toBe('backup')
    expect(useOrderStore().openOrders).toHaveLength(1)
    await store.retryReconciliation()
    expect(store.reconciliationFailedSteps).toEqual([])
    expect(store.activeAccountId).toBe('backup')
  })

  it.each([
    ['save', 'switch'],
    ['delete', 'switch'],
    ['save', 'reconciliation'],
    ['delete', 'reconciliation'],
  ] as const)(
    'rejects a programmatic %s during account %s',
    async (mutation, phase) => {
      const pendingSwitch = deferred<{
        activeAccountId: string
        connected: boolean
        sessionEpoch: number
      }>()
      const pendingConfig = deferred<AppConfig>()
      vi.mocked(tauriInvoke).mockImplementation((command) => {
        if (command === 'switch_account') {
          return phase === 'switch'
            ? pendingSwitch.promise
            : Promise.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
        }
        if (command === 'get_config') return pendingConfig.promise
        if (command === 'list_account_profiles') return Promise.resolve([backupProfile])
        if (command === 'get_connection_status') return Promise.resolve('disconnected')
        return Promise.resolve(undefined)
      })
      const store = useAccountProfilesStore()
      const switching = store.switchAccount('backup')
      if (phase === 'reconciliation') {
        await vi.waitFor(() => expect(store.reconciliationLoading).toBe(true))
      }

      let mutationError: unknown
      try {
        if (mutation === 'save') {
          await store.saveCredentials({
            accountId: 'backup', apiKey: '', apiSecret: '', label: 'Backup',
            baseUrl: 'https://backup.example',
          })
        } else {
          await store.deleteAccount('backup')
        }
      } catch (error) {
        mutationError = error
      }

      pendingSwitch.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
      pendingConfig.resolve(backupConfig)
      await switching
      expect(mutationError).toBeInstanceOf(Error)
      expect((mutationError as Error).message).toContain('暂时无法更改账户')
      const command = mutation === 'save' ? 'save_credentials' : 'delete_account'
      expect(vi.mocked(tauriInvoke).mock.calls
        .filter(([called]) => called === command)).toHaveLength(0)
    },
  )
})
