import {
  ActionType,
  type ActionCallback,
  type Indicator,
  type IndicatorCreate,
  type KLineData,
  type Overlay,
  type OverlayCreate,
  type OverlayEventCallback,
  type OverlayRemove,
} from 'klinecharts'
import type {
  ChartOverlaySnapshot,
  ChartPaneRef,
  ChartPointSnapshot,
  ChartPreferencesV1,
  ChartViewStateV1,
  ChartViewportSnapshot,
  ChartWorkspaceKey,
} from '../types/chartWorkspace'
import { normalizeChartWorkspaceKey, toJsonValue } from '../utils/chartWorkspace'
import type { WorkspaceCoreChart } from './klineChartsCoreBridge'

export interface KLineChartsWorkspaceAdapter {
  attach(): void
  detach(): void
  prepareViewReplacement(
    preferences: ChartPreferencesV1,
    viewState: ChartViewStateV1,
    generation: number,
  ): Promise<void>
  applyPrimedHistoryFallback(data: KLineData[], generation: number): void
  applyRefreshedHistory(data: KLineData[], generation: number): Promise<void>
  cancelGeneration(generation: number): void
  capturePreferences(revision: number): ChartPreferencesV1
  captureViewState(key: ChartWorkspaceKey, revision: number): ChartViewStateV1
  resize(): void
}

export interface WorkspaceAdapterOptions {
  onViewDirty: () => void
  onPreferencesDirty: () => void
  reportDiagnostic: (message: string) => void
}

type CreateOverlay = WorkspaceCoreChart['createOverlay']
type OverrideOverlay = WorkspaceCoreChart['overrideOverlay']
type RemoveOverlay = WorkspaceCoreChart['removeOverlay']
type CreateIndicator = WorkspaceCoreChart['createIndicator']
type RemoveIndicator = WorkspaceCoreChart['removeIndicator']

const MIN_BAR_SPACE = 1
const MAX_BAR_SPACE = 50

interface PendingDataReady {
  generation: number
  reason: 'persisted' | 'refresh'
  viewState?: ChartViewStateV1
  viewport: ChartViewportSnapshot
  promise: Promise<void>
  resolve: () => void
}

function pendingDataReady(
  generation: number,
  reason: PendingDataReady['reason'],
  viewport: ChartViewportSnapshot,
  viewState?: ChartViewStateV1,
): PendingDataReady {
  let resolve = (): void => undefined
  const promise = new Promise<void>((done) => { resolve = done })
  return { generation, reason, viewState, viewport, promise, resolve }
}

function isIndicatorMap(value: unknown): value is Map<string, Indicator> {
  if (!(value instanceof Map)) return false
  return [...value].every(([name, item]) => (
    typeof name === 'string' && typeof item === 'object' && item !== null
      && typeof (item as Indicator).name === 'string'
  ))
}

function indicatorName(value: string | IndicatorCreate): string {
  return typeof value === 'string' ? value : value.name
}

function overlayIds(result: ReturnType<CreateOverlay>): string[] {
  if (typeof result === 'string') return [result]
  if (!Array.isArray(result)) return []
  return result.filter((id): id is string => typeof id === 'string')
}

function jsonPoints(points: Overlay['points']): ChartPointSnapshot[] {
  const sanitized = toJsonValue(points)
  return (Array.isArray(sanitized) ? sanitized : []) as unknown as ChartPointSnapshot[]
}

function indicatorSignature(panes: Map<string, Map<string, Indicator>>): string {
  return JSON.stringify([...panes].map(([paneId, indicators]) => [paneId, [...indicators.keys()]]))
}

function mergeRefreshedAndLive(
  refreshed: KLineData[],
  live: KLineData[],
): KLineData[] {
  const byTimestamp = new Map<number, KLineData>()
  for (const item of refreshed) {
    if (Number.isFinite(item.timestamp)) byTimestamp.set(item.timestamp, item)
  }
  for (const item of live) {
    if (Number.isFinite(item.timestamp)) byTimestamp.set(item.timestamp, item)
  }
  return [...byTimestamp.values()].sort((first, second) => first.timestamp - second.timestamp)
}

export function createKLineChartsWorkspaceAdapter(
  chart: WorkspaceCoreChart,
  options: WorkspaceAdapterOptions,
): KLineChartsWorkspaceAdapter {
  const originalCreateOverlay = chart.createOverlay
  const originalOverrideOverlay = chart.overrideOverlay
  const originalRemoveOverlay = chart.removeOverlay
  const originalCreateIndicator = chart.createIndicator
  const originalOverrideIndicator = chart.overrideIndicator
  const originalRemoveIndicator = chart.removeIndicator

  const trackedOverlayIds = new Set<string>()
  const wrappedOverlayIds = new Set<string>()
  const reportedDiagnostics = new Set<string>()
  let attached = false
  let trackingSuppressed = false
  let viewportDirtyQueued = false
  let removalDepth = 0
  let removalDirty = false
  let pending: PendingDataReady | null = null

  function reportOnce(key: string, message: string): void {
    if (reportedDiagnostics.has(key)) return
    reportedDiagnostics.add(key)
    options.reportDiagnostic(message)
  }

  function readIndicatorPanes(): Map<string, Map<string, Indicator>> {
    let value: ReturnType<WorkspaceCoreChart['getIndicatorByPaneId']>
    try {
      value = chart.getIndicatorByPaneId()
    } catch {
      reportOnce('indicator-map:throw', 'KLineCharts indicator registry could not be read')
      return new Map()
    }
    if (!(value instanceof Map)) {
      reportOnce('indicator-map:shape', 'KLineCharts indicator registry has an unsupported shape')
      return new Map()
    }
    const panes = new Map<string, Map<string, Indicator>>()
    for (const [paneId, indicators] of value) {
      if (typeof paneId === 'string' && isIndicatorMap(indicators)) {
        panes.set(paneId, new Map(indicators))
      }
    }
    return panes
  }

  let indicatorPanes = readIndicatorPanes()

  function refreshIndicatorPanes(): Map<string, Map<string, Indicator>> {
    indicatorPanes = readIndicatorPanes()
    return indicatorPanes
  }

  function findIndicatorPane(name: string, includeCandle = true): string | undefined {
    if (includeCandle && indicatorPanes.get('candle_pane')?.has(name)) return 'candle_pane'
    for (const [paneId, indicators] of indicatorPanes) {
      if (paneId !== 'candle_pane' && indicators.has(name)) return paneId
    }
    return undefined
  }

  function markViewDirty(): void {
    if (attached && !trackingSuppressed) options.onViewDirty()
  }

  function markPreferencesDirty(): void {
    if (attached && !trackingSuppressed) options.onPreferencesDirty()
  }

  function markRemovalDirty(): void {
    if (!attached || trackingSuppressed) return
    if (removalDepth > 0) removalDirty = true
    else options.onViewDirty()
  }

  function composeOverlayCallback(
    original: OverlayEventCallback | null,
    after: () => void,
  ): OverlayEventCallback {
    return function (this: unknown, ...args: Parameters<OverlayEventCallback>): boolean {
      try {
        return original ? original.apply(this, args) : false
      } finally {
        after()
      }
    }
  }

  function installOverlayCallbacks(id: string): void {
    if (wrappedOverlayIds.has(id)) return
    const overlay = chart.getOverlayById(id)
    if (!overlay) return
    wrappedOverlayIds.add(id)
    const onDrawEnd = composeOverlayCallback(overlay.onDrawEnd, markViewDirty)
    const onPressedMoveEnd = composeOverlayCallback(overlay.onPressedMoveEnd, markViewDirty)
    const onRemoved = composeOverlayCallback(overlay.onRemoved, () => {
      trackedOverlayIds.delete(id)
      wrappedOverlayIds.delete(id)
      markRemovalDirty()
    })
    originalOverrideOverlay.call(chart, {
      id,
      onDrawEnd,
      onPressedMoveEnd,
      onRemoved,
    } as Partial<OverlayCreate>)
  }

  function trackOverlayResult(result: ReturnType<CreateOverlay>): void {
    for (const id of overlayIds(result)) {
      trackedOverlayIds.add(id)
      installOverlayCallbacks(id)
    }
  }

  function matchesOverlayRemove(overlay: Overlay, remove?: string | OverlayRemove): boolean {
    if (typeof remove === 'string') return overlay.id === remove
    if (!remove) return true
    if (remove.id && overlay.id !== remove.id) return false
    if (remove.groupId && overlay.groupId !== remove.groupId) return false
    if (remove.name && overlay.name !== remove.name) return false
    return true
  }

  function matchingTrackedOverlays(
    finder: { id?: string; groupId?: string; name?: string },
  ): string[] {
    const matches: string[] = []
    for (const id of trackedOverlayIds) {
      const overlay = chart.getOverlayById(id)
      if (!overlay) continue
      if (finder.id && overlay.id !== finder.id) continue
      if (finder.groupId && overlay.groupId !== finder.groupId) continue
      if (finder.name && overlay.name !== finder.name) continue
      matches.push(id)
    }
    return matches
  }

  const patchedCreateOverlay: CreateOverlay = function (
    this: WorkspaceCoreChart,
    ...args: Parameters<CreateOverlay>
  ): ReturnType<CreateOverlay> {
    const result = originalCreateOverlay.apply(this, args)
    trackOverlayResult(result)
    return result
  }

  const patchedOverrideOverlay: OverrideOverlay = function (
    this: WorkspaceCoreChart,
    ...args: Parameters<OverrideOverlay>
  ): ReturnType<OverrideOverlay> {
    const override = args[0] as Partial<OverlayCreate> & {
      id?: string
      groupId?: string
      name?: string
    }
    const targets = matchingTrackedOverlays(override)
    const result = originalOverrideOverlay.apply(this, args)
    if (targets.length > 0) markViewDirty()
    return result
  }

  const patchedRemoveOverlay: RemoveOverlay = function (
    this: WorkspaceCoreChart,
    ...args: Parameters<RemoveOverlay>
  ): ReturnType<RemoveOverlay> {
    const remove = args[0]
    const candidates = [...trackedOverlayIds].filter((id) => {
      const overlay = chart.getOverlayById(id)
      return overlay ? matchesOverlayRemove(overlay, remove) : false
    })
    removalDepth += 1
    try {
      return originalRemoveOverlay.apply(this, args)
    } finally {
      for (const id of candidates) {
        if (!chart.getOverlayById(id)) {
          trackedOverlayIds.delete(id)
          wrappedOverlayIds.delete(id)
          removalDirty = true
        }
      }
      removalDepth -= 1
      if (removalDepth === 0 && removalDirty) {
        removalDirty = false
        markViewDirty()
      }
    }
  }

  const patchedCreateIndicator: CreateIndicator = function (
    this: WorkspaceCoreChart,
    ...args: Parameters<CreateIndicator>
  ): ReturnType<CreateIndicator> {
    const name = indicatorName(args[0])
    refreshIndicatorPanes()
    const existingPane = findIndicatorPane(name)
    if (existingPane) return existingPane
    const before = indicatorSignature(indicatorPanes)
    const result = originalCreateIndicator.apply(this, args)
    const after = indicatorSignature(refreshIndicatorPanes())
    if (result !== null && before !== after) markPreferencesDirty()
    return result
  }

  const patchedRemoveIndicator: RemoveIndicator = function (
    this: WorkspaceCoreChart,
    ...args: Parameters<RemoveIndicator>
  ): ReturnType<RemoveIndicator> {
    const [paneId, name] = args
    refreshIndicatorPanes()
    const pane = indicatorPanes.get(paneId)
    if (!pane || (name ? !pane.has(name) : pane.size === 0)) return undefined
    const before = indicatorSignature(indicatorPanes)
    const result = originalRemoveIndicator.apply(this, args)
    const after = indicatorSignature(refreshIndicatorPanes())
    if (before !== after) markPreferencesDirty()
    return result
  }

  function captureViewport(): ChartViewportSnapshot {
    const viewport: ChartViewportSnapshot = {}
    try {
      const barSpace = chart.getBarSpace()
      if (Number.isFinite(barSpace)
        && barSpace >= MIN_BAR_SPACE
        && barSpace <= MAX_BAR_SPACE) {
        viewport.barSpace = barSpace
      }
    } catch {
      // A malformed compatibility surface is captured as an empty viewport.
    }
    try {
      const data = chart.getDataList()
      const visibleRange = chart.getVisibleRange()
      if (Array.isArray(data)
        && visibleRange
        && Number.isFinite(visibleRange.to)
        && visibleRange.to > 0
        && data.length > 0) {
        const index = Math.min(Math.trunc(visibleRange.to) - 1, data.length - 1)
        const timestamp = index >= 0 ? data[index]?.timestamp : undefined
        if (typeof timestamp === 'number' && Number.isFinite(timestamp) && timestamp > 0) {
          viewport.rightTimestamp = timestamp
        }
      }
    } catch {
      // A malformed compatibility surface is captured as an empty viewport.
    }
    return viewport
  }

  function resolveCapturedPane(paneId: string): ChartPaneRef | null {
    if (paneId === 'candle_pane') return { kind: 'candle' }
    const indicators = chart.getIndicatorByPaneId(paneId)
    if (isIndicatorMap(indicators)) {
      const first = indicators.values().next().value as Indicator | undefined
      if (first) return { kind: 'indicator', indicatorName: first.name }
    }
    reportOnce(`capture-pane:${paneId}`, `Skipping overlay from unknown pane ${paneId}`)
    return null
  }

  function snapshotOverlay(overlay: Overlay, pane: ChartPaneRef): ChartOverlaySnapshot {
    return {
      id: overlay.id,
      groupId: overlay.groupId,
      pane,
      name: overlay.name,
      lock: overlay.lock,
      visible: overlay.visible,
      zLevel: overlay.zLevel,
      mode: overlay.mode,
      modeSensitivity: overlay.modeSensitivity,
      points: jsonPoints(overlay.points),
      extendData: toJsonValue(overlay.extendData),
      styles: toJsonValue(overlay.styles),
    }
  }

  function removeTrackedOverlays(): void {
    for (const id of [...trackedOverlayIds]) {
      try {
        originalRemoveOverlay.call(chart, id)
      } catch {
        reportOnce(`overlay-remove:${id}`, `Unable to remove stale overlay ${id}`)
      }
      trackedOverlayIds.delete(id)
      wrappedOverlayIds.delete(id)
    }
  }

  function reconcileIndicators(preferences: ChartPreferencesV1): void {
    refreshIndicatorPanes()
    const wantedMain = new Set(preferences.mainIndicators)
    const wantedSub = new Set(preferences.subIndicators)
    for (const [paneId, indicators] of indicatorPanes) {
      const wanted = paneId === 'candle_pane' ? wantedMain : wantedSub
      for (const name of indicators.keys()) {
        if (!wanted.has(name)) originalRemoveIndicator.call(chart, paneId, name)
      }
    }
    refreshIndicatorPanes()
    for (const name of preferences.mainIndicators) {
      if (!indicatorPanes.get('candle_pane')?.has(name)) {
        const paneId = originalCreateIndicator.call(chart, name, true, { id: 'candle_pane' })
        if (!paneId) reportOnce(`indicator-create:main:${name}`, `Unable to restore main indicator ${name}`)
        refreshIndicatorPanes()
      }
    }
    for (const name of preferences.subIndicators) {
      if (!findIndicatorPane(name, false)) {
        const paneId = originalCreateIndicator.call(chart, name, true)
        if (!paneId) reportOnce(`indicator-create:sub:${name}`, `Unable to restore sub indicator ${name}`)
        refreshIndicatorPanes()
      }
    }
  }

  function paneForSnapshot(pane: ChartPaneRef): string {
    if (pane.kind === 'candle') return 'candle_pane'
    refreshIndicatorPanes()
    const paneId = findIndicatorPane(pane.indicatorName, false)
    if (paneId) return paneId
    reportOnce(
      `restore-pane:${pane.indicatorName}`,
      `Indicator pane ${pane.indicatorName} is unavailable; using the candle pane`,
    )
    return 'candle_pane'
  }

  function restoreOverlays(overlays: ChartOverlaySnapshot[]): void {
    for (const snapshot of overlays) {
      const paneId = paneForSnapshot(snapshot.pane)
      const value = {
        id: snapshot.id,
        groupId: snapshot.groupId,
        name: snapshot.name,
        lock: snapshot.lock,
        visible: snapshot.visible,
        zLevel: snapshot.zLevel,
        mode: snapshot.mode,
        modeSensitivity: snapshot.modeSensitivity,
        points: snapshot.points,
        extendData: snapshot.extendData,
        styles: snapshot.styles,
      } as unknown as OverlayCreate
      try {
        const result = originalCreateOverlay.call(chart, value, paneId)
        const ids = overlayIds(result)
        if (ids.length === 0) {
          reportOnce(`restore-overlay:${snapshot.name}`, `Overlay ${snapshot.name} is unavailable`)
          continue
        }
        trackOverlayResult(result)
      } catch {
        reportOnce(`restore-overlay:${snapshot.name}`, `Overlay ${snapshot.name} could not be restored`)
      }
    }
  }

  function restoreViewport(viewport: ChartViewportSnapshot): void {
    if (typeof viewport.barSpace === 'number'
      && Number.isFinite(viewport.barSpace)
      && viewport.barSpace >= MIN_BAR_SPACE
      && viewport.barSpace <= MAX_BAR_SPACE) {
      chart.setBarSpace(viewport.barSpace)
    }
    if (typeof viewport.rightTimestamp === 'number'
      && Number.isFinite(viewport.rightTimestamp)
      && viewport.rightTimestamp > 0) {
      chart.scrollToTimestamp(viewport.rightTimestamp)
    }
  }

  function settlePending(): void {
    const stale = pending
    pending = null
    stale?.resolve()
  }

  function armPending(
    generation: number,
    reason: PendingDataReady['reason'],
    viewport: ChartViewportSnapshot,
    viewState?: ChartViewStateV1,
  ): PendingDataReady {
    settlePending()
    const token = pendingDataReady(generation, reason, viewport, viewState)
    pending = token
    return token
  }

  const onViewportAction: ActionCallback = () => {
    if (!attached || trackingSuppressed || viewportDirtyQueued) return
    viewportDirtyQueued = true
    queueMicrotask(() => {
      viewportDirtyQueued = false
      if (attached && !trackingSuppressed) options.onViewDirty()
    })
  }

  const onDataReady: ActionCallback = () => {
    if (!pending) return
    const token = pending
    pending = null
    try {
      if (token.reason === 'persisted' && token.viewState) {
        refreshIndicatorPanes()
        restoreOverlays(token.viewState.overlays)
      }
      restoreViewport(token.viewport)
    } catch {
      reportOnce(`data-ready:${token.reason}`, `KLineCharts ${token.reason} restoration failed`)
    } finally {
      trackingSuppressed = false
      token.resolve()
    }
  }

  function attach(): void {
    if (attached) return
    attached = true
    chart.createOverlay = patchedCreateOverlay
    chart.overrideOverlay = patchedOverrideOverlay
    chart.removeOverlay = patchedRemoveOverlay
    chart.createIndicator = patchedCreateIndicator
    chart.overrideIndicator = originalOverrideIndicator
    chart.removeIndicator = patchedRemoveIndicator
    chart.subscribeAction(ActionType.OnZoom, onViewportAction)
    chart.subscribeAction(ActionType.OnScroll, onViewportAction)
    chart.subscribeAction(ActionType.OnVisibleRangeChange, onViewportAction)
    chart.subscribeAction(ActionType.OnDataReady, onDataReady)
  }

  function detach(): void {
    if (!attached) return
    attached = false
    viewportDirtyQueued = false
    chart.unsubscribeAction(ActionType.OnZoom, onViewportAction)
    chart.unsubscribeAction(ActionType.OnScroll, onViewportAction)
    chart.unsubscribeAction(ActionType.OnVisibleRangeChange, onViewportAction)
    chart.unsubscribeAction(ActionType.OnDataReady, onDataReady)
    chart.createOverlay = originalCreateOverlay
    chart.overrideOverlay = originalOverrideOverlay
    chart.removeOverlay = originalRemoveOverlay
    chart.createIndicator = originalCreateIndicator
    chart.overrideIndicator = originalOverrideIndicator
    chart.removeIndicator = originalRemoveIndicator
    trackingSuppressed = false
    settlePending()
  }

  function prepareViewReplacement(
    preferences: ChartPreferencesV1,
    viewState: ChartViewStateV1,
    generation: number,
  ): Promise<void> {
    trackingSuppressed = true
    settlePending()
    removeTrackedOverlays()
    try {
      reconcileIndicators(preferences)
    } catch {
      reportOnce('indicator-reconcile', 'KLineCharts indicators could not be reconciled')
    }
    const token = armPending(generation, 'persisted', viewState.viewport, viewState)
    return token.promise
  }

  function applyPrimedHistoryFallback(data: KLineData[], generation: number): void {
    if (!pending || pending.reason !== 'persisted' || pending.generation !== generation) return
    try {
      chart.applyNewData(data)
    } catch {
      reportOnce('primed-history-fallback', 'KLineCharts primed history fallback failed')
      cancelGeneration(generation)
    }
  }

  function applyRefreshedHistory(data: KLineData[], generation: number): Promise<void> {
    trackingSuppressed = true
    const viewport = captureViewport()
    const merged = mergeRefreshedAndLive(data, chart.getDataList())
    const token = armPending(generation, 'refresh', viewport)
    try {
      chart.applyNewData(merged)
    } catch {
      reportOnce('refreshed-history', 'KLineCharts refreshed history replacement failed')
      cancelGeneration(generation)
    }
    return token.promise
  }

  function cancelGeneration(generation: number): void {
    if (!pending || pending.generation !== generation) return
    const stale = pending
    pending = null
    trackingSuppressed = false
    stale.resolve()
  }

  function capturePreferences(revision: number): ChartPreferencesV1 {
    refreshIndicatorPanes()
    const mainIndicators: string[] = []
    const subIndicators: string[] = []
    for (const [paneId, indicators] of indicatorPanes) {
      const target = paneId === 'candle_pane' ? mainIndicators : subIndicators
      for (const item of indicators.values()) target.push(item.name)
    }
    return {
      schemaVersion: 1,
      revision,
      savedAtMs: Date.now(),
      mainIndicators,
      subIndicators,
    }
  }

  function captureViewState(workspaceKey: ChartWorkspaceKey, revision: number): ChartViewStateV1 {
    const normalized = normalizeChartWorkspaceKey(workspaceKey)
    const overlays: ChartOverlaySnapshot[] = []
    for (const id of trackedOverlayIds) {
      const overlay = chart.getOverlayById(id)
      if (!overlay || overlay.currentStep !== -1) continue
      const pane = resolveCapturedPane(overlay.paneId)
      if (pane) overlays.push(snapshotOverlay(overlay, pane))
    }
    return {
      schemaVersion: 1,
      symbol: normalized.symbol,
      interval: normalized.interval,
      revision,
      savedAtMs: Date.now(),
      overlays,
      viewport: captureViewport(),
    }
  }

  return {
    attach,
    detach,
    prepareViewReplacement,
    applyPrimedHistoryFallback,
    applyRefreshedHistory,
    cancelGeneration,
    capturePreferences,
    captureViewState,
    resize: () => chart.resize(),
  }
}
