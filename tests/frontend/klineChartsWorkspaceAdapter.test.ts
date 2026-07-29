import { describe, expect, it, vi } from 'vitest'

import {
  ActionType,
  type ActionCallback,
  type Indicator,
  type IndicatorCreate,
  type KLineData,
  type Overlay,
  type OverlayCreate,
  type OverlayEvent,
  type OverlayRemove,
  type PaneOptions,
  type VisibleRange,
} from 'klinecharts'
import {
  createKLineChartsWorkspaceAdapter,
  type WorkspaceAdapterOptions,
} from '../../src/services/klineChartsWorkspaceAdapter'
import type { WorkspaceCoreChart } from '../../src/services/klineChartsCoreBridge'
import type {
  ChartOverlaySnapshot,
  ChartPreferencesV1,
  ChartViewStateV1,
} from '../../src/types/chartWorkspace'
import {
  key,
  samplePreferences,
  segment,
  viewState,
} from './helpers/chartWorkspaceFixtures'

const bars = (count: number): KLineData[] => Array.from({ length: count }, (_, index) => ({
  timestamp: (index + 1) * 1_000,
  open: 1,
  high: 2,
  low: 0.5,
  close: 1.5,
  volume: 10,
}))

const range = (from: number, to: number): VisibleRange => ({
  from,
  to,
  realFrom: from,
  realTo: to,
})

const indicator = (name: string): Indicator => ({ name } as Indicator)

const preferences = (mainIndicators: string[], subIndicators: string[]): ChartPreferencesV1 => ({
  ...samplePreferences(),
  mainIndicators,
  subIndicators,
})

const viewWithIndicatorOverlay = (indicatorName: string): ChartViewStateV1 => ({
  ...viewState(),
  overlays: [{
    ...segment(),
    pane: { kind: 'indicator', indicatorName },
  }],
  viewport: { barSpace: 8, rightTimestamp: 5_000 },
})

function overlay(patch: Partial<Overlay> = {}): Overlay {
  return {
    id: 'overlay-1',
    groupId: 'group-1',
    paneId: 'candle_pane',
    name: 'segment',
    totalStep: 2,
    currentStep: -1,
    lock: false,
    visible: true,
    zLevel: 0,
    needDefaultPointFigure: true,
    needDefaultXAxisFigure: true,
    needDefaultYAxisFigure: true,
    mode: 'normal',
    modeSensitivity: 8,
    points: [{ timestamp: 1_000, value: 1 }],
    extendData: null,
    styles: null,
    createPointFigures: null,
    createXAxisFigures: null,
    createYAxisFigures: null,
    performEventPressedMove: null,
    performEventMoveForDrawing: null,
    onDrawStart: null,
    onDrawing: null,
    onDrawEnd: null,
    onClick: null,
    onDoubleClick: null,
    onRightClick: null,
    onPressedMoveStart: null,
    onPressedMoving: null,
    onPressedMoveEnd: null,
    onMouseEnter: null,
    onMouseLeave: null,
    onRemoved: null,
    onSelected: null,
    onDeselected: null,
    ...patch,
  } as Overlay
}

function overlayCreateResult(id: string): OverlayCreate {
  return { id, name: 'segment' } as OverlayCreate
}

interface FakeChartOptions {
  overlays?: Overlay[]
  indicatorMaps?: Map<string, Map<string, Indicator>>
  visibleRange?: VisibleRange
  data?: KLineData[]
  barSpace?: number
}

function fakeChart(options: FakeChartOptions = {}) {
  const overlayMap = new Map((options.overlays ?? []).map((item) => [item.id, item]))
  const indicatorMaps = options.indicatorMaps ?? new Map<string, Map<string, Indicator>>()
  const subscriptions = new Map<ActionType, Set<ActionCallback>>()
  const calls: string[] = []
  let data = [...(options.data ?? bars(8))]
  let visibleRange = options.visibleRange ?? range(0, data.length)
  let barSpace = options.barSpace ?? 6
  let overlaySequence = 0

  const createOne = (value: string | OverlayCreate, paneId?: string): string | null => {
    const name = typeof value === 'string' ? value : value.name
    if (name === 'missing') return null
    const suppliedId = typeof value === 'string'
      ? undefined
      : (value as OverlayCreate & { id?: string }).id
    const id = suppliedId ?? `created-${++overlaySequence}`
    const existing = overlayMap.get(id) ?? overlay({ id, name })
    Object.assign(existing, typeof value === 'string' ? { name: value } : value)
    existing.id = id
    existing.paneId = paneId ?? existing.paneId
    overlayMap.set(id, existing)
    calls.push(`createOverlay:${name}:${existing.paneId}`)
    return id
  }

  const createOverlay = vi.fn(function (
    this: unknown,
    value: string | OverlayCreate | Array<string | OverlayCreate>,
    paneId?: string,
  ) {
    return Array.isArray(value)
      ? value.map((item) => createOne(item, paneId))
      : createOne(value, paneId)
  })
  const getOverlayById = vi.fn((id: string) => overlayMap.get(id) ?? null)
  const overrideOverlay = vi.fn(function (this: unknown, patch: Partial<OverlayCreate>) {
    const finder = patch as Partial<OverlayCreate> & { id?: string; groupId?: string }
    for (const item of overlayMap.values()) {
      if (finder.id && item.id !== finder.id) continue
      if (finder.groupId && item.groupId !== finder.groupId) continue
      if (!finder.id && !finder.groupId && finder.name && item.name !== finder.name) continue
      Object.assign(item, patch)
    }
  })
  const removeOverlay = vi.fn(function (this: unknown, remove?: string | OverlayRemove) {
    const finder = typeof remove === 'string' ? { id: remove } : (remove ?? {})
    for (const [id, item] of [...overlayMap]) {
      if (finder.id && id !== finder.id) continue
      if (finder.groupId && item.groupId !== finder.groupId) continue
      if (finder.name && item.name !== finder.name) continue
      item.onRemoved?.call(item, { overlay: item } as OverlayEvent)
      overlayMap.delete(id)
    }
  })

  const createIndicator = vi.fn(function (
    this: unknown,
    value: string | IndicatorCreate,
    _isStack?: boolean,
    paneOptions?: PaneOptions,
  ) {
    const name = typeof value === 'string' ? value : value.name
    const paneId = paneOptions?.id ?? `pane_${name.toLowerCase()}`
    const pane = indicatorMaps.get(paneId) ?? new Map<string, Indicator>()
    pane.set(name, indicator(name))
    indicatorMaps.set(paneId, pane)
    calls.push(`createIndicator:${name}:${paneId}`)
    return paneId
  })
  const getIndicatorByPaneId = vi.fn((paneId?: string, name?: string) => {
    if (!paneId) return indicatorMaps
    const pane = indicatorMaps.get(paneId)
    if (name) return pane?.get(name) ?? null
    return pane ?? null
  })
  const overrideIndicator = vi.fn(function (this: unknown) {})
  const removeIndicator = vi.fn(function (this: unknown, paneId: string, name?: string) {
    const pane = indicatorMaps.get(paneId)
    if (!pane) return
    if (name) pane.delete(name)
    else pane.clear()
    if (paneId !== 'candle_pane' && pane.size === 0) indicatorMaps.delete(paneId)
    calls.push(`removeIndicator:${name ?? '*'}:${paneId}`)
  })

  const applyNewData = vi.fn(function (this: unknown, nextData: KLineData[]) {
    data = [...nextData]
    calls.push(`applyNewData:${nextData.length}`)
  })
  const getBarSpace = vi.fn(() => barSpace)
  const setBarSpace = vi.fn((space: number) => {
    barSpace = space
    calls.push('setBarSpace')
  })
  const getVisibleRange = vi.fn(() => visibleRange)
  const getDataList = vi.fn(() => data)
  const scrollToTimestamp = vi.fn(() => { calls.push('scrollToTimestamp') })
  const subscribeAction = vi.fn((type: ActionType, callback: ActionCallback) => {
    const callbacks = subscriptions.get(type) ?? new Set<ActionCallback>()
    callbacks.add(callback)
    subscriptions.set(type, callbacks)
  })
  const unsubscribeAction = vi.fn((type: ActionType, callback?: ActionCallback) => {
    if (callback) subscriptions.get(type)?.delete(callback)
    else subscriptions.delete(type)
  })
  const resize = vi.fn()

  const chart = {
    id: 'k_line_chart_1',
    applyNewData,
    createOverlay,
    getOverlayById,
    overrideOverlay,
    removeOverlay,
    createIndicator,
    getIndicatorByPaneId,
    overrideIndicator,
    removeIndicator,
    getBarSpace,
    setBarSpace,
    getVisibleRange,
    getDataList,
    scrollToTimestamp,
    subscribeAction,
    unsubscribeAction,
    resize,
  } as WorkspaceCoreChart

  return {
    chart,
    calls,
    overlays: overlayMap,
    indicatorMaps,
    originals: {
      createOverlay,
      overrideOverlay,
      removeOverlay,
      createIndicator,
      overrideIndicator,
      removeIndicator,
    },
    emitAction(type: ActionType): void {
      for (const callback of subscriptions.get(type) ?? []) callback()
    },
    setVisibleRange(next: VisibleRange): void { visibleRange = next },
  }
}

function callbacks() {
  return {
    onViewDirty: vi.fn(),
    onPreferencesDirty: vi.fn(),
    reportDiagnostic: vi.fn(),
  } satisfies WorkspaceAdapterOptions
}

function adapterFixture(chartOptions: FakeChartOptions = {}) {
  const fake = fakeChart(chartOptions)
  const options = callbacks()
  const adapter = createKLineChartsWorkspaceAdapter(fake.chart, options)
  adapter.attach()
  return { ...fake, options, adapter }
}

function restoreFixture(chartOptions: FakeChartOptions = {}) {
  return adapterFixture(chartOptions)
}

describe('KLineCharts workspace adapter', () => {
  it('captures only completed registered overlays with semantic panes', () => {
    const fixture = adapterFixture({
      overlays: [
        overlay({ id: 'done', currentStep: -1, paneId: 'pane_macd' }),
        overlay({ id: 'drawing', currentStep: 2, paneId: 'candle_pane' }),
      ],
      indicatorMaps: new Map([
        ['pane_macd', new Map([['MACD', indicator('MACD')]])],
      ]),
    })
    fixture.chart.createOverlay(overlayCreateResult('done'))
    fixture.chart.createOverlay(overlayCreateResult('drawing'))

    expect(fixture.adapter.captureViewState(key(), 4).overlays).toEqual([
      expect.objectContaining({
        id: 'done',
        pane: { kind: 'indicator', indicatorName: 'MACD' },
      }),
    ])
  })

  it('uses the exclusive visible-range upper bound for the right timestamp', () => {
    const fixture = adapterFixture({
      visibleRange: { from: 2, to: 5, realFrom: 2, realTo: 5 },
      data: bars(8),
    })

    expect(fixture.adapter.captureViewState(key(), 1).viewport.rightTimestamp)
      .toBe(bars(8)[4]!.timestamp)
  })

  it('guards viewport values outside the version-locked core range', async () => {
    const fixture = adapterFixture({
      barSpace: 500,
      visibleRange: range(0, Number.POSITIVE_INFINITY),
    })
    expect(fixture.adapter.captureViewState(key(), 1).viewport).toEqual({})

    const restored = fixture.adapter.prepareViewReplacement(
      preferences([], []),
      { ...viewState(), viewport: { barSpace: 500, rightTimestamp: -1 } },
      1,
    )
    fixture.emitAction(ActionType.OnDataReady)
    await restored
    expect(fixture.calls).toEqual([])
  })

  it('adopts Pro-created indicators before restoring semantic panes', async () => {
    const fixture = restoreFixture({
      indicatorMaps: new Map([
        ['candle_pane', new Map([['MA', indicator('MA')]])],
        ['pane_macd', new Map([['MACD', indicator('MACD')]])],
      ]),
    })
    const restored = fixture.adapter.prepareViewReplacement(
      preferences(['MA'], ['MACD']),
      viewWithIndicatorOverlay('MACD'),
      1,
    )
    fixture.emitAction(ActionType.OnDataReady)
    await restored

    expect(fixture.calls).toEqual([
      'createOverlay:segment:pane_macd',
      'setBarSpace',
      'scrollToTimestamp',
    ])
  })

  it('reapplies the live viewport after guarded REST replacement', async () => {
    const fixture = restoreFixture({ visibleRange: range(2, 5), data: bars(8) })
    const refreshed = fixture.adapter.applyRefreshedHistory(bars(12), 3)
    expect(fixture.calls[0]).toBe('applyNewData:12')
    fixture.emitAction(ActionType.OnDataReady)
    await refreshed

    expect(fixture.calls.slice(-2)).toEqual(['setBarSpace', 'scrollToTimestamp'])
  })

  it('wrapped_mutators_preserve_this_arguments_and_return_values', () => {
    const fixture = adapterFixture()
    const createArgs = ['segment', 'candle_pane'] as const
    const created = fixture.chart.createOverlay(...createArgs)
    expect(created).toBe('created-1')
    expect(fixture.originals.createOverlay.mock.instances[0]).toBe(fixture.chart)
    expect(fixture.originals.createOverlay.mock.calls[0]).toEqual(createArgs)

    const overrideArg = { id: 'created-1', visible: false }
    expect(fixture.chart.overrideOverlay(overrideArg)).toBeUndefined()
    expect(fixture.originals.overrideOverlay.mock.instances.at(-1)).toBe(fixture.chart)
    expect(fixture.originals.overrideOverlay.mock.calls.at(-1)).toEqual([overrideArg])

    const removeArg = { id: 'created-1' }
    expect(fixture.chart.removeOverlay(removeArg)).toBeUndefined()
    expect(fixture.originals.removeOverlay.mock.instances.at(-1)).toBe(fixture.chart)
    expect(fixture.originals.removeOverlay.mock.calls.at(-1)).toEqual([removeArg])
  })

  it('overlay_callbacks_are_composed', () => {
    const fixture = adapterFixture()
    const receiver = { receiver: true }
    const event = { overlay: overlay() } as OverlayEvent
    const onDrawEnd = vi.fn(function (this: unknown, received: OverlayEvent) {
      expect(this).toBe(receiver)
      expect(received).toBe(event)
      return true
    })
    const onPressedMoveEnd = vi.fn(function (this: unknown) { return true })
    const onRemoved = vi.fn(function (this: unknown) { return true })
    const input = overlayCreateResult('callbacks') as OverlayCreate & {
      onDrawEnd: typeof onDrawEnd
      onPressedMoveEnd: typeof onPressedMoveEnd
      onRemoved: typeof onRemoved
    }
    input.onDrawEnd = onDrawEnd
    input.onPressedMoveEnd = onPressedMoveEnd
    input.onRemoved = onRemoved
    fixture.chart.createOverlay(input)
    const tracked = fixture.overlays.get('callbacks')!

    expect(tracked.onDrawEnd?.call(receiver, event)).toBe(true)
    expect(tracked.onPressedMoveEnd?.call(receiver, event)).toBe(true)
    expect(tracked.onRemoved?.call(receiver, event)).toBe(true)
    expect(onDrawEnd).toHaveBeenCalledOnce()
    expect(onPressedMoveEnd).toHaveBeenCalledOnce()
    expect(onRemoved).toHaveBeenCalledOnce()
    expect(fixture.options.onViewDirty).toHaveBeenCalledTimes(3)
  })

  it('indicator_mutation_marks_only_preferences_dirty', () => {
    const fixture = adapterFixture()
    const paneId = fixture.chart.createIndicator('RSI')
    fixture.chart.overrideIndicator({ name: 'RSI', visible: false } as IndicatorCreate, paneId ?? undefined)
    if (paneId) fixture.chart.removeIndicator(paneId, 'RSI')

    expect(fixture.options.onPreferencesDirty).toHaveBeenCalledTimes(2)
    expect(fixture.options.onViewDirty).not.toHaveBeenCalled()
    expect(fixture.chart.overrideIndicator).toBe(fixture.originals.overrideIndicator)
  })

  it('indicator_create_is_idempotent_after_cross_surface_restore', () => {
    const fixture = adapterFixture({
      indicatorMaps: new Map([
        ['pane_rsi', new Map([['RSI', indicator('RSI')]])],
      ]),
    })
    fixture.originals.createIndicator.mockClear()

    expect(fixture.chart.createIndicator('RSI')).toBe('pane_rsi')
    expect(fixture.originals.createIndicator).not.toHaveBeenCalled()
    expect(fixture.options.onPreferencesDirty).not.toHaveBeenCalled()
  })

  it('snapshot_json_sanitization_never_leaks_runtime_values', () => {
    const circular: Record<string, unknown> = { fn: () => undefined, nan: Number.NaN }
    circular.self = circular
    const runtimeOverlay = overlay({
      id: 'unsafe',
      extendData: circular,
      styles: { fn: () => undefined, infinite: Number.POSITIVE_INFINITY },
      points: [{
        timestamp: Number.POSITIVE_INFINITY,
        dataIndex: Number.NaN,
        value: (() => undefined) as unknown as number,
      }],
    })
    const fixture = adapterFixture({ overlays: [runtimeOverlay] })
    fixture.chart.createOverlay(overlayCreateResult('unsafe'))

    const captured = fixture.adapter.captureViewState(key(), 1).overlays[0]!
    expect(captured.extendData).toEqual({ fn: null, nan: null, self: null })
    expect(captured.styles).toEqual({ fn: null, infinite: null })
    expect(captured.points).toEqual([{ timestamp: null, dataIndex: null, value: null }])
    expect(() => JSON.stringify(captured)).not.toThrow()
  })

  it('unknown_overlay_names_are_skipped_independently', async () => {
    const fixture = restoreFixture()
    const overlays: ChartOverlaySnapshot[] = [
      { ...segment('missing-id'), name: 'missing' },
      segment('valid-id'),
    ]
    const restored = fixture.adapter.prepareViewReplacement(
      preferences([], []),
      { ...viewState(), overlays },
      1,
    )
    fixture.emitAction(ActionType.OnDataReady)
    await restored

    expect([...fixture.overlays.values()].map((item) => item.name)).toEqual(['segment'])
    expect(fixture.options.reportDiagnostic).toHaveBeenCalledOnce()
  })

  it('missing_indicator_pane_falls_back_once', async () => {
    const fixture = restoreFixture()
    const indicatorOverlay = (id: string): ChartOverlaySnapshot => ({
      ...segment(id),
      pane: { kind: 'indicator', indicatorName: 'MACD' },
    })
    const restored = fixture.adapter.prepareViewReplacement(
      preferences([], []),
      { ...viewState(), overlays: [indicatorOverlay('one'), indicatorOverlay('two')] },
      1,
    )
    fixture.emitAction(ActionType.OnDataReady)
    await restored

    expect(fixture.calls.filter((call) => call.startsWith('createOverlay'))).toEqual([
      'createOverlay:segment:candle_pane',
      'createOverlay:segment:candle_pane',
    ])
    expect(fixture.options.reportDiagnostic).toHaveBeenCalledOnce()
  })

  it('detach_is_exactly_reversible', () => {
    const fake = fakeChart()
    const options = callbacks()
    const originals = {
      createOverlay: fake.chart.createOverlay,
      overrideOverlay: fake.chart.overrideOverlay,
      removeOverlay: fake.chart.removeOverlay,
      createIndicator: fake.chart.createIndicator,
      overrideIndicator: fake.chart.overrideIndicator,
      removeIndicator: fake.chart.removeIndicator,
    }
    const adapter = createKLineChartsWorkspaceAdapter(fake.chart, options)

    adapter.attach()
    adapter.attach()
    expect(fake.chart.createOverlay).not.toBe(originals.createOverlay)
    adapter.detach()
    adapter.detach()
    expect(fake.chart).toMatchObject(originals)
    adapter.attach()
    adapter.detach()

    expect(fake.chart.unsubscribeAction).toHaveBeenCalledTimes(8)
    const subscribed = fake.chart.subscribeAction as ReturnType<typeof vi.fn>
    const unsubscribed = fake.chart.unsubscribeAction as ReturnType<typeof vi.fn>
    for (let index = 0; index < unsubscribed.mock.calls.length; index += 1) {
      expect(unsubscribed.mock.calls[index]).toEqual(subscribed.mock.calls[index])
    }
  })

  it('detached overlay callbacks do not emit persistence dirtiness', () => {
    const fixture = adapterFixture()
    fixture.chart.createOverlay(overlayCreateResult('detached'))
    const onDrawEnd = fixture.overlays.get('detached')!.onDrawEnd!

    fixture.adapter.detach()
    expect(onDrawEnd({ overlay: fixture.overlays.get('detached')! } as OverlayEvent)).toBe(false)
    expect(fixture.options.onViewDirty).not.toHaveBeenCalled()
  })

  it('viewport_actions_coalesce_one_dirty_notification', async () => {
    const fixture = adapterFixture()

    fixture.emitAction(ActionType.OnZoom)
    fixture.emitAction(ActionType.OnScroll)
    fixture.emitAction(ActionType.OnVisibleRangeChange)
    await Promise.resolve()

    expect(fixture.options.onViewDirty).toHaveBeenCalledOnce()
  })

  it('captures main and sub indicators in pane insertion order', () => {
    const fixture = adapterFixture({
      indicatorMaps: new Map([
        ['candle_pane', new Map([
          ['EMA', indicator('EMA')],
          ['MA', indicator('MA')],
        ])],
        ['pane_rsi', new Map([['RSI', indicator('RSI')]])],
        ['pane_macd', new Map([['MACD', indicator('MACD')]])],
      ]),
    })

    expect(fixture.adapter.capturePreferences(7)).toMatchObject({
      revision: 7,
      mainIndicators: ['EMA', 'MA'],
      subIndicators: ['RSI', 'MACD'],
    })
  })

  it('ignores realtime data-ready actions when no token is pending', () => {
    const fixture = adapterFixture()
    fixture.emitAction(ActionType.OnDataReady)

    expect(fixture.calls).toEqual([])
    expect(fixture.options.onViewDirty).not.toHaveBeenCalled()
  })

  it('uses the armed persisted token for a proven primed-history fallback', async () => {
    const fixture = restoreFixture()
    const restored = fixture.adapter.prepareViewReplacement(
      preferences([], []),
      viewState(),
      4,
    )

    fixture.adapter.applyPrimedHistoryFallback(bars(3), 3)
    expect(fixture.chart.applyNewData).not.toHaveBeenCalled()
    fixture.adapter.applyPrimedHistoryFallback(bars(3), 4)
    expect(fixture.chart.applyNewData).toHaveBeenCalledOnce()
    fixture.emitAction(ActionType.OnDataReady)
    await restored
  })

  it('lets existing realtime bars win when refreshed history is merged', async () => {
    const live = bars(3)
    live[2] = { ...live[2]!, close: 77 }
    const refreshed = bars(5)
    refreshed[2] = { ...refreshed[2]!, close: 12 }
    const fixture = restoreFixture({ data: live })

    const completed = fixture.adapter.applyRefreshedHistory(refreshed, 2)
    const applied = fixture.chart.applyNewData as ReturnType<typeof vi.fn>
    expect(applied.mock.calls[0]![0][2].close).toBe(77)
    fixture.emitAction(ActionType.OnDataReady)
    await completed
  })

  it('cancelGeneration resolves a stale gate without applying restoration', async () => {
    const fixture = restoreFixture()
    const restored = fixture.adapter.prepareViewReplacement(
      preferences([], []),
      { ...viewState(), viewport: { barSpace: 9, rightTimestamp: 4_000 } },
      8,
    )

    fixture.adapter.cancelGeneration(8)
    await restored
    fixture.emitAction(ActionType.OnDataReady)
    expect(fixture.calls).toEqual([])
  })
})
