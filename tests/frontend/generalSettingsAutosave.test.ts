import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useGeneralSettingsAutosave } from '../../src/composables/useGeneralSettingsAutosave'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useConfigStore } from '../../src/stores/config'
import type { AppConfig, GeneralSettings } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => {
    resolve = done
    reject = fail
  })
  return { promise, resolve, reject }
}

function makeConfig(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    activeSymbol: 'BTCUSDT',
    activeAccountId: 'primary',
    watchlistSymbols: ['BTCUSDT'],
    theme: 'dark',
    klineInterval: '15',
    useWebsocket: true,
    wsPublicUrl: 'wss://example.test/public',
    wsPrivateUrl: 'wss://example.test/private',
    tickerPollInterval: 1,
    windowWidth: 1200,
    windowHeight: 800,
    accounts: ['primary'],
    riskEnabled: true,
    riskMaxOrderQty: '10',
    riskMaxPriceDeviationPct: '5',
    riskMaxDailyOrders: 100,
    tradingDayTimezone: 'Asia/Shanghai',
    ...overrides,
  }
}

async function flushPromises(): Promise<void> {
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
  await Promise.resolve()
}

describe('general settings config store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('updates only the returned general settings fields', async () => {
    const store = useConfigStore()
    store.config = makeConfig({ activeSymbol: 'ETHUSDT', windowWidth: 1555 })
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      useWebsocket: false,
      tickerPollInterval: 15,
    })

    await store.updateGeneralSettings({ useWebsocket: false, tickerPollInterval: 15 })

    expect(tauriInvoke).toHaveBeenCalledWith('update_general_settings', {
      request: { useWebsocket: false, tickerPollInterval: 15 },
    })
    expect(store.config).toMatchObject({
      activeSymbol: 'ETHUSDT',
      windowWidth: 1555,
      useWebsocket: false,
      tickerPollInterval: 15,
    })
  })

  it('does not serialize unrelated runtime properties in a general settings update', async () => {
    const store = useConfigStore()
    const request = {
      useWebsocket: false,
      tickerPollInterval: 15,
      activeSymbol: 'STALE-SHOULD-NOT-SEND',
    }
    vi.mocked(tauriInvoke).mockResolvedValueOnce({
      useWebsocket: false,
      tickerPollInterval: 15,
    })

    await store.updateGeneralSettings(request)

    expect(tauriInvoke).toHaveBeenCalledWith('update_general_settings', {
      request: { useWebsocket: false, tickerPollInterval: 15 },
    })
  })

  it('keeps committed general settings when an older full config fetch finishes', async () => {
    const store = useConfigStore()
    const initial = makeConfig({ windowWidth: 1555 })
    const pendingFetch = deferred<AppConfig>()
    store.config = initial
    vi.mocked(tauriInvoke)
      .mockReturnValueOnce(pendingFetch.promise)
      .mockResolvedValueOnce({ useWebsocket: false, tickerPollInterval: 15 })

    const oldFetch = store.fetchConfig()
    await store.updateGeneralSettings({ useWebsocket: false, tickerPollInterval: 15 })

    pendingFetch.resolve(makeConfig({
      useWebsocket: true,
      tickerPollInterval: 999,
      windowWidth: 900,
    }))
    await oldFetch

    expect(store.config).toMatchObject({
      windowWidth: 1555,
      useWebsocket: false,
      tickerPollInterval: 15,
    })
  })
})

describe('general settings autosave', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('does not save initialization and coalesces rapid edits', async () => {
    vi.useFakeTimers()
    const save = vi.fn(async (value: GeneralSettings) => value)
    const autosave = useGeneralSettingsAutosave(save, vi.fn(async () => undefined), 300)

    autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
    autosave.update({ tickerPollInterval: 2 })
    autosave.update({ tickerPollInterval: 3 })
    await vi.advanceTimersByTimeAsync(299)
    expect(save).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1)
    await flushPromises()

    expect(save).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenLastCalledWith({ useWebsocket: true, tickerPollInterval: 3 })
    expect(autosave.status.value).toBe('saved')
  })

  it('queues only the latest complete draft after an in-flight save', async () => {
    vi.useFakeTimers()
    const firstSave = deferred<GeneralSettings>()
    const save = vi.fn<
      (value: GeneralSettings) => Promise<GeneralSettings>
    >()
      .mockReturnValueOnce(firstSave.promise)
      .mockResolvedValueOnce({ useWebsocket: false, tickerPollInterval: 3 })
    const autosave = useGeneralSettingsAutosave(save, vi.fn(async () => undefined), 300)

    autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
    autosave.update({ tickerPollInterval: 2 })
    await vi.advanceTimersByTimeAsync(300)
    expect(save).toHaveBeenLastCalledWith({ useWebsocket: true, tickerPollInterval: 2 })

    autosave.update({ useWebsocket: false, tickerPollInterval: 3 })
    await vi.advanceTimersByTimeAsync(300)
    expect(save).toHaveBeenCalledTimes(1)

    firstSave.resolve({ useWebsocket: true, tickerPollInterval: 2 })
    await flushPromises()

    expect(save).toHaveBeenCalledTimes(2)
    expect(save).toHaveBeenLastCalledWith({ useWebsocket: false, tickerPollInterval: 3 })
    expect(autosave.draft.value).toEqual({ useWebsocket: false, tickerPollInterval: 3 })
    expect(autosave.lastSaved.value).toEqual({ useWebsocket: false, tickerPollInterval: 3 })
    expect(autosave.status.value).toBe('saved')
  })

  it('pauses failed saves until retry after reconciling once', async () => {
    vi.useFakeTimers()
    const save = vi.fn<
      (value: GeneralSettings) => Promise<GeneralSettings>
    >()
      .mockRejectedValueOnce(new Error('save failed'))
      .mockResolvedValueOnce({ useWebsocket: true, tickerPollInterval: 2 })
    const reconcile = vi.fn(async () => undefined)
    const autosave = useGeneralSettingsAutosave(save, reconcile, 300)

    autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
    autosave.update({ tickerPollInterval: 2 })
    await vi.advanceTimersByTimeAsync(300)
    await flushPromises()

    expect(autosave.status.value).toBe('error')
    expect(autosave.error.value).toBe('save failed')
    expect(autosave.draft.value).toEqual({ useWebsocket: true, tickerPollInterval: 2 })
    expect(autosave.lastSaved.value).toEqual({ useWebsocket: true, tickerPollInterval: 1 })
    expect(reconcile).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(save).toHaveBeenCalledTimes(1)

    await autosave.retry()
    expect(save).toHaveBeenCalledTimes(2)
    expect(autosave.lastSaved.value).toEqual({ useWebsocket: true, tickerPollInterval: 2 })
    expect(autosave.status.value).toBe('saved')
  })

  it('flushes a debounced draft when disposed', async () => {
    vi.useFakeTimers()
    const save = vi.fn(async (value: GeneralSettings) => value)
    const autosave = useGeneralSettingsAutosave(save, vi.fn(async () => undefined), 300)

    autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
    autosave.update({ tickerPollInterval: 2 })
    await autosave.dispose()

    expect(save).toHaveBeenCalledTimes(1)
    expect(save).toHaveBeenLastCalledWith({ useWebsocket: true, tickerPollInterval: 2 })
    expect(autosave.status.value).toBe('saved')
  })

  it('rejects disposal after reconciling a final failed save', async () => {
    vi.useFakeTimers()
    const save = vi.fn<
      (value: GeneralSettings) => Promise<GeneralSettings>
    >().mockRejectedValueOnce(new Error('final save failed'))
    const reconcile = vi.fn(async () => undefined)
    const autosave = useGeneralSettingsAutosave(save, reconcile, 300)

    autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
    autosave.update({ tickerPollInterval: 2 })

    await expect(autosave.dispose()).rejects.toThrow('final save failed')
    expect(reconcile).toHaveBeenCalledTimes(1)
    expect(autosave.status.value).toBe('error')
  })
})
