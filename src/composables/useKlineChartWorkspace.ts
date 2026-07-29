import { KLineChartPro } from '@klinecharts/pro'
import type { KLineData } from 'klinecharts'
import { storeToRefs } from 'pinia'
import {
  nextTick,
  onMounted,
  onUnmounted,
  watch,
  type Ref,
} from 'vue'
import { useMarketStore } from '../stores/market'
import type {
  ChartWorkspaceKey,
  ChartWorkspaceSnapshot,
} from '../types/chartWorkspace'
import {
  chartPreferencesContentFingerprint,
  chartViewContentFingerprint,
  defaultChartPreferences,
  defaultChartViewState,
  normalizeChartWorkspaceKey,
} from '../utils/chartWorkspace'
import {
  intervalToPeriod,
  KLINE_PERIODS,
  toKLineData,
  toSymbolInfo,
} from '../utils/klinecharts'
import { EasiKlineDatafeed, type ChartHistoryRefreshResult } from './easiKlineDatafeed'
import { useChartWorkspaceAutosave } from './useChartWorkspaceAutosaveHost'
import { loadChartWorkspace } from '../services/chartWorkspaceService'
import { reportError } from '../services/errorService'
import { recoverKLineChartsCore } from '../services/klineChartsCoreBridge'
import {
  createKLineChartsWorkspaceAdapter,
  type KLineChartsWorkspaceAdapter,
} from '../services/klineChartsWorkspaceAdapter'
import type { RegisteredChartWorkspaceCaptureSource } from '../services/chartWorkspaceAutosave'
import { migrateLegacyChartSettingsOnce } from '../services/legacyChartSettingsMigration'

export interface KlineChartWorkspaceProps {
  mode: 'trading' | 'workspace'
  active: boolean
}

type TransitionReason = 'selector' | 'store' | 'activation'

function sameKey(first: ChartWorkspaceKey, second: ChartWorkspaceKey): boolean {
  return first.symbol === second.symbol && first.interval === second.interval
}

function fallbackSnapshot(key: ChartWorkspaceKey): ChartWorkspaceSnapshot {
  return {
    key,
    klines: [],
    viewState: defaultChartViewState(key),
    preferences: defaultChartPreferences(),
  }
}

export function useKlineChartWorkspace(
  chartContainer: Ref<HTMLDivElement | null>,
  props: Readonly<KlineChartWorkspaceProps>,
): void {
  const marketStore = useMarketStore()
  const { activeSymbol, klineInterval, klines, symbols } = storeToRefs(marketStore)
  const autosave = useChartWorkspaceAutosave()
  const reported = new Set<string>()

  let desiredKey = currentStoreKey()
  let currentKey: ChartWorkspaceKey | null = null
  let generation = 0
  let activeGeneration = 0
  let restoreResolvedGeneration = 0
  let initialized = false
  let destroyed = false
  let transitionTail: Promise<void> = Promise.resolve()
  let pendingTransition: Promise<void> | null = null
  let pendingTransitionKey: ChartWorkspaceKey | null = null
  let chartPro: KLineChartPro | null = null
  let datafeed: EasiKlineDatafeed | null = null
  let adapter: KLineChartsWorkspaceAdapter | null = null
  let captureRegistration: RegisteredChartWorkspaceCaptureSource | null = null
  let resizeObserver: ResizeObserver | null = null
  let bufferedGeneration = 0
  const bufferedRefresh = new Map<number, KLineData>()

  function currentStoreKey(): ChartWorkspaceKey {
    return normalizeChartWorkspaceKey({
      symbol: activeSymbol.value,
      interval: klineInterval.value,
    })
  }

  function reportOnce(key: string, error: unknown, context: string): void {
    if (reported.has(key)) return
    reported.add(key)
    reportError(error, context)
  }

  async function loadSnapshot(
    key: ChartWorkspaceKey,
    token: number,
  ): Promise<ChartWorkspaceSnapshot | null> {
    try {
      const snapshot = await loadChartWorkspace(key)
      return token === generation && !destroyed ? snapshot : null
    } catch (error) {
      if (token !== generation || destroyed) return null
      reportOnce(
        `load:${key.symbol}:${key.interval}`,
        error,
        '加载本地图表工作区失败',
      )
      return fallbackSnapshot(key)
    }
  }

  function registerCaptureSource(): void {
    captureRegistration = autosave.registerCaptureSource({
      getActiveKey: () => currentKey ?? desiredKey,
      capture: {
        capture: (key, viewRevision, preferencesRevision) => {
          if (!adapter || !currentKey || !sameKey(key, currentKey)) return null
          const viewState = adapter.captureViewState(key, viewRevision)
          const preferences = adapter.capturePreferences(preferencesRevision)
          return {
            viewState,
            preferences,
            viewFingerprint: chartViewContentFingerprint(viewState),
            preferencesFingerprint: chartPreferencesContentFingerprint(preferences),
          }
        },
      },
    })
    captureRegistration.setCaptureEnabled(false)
    captureRegistration.setActive(props.active)
  }

  async function applyBufferedRefresh(token: number): Promise<void> {
    if (
      token !== generation
      || token !== activeGeneration
      || bufferedGeneration !== token
      || !props.active
      || !adapter
      || bufferedRefresh.size === 0
    ) return
    const bars = [...bufferedRefresh.values()]
      .sort((first, second) => first.timestamp - second.timestamp)
    bufferedRefresh.clear()
    try {
      await adapter.applyRefreshedHistory(bars, token)
    } catch (error) {
      reportOnce(`refresh:${token}`, error, '应用图表历史刷新失败')
    }
  }

  function finishRestore(token: number, restore: Promise<void>): void {
    void restore.then(async () => {
      if (token !== generation || token !== activeGeneration || destroyed) return
      restoreResolvedGeneration = token
      await applyBufferedRefresh(token)
      if (token !== generation || token !== activeGeneration || destroyed) return
      captureRegistration?.setCaptureEnabled(props.active)
    }).catch((error: unknown) => {
      if (token !== generation || destroyed) return
      reportOnce(`restore:${token}`, error, '恢复图表工作区失败')
      captureRegistration?.setCaptureEnabled(props.active)
    })
  }

  function handleHistoryRefresh(result: ChartHistoryRefreshResult): void {
    let key: ChartWorkspaceKey
    try {
      key = normalizeChartWorkspaceKey(result.key)
    } catch {
      return
    }
    if (!initialized || destroyed || !props.active || !currentKey || !sameKey(key, currentKey)) {
      return
    }
    const token = activeGeneration
    if (token !== generation) return
    if (bufferedGeneration !== token) {
      bufferedGeneration = token
      bufferedRefresh.clear()
    }
    for (const bar of result.klines) {
      if (Number.isFinite(bar.timestamp)) bufferedRefresh.set(bar.timestamp, bar)
    }
    if (restoreResolvedGeneration === token) {
      void applyBufferedRefresh(token)
    }
  }

  function pushRealtimeFromStore(): void {
    if (!datafeed) return
    const bars = toKLineData(klines.value)
    datafeed.pushRealtimeBar(currentStoreKey(), bars[bars.length - 1] ?? null)
  }

  function installAdapter(snapshot: ChartWorkspaceSnapshot, token: number): Promise<void> | null {
    if (!chartContainer.value) return null
    const bridge = recoverKLineChartsCore(chartContainer.value)
    if (!bridge.ok) {
      reportOnce(`bridge:${bridge.code}`, new Error(bridge.message), '图表兼容层不可用')
      return null
    }
    adapter = createKLineChartsWorkspaceAdapter(bridge.chart, {
      onViewDirty: () => captureRegistration?.markViewDirty(),
      onPreferencesDirty: () => captureRegistration?.markPreferencesDirty(),
      reportDiagnostic: (message) => {
        reportOnce(`adapter:${message}`, new Error(message), '图表兼容性诊断')
      },
    })
    adapter.attach()
    return adapter.prepareViewReplacement(snapshot.preferences, snapshot.viewState, token)
  }

  async function initialize(): Promise<void> {
    const migrationKey = desiredKey
    try {
      await migrateLegacyChartSettingsOnce(migrationKey)
    } catch (error) {
      reportOnce('migration', error, '迁移旧图表设置失败')
    }
    if (destroyed) return

    let snapshot: ChartWorkspaceSnapshot | null = null
    let key = desiredKey
    let token = 0
    while (!snapshot && !destroyed) {
      key = desiredKey
      token = ++generation
      snapshot = await loadSnapshot(key, token)
      if (token !== generation || !sameKey(key, desiredKey)) snapshot = null
    }
    if (!snapshot || destroyed || !chartContainer.value) return

    currentKey = key
    activeGeneration = token
    bufferedGeneration = token
    datafeed = new EasiKlineDatafeed(
      () => symbols.value,
      () => currentKey ?? desiredKey,
      (next) => requestTransition(next, 'selector'),
      handleHistoryRefresh,
      (context, error) => reportError(error, context),
    )
    datafeed.primeHistory(key, token, snapshot.klines)
    chartPro = new KLineChartPro({
      container: chartContainer.value,
      theme: 'dark',
      locale: 'zh-CN',
      drawingBarVisible: props.mode === 'workspace',
      symbol: toSymbolInfo(key.symbol),
      period: intervalToPeriod(key.interval),
      periods: KLINE_PERIODS,
      mainIndicators: snapshot.preferences.mainIndicators,
      subIndicators: snapshot.preferences.subIndicators,
      datafeed,
    })

    const restore = installAdapter(snapshot, token)
    autosave.adoptSnapshot(snapshot)
    registerCaptureSource()
    initialized = true
    pushRealtimeFromStore()
    if (restore) {
      finishRestore(token, restore)
    } else {
      restoreResolvedGeneration = token
      captureRegistration?.setCaptureEnabled(props.active)
    }
  }

  async function performTransition(
    key: ChartWorkspaceKey,
    token: number,
    reason: TransitionReason,
  ): Promise<void> {
    if (token !== generation || destroyed || !initialized || !props.active) return

    if (reason === 'selector') {
      await marketStore.setChartContext(key)
      if (token !== generation || destroyed || !props.active) return
    }

    captureRegistration?.setCaptureEnabled(false)
    const snapshot = await loadSnapshot(key, token)
    if (!snapshot || token !== generation || destroyed || !props.active || !datafeed) return

    const previousKey = currentKey
    adapter?.cancelGeneration(activeGeneration)
    currentKey = key
    activeGeneration = token
    restoreResolvedGeneration = 0
    bufferedGeneration = token
    bufferedRefresh.clear()
    datafeed.primeHistory(key, token, snapshot.klines)
    autosave.adoptSnapshot(snapshot)

    if (!adapter) {
      restoreResolvedGeneration = token
      captureRegistration?.setCaptureEnabled(true)
      return
    }

    const restore = adapter.prepareViewReplacement(
      snapshot.preferences,
      snapshot.viewState,
      token,
    )
    finishRestore(token, restore)
    if (reason === 'selector') return

    datafeed.expectProgrammaticContext(key, token)
    let usedSetter = false
    if (!previousKey || previousKey.symbol !== key.symbol) {
      chartPro?.setSymbol(toSymbolInfo(key.symbol))
      usedSetter = true
    }
    if (!previousKey || previousKey.interval !== key.interval) {
      chartPro?.setPeriod(intervalToPeriod(key.interval))
      usedSetter = true
    }

    let matched = false
    if (usedSetter) {
      matched = await datafeed.waitForExpectedContext(key, token, 250)
      if (token !== generation || destroyed || !props.active) return
    }
    if (!matched) {
      adapter.applyPrimedHistoryFallback(toKLineData(snapshot.klines), token)
    }
  }

  function requestTransition(
    next: ChartWorkspaceKey,
    reason: TransitionReason,
  ): Promise<void> {
    const key = normalizeChartWorkspaceKey(next)
    desiredKey = key
    if (pendingTransition && pendingTransitionKey && sameKey(pendingTransitionKey, key)) {
      return pendingTransition
    }
    const token = ++generation
    const transition = transitionTail.then(() => performTransition(key, token, reason))
    transitionTail = transition.catch(() => undefined)
    pendingTransition = transition.finally(() => {
      if (token === generation) {
        pendingTransition = null
        pendingTransitionKey = null
      }
    })
    pendingTransitionKey = key
    return pendingTransition
  }

  watch([activeSymbol, klineInterval], () => {
    const key = currentStoreKey()
    desiredKey = key
    if (!initialized) {
      generation += 1
      return
    }
    if (pendingTransitionKey && sameKey(pendingTransitionKey, key)) return
    if (!props.active) {
      const token = ++generation
      adapter?.cancelGeneration(activeGeneration)
      const recorded = transitionTail.then(() => {
        if (token !== generation || destroyed) return
      })
      transitionTail = recorded.catch(() => undefined)
      return
    }
    void requestTransition(key, 'store')
  })

  watch(klines, pushRealtimeFromStore)

  watch(() => props.active, (active) => {
    captureRegistration?.setActive(active)
    captureRegistration?.setCaptureEnabled(false)
    if (!active) {
      desiredKey = currentStoreKey()
      generation += 1
      adapter?.cancelGeneration(activeGeneration)
      return
    }
    void nextTick().then(() => adapter?.resize())
    if (initialized) void requestTransition(currentStoreKey(), 'activation')
  })

  onMounted(() => {
    if (chartContainer.value && typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => adapter?.resize())
      resizeObserver.observe(chartContainer.value)
    }
    void initialize()
  })

  onUnmounted(() => {
    destroyed = true
    generation += 1
    adapter?.cancelGeneration(activeGeneration)
    captureRegistration?.setCaptureEnabled(false)
    captureRegistration?.setActive(false)
    captureRegistration?.unregister()
    adapter?.detach()
    resizeObserver?.disconnect()
    resizeObserver = null
    chartPro = null
    datafeed = null
  })
}
