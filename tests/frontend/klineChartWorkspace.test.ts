import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { nextTick } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import KlineChart from '../../src/components/market/KlineChart.vue'
import { chartWorkspaceAutosaveKey } from '../../src/composables/useChartWorkspaceAutosaveHost'
import { useMarketStore } from '../../src/stores/market'
import type { ChartWorkspaceSnapshot } from '../../src/types/chartWorkspace'
import { defaultChartPreferences, defaultChartViewState } from '../../src/utils/chartWorkspace'

const deferred = <T,>() => {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

const mocks = vi.hoisted(() => ({
  proOptions: [] as any[],
  proInstances: [] as any[],
  proConstruct: vi.fn(),
  datafeeds: [] as any[],
  primeHistory: vi.fn(),
  pushRealtimeBar: vi.fn(),
  expectProgrammaticContext: vi.fn(),
  waitForExpectedContext: vi.fn().mockResolvedValue(true),
  recover: vi.fn(),
  createAdapter: vi.fn(),
  adapter: {
    attach: vi.fn(),
    detach: vi.fn(),
    prepareViewReplacement: vi.fn().mockResolvedValue(undefined),
    applyPrimedHistoryFallback: vi.fn(),
    applyRefreshedHistory: vi.fn().mockResolvedValue(undefined),
    cancelGeneration: vi.fn(),
    capturePreferences: vi.fn(),
    captureViewState: vi.fn(),
    resize: vi.fn(),
  },
  migrate: vi.fn().mockResolvedValue(undefined),
  load: vi.fn(),
  report: vi.fn(),
  invoke: vi.fn().mockResolvedValue([]),
}))

vi.mock('@klinecharts/pro', () => ({
  KLineChartPro: class {
    setSymbol = vi.fn()
    setPeriod = vi.fn()

    constructor(options: any) {
      mocks.proConstruct(options)
      mocks.proOptions.push(options)
      mocks.proInstances.push(this)
    }
  },
}))

vi.mock('../../src/composables/easiKlineDatafeed', () => ({
  EasiKlineDatafeed: class {
    primeHistory = mocks.primeHistory
    pushRealtimeBar = mocks.pushRealtimeBar
    expectProgrammaticContext = mocks.expectProgrammaticContext
    waitForExpectedContext = mocks.waitForExpectedContext
    onContextChange: (key: any) => Promise<void>
    onHistoryRefresh: (result: any) => void

    constructor(
      _getSymbols: () => string[],
      _getContext: () => any,
      onContextChange: (key: any) => Promise<void>,
      onHistoryRefresh: (result: any) => void,
    ) {
      this.onContextChange = onContextChange
      this.onHistoryRefresh = onHistoryRefresh
      mocks.datafeeds.push(this)
    }
  },
}))

vi.mock('../../src/services/klineChartsCoreBridge', () => ({
  recoverKLineChartsCore: mocks.recover,
}))

vi.mock('../../src/services/klineChartsWorkspaceAdapter', () => ({
  createKLineChartsWorkspaceAdapter: mocks.createAdapter,
}))

vi.mock('../../src/services/legacyChartSettingsMigration', () => ({
  migrateLegacyChartSettingsOnce: mocks.migrate,
}))

vi.mock('../../src/services/chartWorkspaceService', () => ({
  loadChartWorkspace: mocks.load,
}))

vi.mock('../../src/services/errorService', () => ({ reportError: mocks.report }))
vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: mocks.invoke }))

function snapshot(
  symbol = 'BTCUSDT',
  interval = '15',
  revision = 2,
): ChartWorkspaceSnapshot {
  const key = { symbol, interval }
  return {
    key,
    klines: [{
      symbol,
      interval,
      openTime: 1_000,
      open: '1',
      high: '2',
      low: '0.5',
      close: '1.5',
      volume: '10',
    }],
    viewState: {
      ...defaultChartViewState(key),
      revision,
      overlays: [],
      viewport: { barSpace: 9, rightTimestamp: 1_000 },
    },
    preferences: {
      ...defaultChartPreferences(),
      revision,
      mainIndicators: ['EMA'],
      subIndicators: ['RSI'],
    },
  }
}

function fakeAutosave() {
  const handles: any[] = []
  const controller = {
    adoptSnapshot: vi.fn(),
    registerCaptureSource: vi.fn(() => {
      const handle = {
        setActive: vi.fn(),
        setCaptureEnabled: vi.fn(),
        markViewDirty: vi.fn(),
        markPreferencesDirty: vi.fn(),
        unregister: vi.fn(),
      }
      handles.push(handle)
      return handle
    }),
    handles,
  }
  return controller
}

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = []
  observe = vi.fn()
  disconnect = vi.fn()

  constructor(readonly callback: ResizeObserverCallback) {
    FakeResizeObserver.instances.push(this)
  }
}

describe('KlineChart workspace integration', () => {
  let pinia: Pinia
  let autosave: ReturnType<typeof fakeAutosave>

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    const market = useMarketStore()
    market.activeSymbol = 'BTCUSDT'
    market.klineInterval = '15'
    market.symbols = ['BTCUSDT', 'ETHUSDT', 'SOLUSDT']
    autosave = fakeAutosave()
    FakeResizeObserver.instances = []
    vi.stubGlobal('ResizeObserver', FakeResizeObserver)
    mocks.proOptions.length = 0
    mocks.proInstances.length = 0
    mocks.datafeeds.length = 0
    mocks.proConstruct.mockClear()
    mocks.primeHistory.mockClear()
    mocks.pushRealtimeBar.mockClear()
    mocks.expectProgrammaticContext.mockClear()
    mocks.waitForExpectedContext.mockReset().mockResolvedValue(true)
    mocks.recover.mockReset().mockReturnValue({ ok: true, chart: {} })
    for (const method of Object.values(mocks.adapter)) {
      if ('mockReset' in method) (method as any).mockReset()
    }
    mocks.adapter.prepareViewReplacement.mockResolvedValue(undefined)
    mocks.adapter.applyRefreshedHistory.mockResolvedValue(undefined)
    mocks.createAdapter.mockReset().mockReturnValue(mocks.adapter)
    mocks.migrate.mockReset().mockResolvedValue(undefined)
    mocks.load.mockReset().mockResolvedValue(snapshot())
    mocks.report.mockReset()
    mocks.invoke.mockReset().mockResolvedValue([])
  })

  function mountChart(
    props: { mode?: 'trading' | 'workspace'; active?: boolean } = {},
    controller = autosave,
  ): VueWrapper {
    return mount(KlineChart, {
      props,
      global: {
        plugins: [pinia],
        provide: { [chartWorkspaceAutosaveKey as symbol]: controller },
      },
    })
  }

  async function initialized(wrapper: VueWrapper): Promise<void> {
    await vi.waitFor(() => expect(mocks.proConstruct).toHaveBeenCalled())
    await flushPromises()
    expect(wrapper.find('.chart-view').exists()).toBe(true)
  }

  it('fixes drawing bar visibility by mode and removes the external settings toolbar', async () => {
    const workspace = mountChart({ mode: 'workspace', active: true })
    await initialized(workspace)
    expect(mocks.proOptions[0].drawingBarVisible).toBe(true)
    expect(workspace.find('.chart-toolbar').exists()).toBe(false)
    workspace.unmount()

    const trading = mountChart({ mode: 'trading', active: true })
    await initialized(trading)
    expect(mocks.proOptions[1].drawingBarVisible).toBe(false)
    expect(trading.find('.chart-toolbar').exists()).toBe(false)
  })

  it('creates one Pro instance while active state and shared context change', async () => {
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)
    const market = useMarketStore()

    await wrapper.setProps({ active: false })
    market.activeSymbol = 'ETHUSDT'
    market.klineInterval = '60'
    await nextTick()
    await wrapper.setProps({ active: true })
    await flushPromises()

    expect(mocks.proConstruct).toHaveBeenCalledOnce()
  })

  it('primes local history before Pro construction and gates capture on persisted restore', async () => {
    const restored = deferred<void>()
    mocks.adapter.prepareViewReplacement.mockReturnValueOnce(restored.promise)
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)

    expect(mocks.primeHistory).toHaveBeenCalledWith(
      { symbol: 'BTCUSDT', interval: '15' },
      expect.any(Number),
      snapshot().klines,
    )
    expect(mocks.primeHistory.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.proConstruct.mock.invocationCallOrder[0]!,
    )
    expect(mocks.adapter.attach).toHaveBeenCalledBefore(
      mocks.adapter.prepareViewReplacement,
    )
    expect(autosave.handles[0].setCaptureEnabled).toHaveBeenLastCalledWith(false)

    restored.resolve()
    await flushPromises()
    expect(autosave.handles[0].setCaptureEnabled).toHaveBeenLastCalledWith(true)
  })

  it('timestamp-merges early REST refreshes and applies them after persisted restore', async () => {
    const restored = deferred<void>()
    mocks.adapter.prepareViewReplacement.mockReturnValueOnce(restored.promise)
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)
    const refresh = mocks.datafeeds[0].onHistoryRefresh

    refresh({
      key: { symbol: 'BTCUSDT', interval: '15' }, generation: 2, range: {},
      klines: [{ timestamp: 2_000, close: 2 }, { timestamp: 3_000, close: 3 }],
    })
    refresh({
      key: { symbol: 'BTCUSDT', interval: '15' }, generation: 1, range: {},
      klines: [{ timestamp: 1_000, close: 1 }, { timestamp: 2_000, close: 22 }],
    })
    expect(mocks.adapter.applyRefreshedHistory).not.toHaveBeenCalled()

    restored.resolve()
    await flushPromises()
    expect(mocks.adapter.applyRefreshedHistory).toHaveBeenCalledExactlyOnceWith([
      expect.objectContaining({ timestamp: 1_000, close: 1 }),
      expect.objectContaining({ timestamp: 2_000, close: 22 }),
      expect.objectContaining({ timestamp: 3_000, close: 3 }),
    ], expect.any(Number))
  })

  it('reports one initial load failure but constructs Pro with version-one defaults', async () => {
    mocks.load.mockRejectedValue(new Error('offline'))
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)

    expect(mocks.report).toHaveBeenCalledOnce()
    expect(mocks.proConstruct).toHaveBeenCalledOnce()
    expect(mocks.primeHistory).toHaveBeenCalledWith(
      { symbol: 'BTCUSDT', interval: '15' },
      expect.any(Number),
      [],
    )
    expect(mocks.proOptions[0]).toMatchObject({
      mainIndicators: ['MA', 'EMA'],
      subIndicators: ['VOL', 'MACD'],
    })
  })

  it('activation replaces the hidden view, falls back after an unmatched context, then resumes capture', async () => {
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)
    const market = useMarketStore()
    mocks.load.mockResolvedValue(snapshot('ETHUSDT', '60', 4))
    mocks.waitForExpectedContext.mockResolvedValueOnce(false)

    await wrapper.setProps({ active: false })
    market.activeSymbol = 'ETHUSDT'
    market.klineInterval = '60'
    await nextTick()
    await wrapper.setProps({ active: true })
    await vi.waitFor(() => expect(mocks.adapter.applyPrimedHistoryFallback).toHaveBeenCalled())
    await flushPromises()

    expect(mocks.adapter.prepareViewReplacement).toHaveBeenLastCalledWith(
      expect.objectContaining({ revision: 4 }),
      expect.objectContaining({ symbol: 'ETHUSDT', interval: '60', revision: 4 }),
      expect.any(Number),
    )
    expect(mocks.expectProgrammaticContext).toHaveBeenCalledWith(
      { symbol: 'ETHUSDT', interval: '60' },
      expect.any(Number),
    )
    expect(mocks.proInstances[0].setSymbol).toHaveBeenCalledOnce()
    expect(mocks.proInstances[0].setPeriod).toHaveBeenCalledOnce()
    expect(autosave.handles[0].setCaptureEnabled).toHaveBeenLastCalledWith(true)
  })

  it('serializes selector and watcher echoes and applies only the latest key', async () => {
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)
    mocks.load.mockClear()
    mocks.load.mockImplementation(async (key) => snapshot(key.symbol, key.interval))

    const selector = mocks.datafeeds[0].onContextChange
    const selectB = selector({ symbol: 'ETHUSDT', interval: '60' })
    const selectC = selector({ symbol: 'SOLUSDT', interval: '240' })
    await Promise.all([selectB, selectC])
    await flushPromises()

    expect(mocks.load.mock.calls.map(([key]) => key)).toEqual([
      { symbol: 'SOLUSDT', interval: '240' },
    ])
    expect(useMarketStore().activeSymbol).toBe('SOLUSDT')
    expect(mocks.adapter.prepareViewReplacement).toHaveBeenCalledTimes(2)
  })

  it('degrades one bridge compatibility failure without rebuilding Pro or losing autosave registration', async () => {
    mocks.recover.mockReturnValue({ ok: false, code: 'shape', message: 'bad runtime' })
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)

    expect(mocks.report).toHaveBeenCalledOnce()
    expect(mocks.proConstruct).toHaveBeenCalledOnce()
    expect(mocks.createAdapter).not.toHaveBeenCalled()
    expect(autosave.registerCaptureSource).toHaveBeenCalledOnce()
    expect(autosave.handles[0].setActive).toHaveBeenCalledWith(true)
  })

  it('shares the provided autosave controller and only enables the active source', async () => {
    const first = mountChart({ mode: 'workspace', active: true })
    const second = mountChart({ mode: 'trading', active: false })
    await vi.waitFor(() => expect(mocks.proConstruct).toHaveBeenCalledTimes(2))
    await flushPromises()

    expect(autosave.registerCaptureSource).toHaveBeenCalledTimes(2)
    expect(autosave.handles[0].setActive).toHaveBeenLastCalledWith(true)
    expect(autosave.handles[1].setActive).toHaveBeenLastCalledWith(false)
    expect(autosave.handles[1].setCaptureEnabled).toHaveBeenLastCalledWith(false)
    first.unmount()
    second.unmount()
  })

  it('resizes from observation and reactivation, disconnecting only on final unmount', async () => {
    const wrapper = mountChart({ mode: 'workspace', active: true })
    await initialized(wrapper)
    const observer = FakeResizeObserver.instances[0]!
    expect(observer.observe).toHaveBeenCalledWith(wrapper.get('.chart-view').element)

    observer.callback([], observer as unknown as ResizeObserver)
    expect(mocks.adapter.resize).toHaveBeenCalled()
    const beforeActivation = mocks.adapter.resize.mock.calls.length
    await wrapper.setProps({ active: false })
    expect(observer.disconnect).not.toHaveBeenCalled()
    await wrapper.setProps({ active: true })
    await nextTick()
    expect(mocks.adapter.resize.mock.calls.length).toBeGreaterThan(beforeActivation)

    wrapper.unmount()
    expect(observer.disconnect).toHaveBeenCalledOnce()
    expect(mocks.adapter.detach).toHaveBeenCalledOnce()
  })
})
