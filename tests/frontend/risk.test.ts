import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useRiskStore } from '../../src/stores/risk'
import { useConfigStore } from '../../src/stores/config'
import type { AppConfig, RiskStatus, UpdateRiskConfigRequest } from '../../src/types/models'
import { validateRiskConfig } from '../../src/utils/risk'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const ready: RiskStatus = {
  enabled: true,
  maxOrderQty: '100',
  maxPriceDeviationPct: '5',
  maxDailyOrders: 500,
  tradingDayTimezone: 'Asia/Shanghai',
  ledgerState: 'ready',
  tradingDay: '2026-07-22',
  occupiedOrders: 12,
  remainingOrders: 488,
  updatedAtMs: 1_784_692_800_000,
  error: null,
}

function request(overrides: Partial<UpdateRiskConfigRequest> = {}): UpdateRiskConfigRequest {
  return {
    enabled: true,
    maxOrderQty: '10',
    maxPriceDeviationPct: '1',
    maxDailyOrders: 25,
    tradingDayTimezone: 'UTC',
    ...overrides,
  }
}

describe('risk validator and store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it.each([
    ['invalid quantity', { maxOrderQty: 'abc' }],
    ['zero quantity', { maxOrderQty: '0' }],
    ['negative quantity', { maxOrderQty: '-1' }],
    ['invalid deviation', { maxPriceDeviationPct: 'abc' }],
    ['negative deviation', { maxPriceDeviationPct: '-1' }],
    ['non-positive limit', { maxDailyOrders: 0 }],
    ['blank timezone', { tradingDayTimezone: '   ' }],
  ])('rejects %s before invoking the backend', async (_label, overrides) => {
    const invalid = request(overrides)
    expect(validateRiskConfig(invalid)).not.toBeNull()

    await expect(useRiskStore().save(invalid)).rejects.toThrow()

    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('returns Chinese validation messages', () => {
    expect(validateRiskConfig(request({ maxOrderQty: '0' })))
      .toBe('最大单笔下单数量必须是大于 0 的十进制数。')
    expect(validateRiskConfig(request({ maxPriceDeviationPct: '-1' })))
      .toBe('最大价格偏离必须是非负十进制数。')
    expect(validateRiskConfig(request({ maxDailyOrders: 0 })))
      .toBe('每日最大下单次数必须是正整数。')
    expect(validateRiskConfig(request({ tradingDayTimezone: '   ' })))
      .toBe('交易日时区不能为空。')
  })

  it('refreshes status through the read-only command', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(ready)
    const store = useRiskStore()

    await store.refresh()

    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
    expect(store.status).toEqual(ready)
  })

  it('trims saves and adopts the returned snapshot', async () => {
    const updated = { ...ready, maxOrderQty: '25.5', tradingDayTimezone: 'UTC' }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(updated)
    const store = useRiskStore()

    await store.save(request({
      maxOrderQty: ' 25.5 ',
      maxPriceDeviationPct: ' 0 ',
      tradingDayTimezone: ' UTC ',
    }))

    expect(tauriInvoke).toHaveBeenCalledWith('update_risk_config', {
      request: {
        enabled: true,
        maxOrderQty: '25.5',
        maxPriceDeviationPct: '0',
        maxDailyOrders: 25,
        tradingDayTimezone: 'UTC',
      },
    })
    expect(store.status).toEqual(updated)
  })

  it('keeps risk fields authoritative when ordinary settings save next', async () => {
    const config = useConfigStore()
    config.config = {
      activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
      theme: 'dark', klineInterval: '15', useWebsocket: true,
      wsPublicUrl: 'wss://example.test/public', wsPrivateUrl: 'wss://example.test/private',
      tickerPollInterval: 1, windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
      riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
      riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
    }
    const updated = { ...ready, maxOrderQty: '25.5', maxDailyOrders: 20,
      tradingDayTimezone: 'UTC' }
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'update_risk_config') return Promise.resolve(updated)
      if (command === 'save_config') return Promise.resolve(args?.config)
      return Promise.resolve(undefined)
    })

    await useRiskStore().save(request({ maxOrderQty: '25.5', maxDailyOrders: 20 }))
    await config.saveConfig({ ...config.config!, windowWidth: 1440 })

    expect(tauriInvoke).toHaveBeenLastCalledWith('save_config', {
      config: expect.objectContaining({
        windowWidth: 1440,
        riskMaxOrderQty: '25.5',
        riskMaxDailyOrders: 20,
        tradingDayTimezone: 'UTC',
      }),
    })
  })

  it('invalidates an older config fetch when a risk save becomes authoritative', async () => {
    const initial: AppConfig = {
      activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
      theme: 'dark', klineInterval: '15', useWebsocket: true,
      wsPublicUrl: 'wss://example.test/public', wsPrivateUrl: 'wss://example.test/private',
      tickerPollInterval: 1, windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
      riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
      riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
    }
    const staleFetch = deferred<AppConfig>()
    const saved = {
      ...ready,
      enabled: false,
      maxOrderQty: '25.5',
      maxPriceDeviationPct: '2',
      maxDailyOrders: 20,
      tradingDayTimezone: 'UTC',
    }
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_config') return staleFetch.promise
      if (command === 'update_risk_config') return Promise.resolve(saved)
      return Promise.resolve(undefined)
    })
    const config = useConfigStore()
    config.config = initial

    const oldFetch = config.fetchConfig()
    expect(config.loading).toBe(true)
    await useRiskStore().save(request({
      enabled: false,
      maxOrderQty: '25.5',
      maxPriceDeviationPct: '2',
      maxDailyOrders: 20,
      tradingDayTimezone: 'UTC',
    }))

    expect(config.loading).toBe(false)
    expect(config.config).toEqual({
      ...initial,
      riskEnabled: false,
      riskMaxOrderQty: '25.5',
      riskMaxPriceDeviationPct: '2',
      riskMaxDailyOrders: 20,
      tradingDayTimezone: 'UTC',
    })

    staleFetch.resolve({
      ...initial,
      windowWidth: 900,
      riskEnabled: true,
      riskMaxOrderQty: '1',
      riskMaxPriceDeviationPct: '1',
      riskMaxDailyOrders: 1,
      tradingDayTimezone: 'Europe/London',
    })
    await oldFetch

    expect(config.config?.windowWidth).toBe(1200)
    expect(config.config).toEqual(expect.objectContaining({
      riskEnabled: false,
      riskMaxOrderQty: '25.5',
      riskMaxPriceDeviationPct: '2',
      riskMaxDailyOrders: 20,
      tradingDayTimezone: 'UTC',
    }))
  })

  it('finishes an earlier refresh before starting save and keeps the saved snapshot', async () => {
    const refreshed = { ...ready, occupiedOrders: 13, remainingOrders: 487 }
    const saved = { ...ready, maxOrderQty: '25.5', maxDailyOrders: 20,
      tradingDayTimezone: 'UTC' }
    const refreshGate = deferred<RiskStatus>()
    const saveGate = deferred<RiskStatus>()
    const calls: string[] = []
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      calls.push(command)
      if (command === 'get_risk_status') return refreshGate.promise
      if (command === 'update_risk_config') return saveGate.promise
      return Promise.resolve(undefined)
    })
    const config = useConfigStore()
    config.config = {
      activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
      theme: 'dark', klineInterval: '15', useWebsocket: true,
      wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1,
      windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
      riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
      riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
    }
    const store = useRiskStore()

    const refreshPromise = store.refresh()
    const savePromise = store.save(request({ maxOrderQty: '25.5', maxDailyOrders: 20 }))

    expect(calls).toEqual(['get_risk_status'])
    expect(store.reading).toBe(true)
    expect(store.saving).toBe(false)

    refreshGate.resolve(refreshed)
    await refreshPromise
    await Promise.resolve()
    expect(calls).toEqual(['get_risk_status', 'update_risk_config'])
    expect(store.reading).toBe(false)
    expect(store.saving).toBe(true)

    saveGate.resolve(saved)
    await savePromise
    expect(store.status).toEqual(saved)
    expect(config.config).toEqual(expect.objectContaining({
      riskEnabled: saved.enabled,
      riskMaxOrderQty: saved.maxOrderQty,
      riskMaxPriceDeviationPct: saved.maxPriceDeviationPct,
      riskMaxDailyOrders: saved.maxDailyOrders,
      tradingDayTimezone: saved.tradingDayTimezone,
    }))
  })

  it('finishes an earlier save before starting refresh and keeps the refreshed snapshot', async () => {
    const saved = { ...ready, maxOrderQty: '25.5' }
    const refreshed = { ...saved, occupiedOrders: 14, remainingOrders: 486 }
    const saveGate = deferred<RiskStatus>()
    const refreshGate = deferred<RiskStatus>()
    const calls: string[] = []
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      calls.push(command)
      if (command === 'update_risk_config') return saveGate.promise
      if (command === 'get_risk_status') return refreshGate.promise
      return Promise.resolve(undefined)
    })
    const store = useRiskStore()

    const savePromise = store.save(request({ maxOrderQty: '25.5' }))
    const refreshPromise = store.refresh()

    expect(calls).toEqual(['update_risk_config'])
    expect(store.saving).toBe(true)
    expect(store.reading).toBe(false)

    saveGate.resolve(saved)
    await savePromise
    await Promise.resolve()
    expect(calls).toEqual(['update_risk_config', 'get_risk_status'])
    expect(store.saving).toBe(false)
    expect(store.reading).toBe(true)

    refreshGate.resolve(refreshed)
    await refreshPromise
    expect(store.status).toEqual(refreshed)
    expect(store.reading).toBe(false)
  })

  it('continues the queue after failure without stale loading cleanup', async () => {
    const refreshGate = deferred<RiskStatus>()
    const saved = { ...ready, maxOrderQty: '30' }
    const saveGate = deferred<RiskStatus>()
    const calls: string[] = []
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      calls.push(command)
      if (command === 'get_risk_status') return refreshGate.promise
      if (command === 'update_risk_config') return saveGate.promise
      return Promise.resolve(undefined)
    })
    const store = useRiskStore()

    const refreshPromise = store.refresh()
    const savePromise = store.save(request({ maxOrderQty: '30' }))
    refreshGate.reject(new Error('refresh failed'))

    await expect(refreshPromise).rejects.toThrow('refresh failed')
    await Promise.resolve()
    expect(calls).toEqual(['get_risk_status', 'update_risk_config'])
    expect(store.readError).toBeNull()
    expect(store.updateError).toBeNull()
    expect(store.reading).toBe(false)
    expect(store.saving).toBe(true)

    saveGate.resolve(saved)
    await savePromise
    expect(store.status).toEqual(saved)
    expect(store.saving).toBe(false)
  })

  it('clears a save failure when the queued refresh becomes current', async () => {
    const saveGate = deferred<RiskStatus>()
    const refreshGate = deferred<RiskStatus>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'update_risk_config') return saveGate.promise
      if (command === 'get_risk_status') return refreshGate.promise
      return Promise.resolve(undefined)
    })
    const store = useRiskStore()
    const savePromise = store.save(request()).catch((error: unknown) => error)
    const refreshPromise = store.refresh().catch((error: unknown) => error)

    saveGate.reject(new Error('save failed'))
    expect(await savePromise).toEqual(new Error('save failed'))
    await Promise.resolve()
    expect(store.saving).toBe(false)
    expect(store.reading).toBe(true)
    expect(store.updateError).toBeNull()
    expect(store.readError).toBeNull()

    refreshGate.reject(new Error('refresh failed'))
    expect(await refreshPromise).toEqual(new Error('refresh failed'))
    expect(store.readError).toBe('refresh failed')
    expect(store.updateError).toBeNull()
  })

  it('orders invalid save side effects as the only current error without backend invocation', async () => {
    const refreshGate = deferred<RiskStatus>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_risk_status') return refreshGate.promise
      return Promise.resolve(undefined)
    })
    const store = useRiskStore()
    const refreshPromise = store.refresh().catch((error: unknown) => error)
    const invalidPromise = store.save(request({ maxOrderQty: '0' }))
      .catch((error: unknown) => error)

    expect(store.updateError).toBeNull()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)

    refreshGate.reject(new Error('refresh failed'))
    expect(await refreshPromise).toEqual(new Error('refresh failed'))
    expect(await invalidPromise).toBeInstanceOf(Error)
    expect(store.readError).toBeNull()
    expect(store.updateError).not.toBeNull()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
  })

  it('serializes multiple refreshes in their invocation order', async () => {
    const first = deferred<RiskStatus>()
    const second = deferred<RiskStatus>()
    const calls: string[] = []
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      calls.push(command)
      return calls.length === 1 ? first.promise : second.promise
    })
    const store = useRiskStore()

    const firstPromise = store.refresh()
    const secondPromise = store.refresh()
    expect(calls).toEqual(['get_risk_status'])

    first.resolve({ ...ready, occupiedOrders: 13 })
    await firstPromise
    await Promise.resolve()
    expect(calls).toEqual(['get_risk_status', 'get_risk_status'])

    const latest = { ...ready, occupiedOrders: 14 }
    second.resolve(latest)
    await secondPromise
    expect(store.status).toEqual(latest)
  })
})
