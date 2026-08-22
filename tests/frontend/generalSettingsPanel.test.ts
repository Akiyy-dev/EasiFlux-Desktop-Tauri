import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { NInputNumber, NSwitch } from 'naive-ui'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import GeneralSettingsPanel from '../../src/components/settings/GeneralSettingsPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { reportError } from '../../src/services/errorService'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import type { AppConfig, GeneralSettings } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))
vi.mock('../../src/services/errorService', () => ({
  reportError: vi.fn((error: unknown, context?: string) => {
    const detail = error instanceof Error ? error.message : String(error)
    return context ? `${context}: ${detail}` : detail
  }),
}))

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
    tickerPollInterval: 10,
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

function installSuccessfulBackend(config = makeConfig()): void {
  vi.mocked(tauriInvoke).mockImplementation((command, args) => {
    if (command === 'get_config') return Promise.resolve(config)
    if (command === 'update_general_settings') {
      return Promise.resolve({ ...(args?.request as GeneralSettings) })
    }
    if (command === 'get_connection_status') return Promise.resolve('connected')
    return Promise.resolve(undefined)
  })
}

function mountPanel(config: AppConfig | null = makeConfig()): VueWrapper {
  useConfigStore().config = config
  return mount(GeneralSettingsPanel, { global: { plugins: [pinia] } })
}

async function setWebsocket(wrapper: VueWrapper, value: boolean): Promise<void> {
  wrapper.getComponent(NSwitch).vm.$emit('update:value', value)
  await flushPromises()
}

async function setInterval(wrapper: VueWrapper, value: number | null): Promise<void> {
  wrapper.getComponent(NInputNumber).vm.$emit('update:value', value)
  await flushPromises()
}

async function finishDebounce(): Promise<void> {
  await vi.advanceTimersByTimeAsync(300)
  await flushPromises()
}

let pinia: Pinia

describe('GeneralSettingsPanel', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(reportError).mockClear()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('initializes controls from config without saving defaults', async () => {
    installSuccessfulBackend()
    const wrapper = mountPanel(makeConfig({ useWebsocket: false, tickerPollInterval: 27 }))
    await flushPromises()

    expect(wrapper.getComponent(NSwitch).props('value')).toBe(false)
    expect(wrapper.getComponent(NInputNumber).props('value')).toBe(27)
    expect(tauriInvoke).not.toHaveBeenCalledWith(
      'update_general_settings',
      expect.anything(),
    )
  })

  it('coalesces rapid interval edits and announces the saved state politely', async () => {
    installSuccessfulBackend()
    const wrapper = mountPanel()
    await flushPromises()

    await setInterval(wrapper, 11)
    await setInterval(wrapper, 12)
    await finishDebounce()

    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(tauriInvoke).toHaveBeenCalledWith('update_general_settings', {
      request: { useWebsocket: true, tickerPollInterval: 12 },
    })
    expect(wrapper.get('[role="status"]').attributes('aria-live')).toBe('polite')
    expect(wrapper.get('[role="status"]').text()).toContain('已保存')
  })

  it('rejects invalid intervals and saves both inclusive boundaries', async () => {
    installSuccessfulBackend()
    const wrapper = mountPanel()
    await flushPromises()

    for (const value of [null, 0, -1, Number.NaN, Number.POSITIVE_INFINITY, 3601]) {
      await setInterval(wrapper, value)
      await finishDebounce()
      expect(wrapper.get('[role="alert"]').text()).toContain('1')
      expect(wrapper.get('[role="alert"]').text()).toContain('3600')
      expect(tauriInvoke).not.toHaveBeenCalledWith(
        'update_general_settings',
        expect.anything(),
      )
    }

    await setInterval(wrapper, 1)
    await finishDebounce()
    await setInterval(wrapper, 3600)
    await finishDebounce()

    const saves = vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))
    expect(saves).toEqual([
      ['update_general_settings', {
        request: { useWebsocket: true, tickerPollInterval: 1 },
      }],
      ['update_general_settings', {
        request: { useWebsocket: true, tickerPollInterval: 3600 },
      }],
    ])
  })

  it('persists websocket changes while connected without reconnecting automatically', async () => {
    installSuccessfulBackend()
    useConnectionStore().setStatus('connected')
    const wrapper = mountPanel()
    await flushPromises()

    await setWebsocket(wrapper, false)
    await finishDebounce()

    expect(tauriInvoke).toHaveBeenCalledWith('update_general_settings', {
      request: { useWebsocket: false, tickerPollInterval: 10 },
    })
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'disconnect'))
      .toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls.some(([command]) => command === 'connect'))
      .toBe(false)
  })

  it('offers explicit reconnect only after save and uses the committed preference', async () => {
    installSuccessfulBackend()
    useConnectionStore().setStatus('connected')
    const wrapper = mountPanel()
    await flushPromises()

    await setWebsocket(wrapper, false)
    expect(wrapper.find('[data-testid="general-reconnect"]').exists()).toBe(false)
    await finishDebounce()
    expect(wrapper.text()).toContain('重连后生效')

    await wrapper.get('[data-testid="general-reconnect"]').trigger('click')
    await flushPromises()

    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(wrapper.find('[data-testid="general-reconnect"]').exists()).toBe(false)
  })

  it('retains a failed draft and exposes retry without reconnecting it', async () => {
    const retrySave = deferred<GeneralSettings>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'update_general_settings') {
        return Promise.reject(new Error('settings save failed'))
      }
      if (command === 'get_config') return Promise.resolve(makeConfig())
      return Promise.resolve(undefined)
    })
    useConnectionStore().setStatus('connected')
    const wrapper = mountPanel()
    await flushPromises()

    await setWebsocket(wrapper, false)
    await finishDebounce()

    expect(wrapper.getComponent(NSwitch).props('value')).toBe(false)
    expect(wrapper.get('[role="alert"]').text()).toContain('settings save failed')
    expect(wrapper.find('[data-testid="general-reconnect"]').exists()).toBe(false)

    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'update_general_settings') return retrySave.promise
      if (command === 'get_config') return Promise.resolve(makeConfig())
      return Promise.resolve(undefined)
    })
    await wrapper.get('[data-testid="general-settings-save-retry"]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain('settings save failed')

    retrySave.resolve({ useWebsocket: false, tickerPollInterval: 10 })
    await flushPromises()
    expect(wrapper.text()).not.toContain('settings save failed')
    expect(wrapper.getComponent(NSwitch).props('value')).toBe(false)
    expect(wrapper.get('[data-testid="general-reconnect"]').exists()).toBe(true)
  })

  it('keeps reconnect failures separate and retries without saving again', async () => {
    let connectAttempts = 0
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'update_general_settings') {
        return Promise.resolve({ ...(args?.request as GeneralSettings) })
      }
      if (command === 'connect') {
        connectAttempts += 1
        if (connectAttempts === 1) return Promise.reject(new Error('reconnect failed'))
        return Promise.resolve(undefined)
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    useConnectionStore().setStatus('connected')
    const wrapper = mountPanel()
    await flushPromises()
    await setWebsocket(wrapper, false)
    await finishDebounce()

    await expect(
      wrapper.get('[data-testid="general-reconnect"]').trigger('click'),
    ).resolves.toBeUndefined()
    await flushPromises()
    expect(wrapper.get('[data-testid="general-reconnect-error"]').text())
      .toContain('reconnect failed')
    expect(wrapper.find('[data-testid="general-settings-save-retry"]').exists()).toBe(false)
    expect(wrapper.get('[data-testid="general-reconnect"]').exists()).toBe(true)

    await wrapper.get('[data-testid="general-reconnect"]').trigger('click')
    await flushPromises()

    const commands = vi.mocked(tauriInvoke).mock.calls
      .map(([command]) => command)
      .filter((command) => command === 'disconnect' || command === 'connect')
    expect(commands).toEqual(['disconnect', 'connect', 'disconnect', 'connect'])
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))).toHaveLength(1)
  })

  it('reconnects committed mode B while a failed mode A draft remains retryable', async () => {
    let saveAttempts = 0
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'update_general_settings') {
        saveAttempts += 1
        if (saveAttempts === 2) return Promise.reject(new Error('draft A failed'))
        return Promise.resolve({ ...(args?.request as GeneralSettings) })
      }
      if (command === 'get_config') {
        return Promise.resolve(makeConfig({ useWebsocket: false }))
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    useConnectionStore().setStatus('connected')
    const wrapper = mountPanel()
    await flushPromises()

    await setWebsocket(wrapper, false)
    await finishDebounce()
    await setWebsocket(wrapper, true)
    await finishDebounce()

    expect(wrapper.getComponent(NSwitch).props('value')).toBe(true)
    expect(wrapper.text()).toContain('draft A failed')
    expect(wrapper.get('[data-testid="general-reconnect"]').exists()).toBe(true)
    expect(wrapper.get('[data-testid="general-settings-save-retry"]').exists()).toBe(true)

    await wrapper.get('[data-testid="general-reconnect"]').trigger('click')
    await flushPromises()

    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
    expect(wrapper.getComponent(NSwitch).props('value')).toBe(true)
    expect(wrapper.get('[data-testid="general-settings-save-retry"]').exists()).toBe(true)
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))).toHaveLength(2)
  })

  it('does not retain reconnect work across disconnected or natural connection states', async () => {
    installSuccessfulBackend()
    const connectionStore = useConnectionStore()
    const wrapper = mountPanel()
    await flushPromises()

    await setWebsocket(wrapper, false)
    await finishDebounce()
    expect(wrapper.find('[data-testid="general-reconnect"]').exists()).toBe(false)

    connectionStore.setStatus('connected')
    await flushPromises()
    expect(wrapper.find('[data-testid="general-reconnect"]').exists()).toBe(false)
  })

  it('waits for unknown config and initializes a deferred fetch without saving', async () => {
    const load = deferred<AppConfig>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_config') return load.promise
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel(null)
    await flushPromises()

    expect(wrapper.getComponent(NSwitch).props('disabled')).toBe(true)
    expect(wrapper.getComponent(NInputNumber).props('disabled')).toBe(true)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))).toHaveLength(0)

    load.resolve(makeConfig({ useWebsocket: false, tickerPollInterval: 45 }))
    await flushPromises()
    expect(wrapper.getComponent(NSwitch).props('disabled')).toBe(false)
    expect(wrapper.getComponent(NSwitch).props('value')).toBe(false)
    expect(wrapper.getComponent(NInputNumber).props('value')).toBe(45)
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))).toHaveLength(0)
  })

  it('renders a load alert and initializes once after explicit retry', async () => {
    vi.mocked(tauriInvoke)
      .mockRejectedValueOnce(new Error('config load failed'))
      .mockResolvedValueOnce(makeConfig({ tickerPollInterval: 60 }))
    const wrapper = mountPanel(null)
    await flushPromises()

    expect(wrapper.get('[role="alert"]').text()).toContain('config load failed')
    expect(wrapper.getComponent(NSwitch).props('disabled')).toBe(true)

    await wrapper.get('[data-testid="general-settings-retry"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="general-settings-retry"]').exists()).toBe(false)
    expect(wrapper.getComponent(NInputNumber).props('value')).toBe(60)
    expect(wrapper.getComponent(NInputNumber).props('disabled')).toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls.filter(([command]) => (
      command === 'update_general_settings'
    ))).toHaveLength(0)
  })

  it('reports a debounced save failure when unmounted', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'update_general_settings') {
        return Promise.reject(new Error('final settings failure'))
      }
      if (command === 'get_config') return Promise.resolve(makeConfig())
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await flushPromises()
    await setInterval(wrapper, 20)

    wrapper.unmount()
    await flushPromises()

    expect(reportError).toHaveBeenCalledWith(
      expect.objectContaining({ message: 'final settings failure' }),
      '离开通用设置前保存失败',
    )
  })
})
