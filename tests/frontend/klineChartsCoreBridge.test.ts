import type { Chart } from 'klinecharts'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const registry = vi.hoisted(() => new Map<string, Chart>())
const registryInit = vi.hoisted(() => vi.fn((container: HTMLElement): Chart | null => (
  registry.get(container.id) ?? null
)))

vi.mock('klinecharts', async (importOriginal) => {
  const actual = await importOriginal<typeof import('klinecharts')>()
  return {
    ...actual,
    version: () => '9.8.12',
    init: registryInit,
  }
})

import {
  recoverKLineChartsCore,
  type CoreBridgeDependencies,
  type WorkspaceCoreChart,
} from '../../src/services/klineChartsCoreBridge'

const methodNames = [
  'applyNewData',
  'createOverlay',
  'getOverlayById',
  'overrideOverlay',
  'removeOverlay',
  'createIndicator',
  'getIndicatorByPaneId',
  'overrideIndicator',
  'removeIndicator',
  'getBarSpace',
  'setBarSpace',
  'getVisibleRange',
  'getDataList',
  'scrollToTimestamp',
  'subscribeAction',
  'unsubscribeAction',
  'resize',
] as const

function fakeCoreChart(id = 'k_line_chart_1'): WorkspaceCoreChart {
  const chart: Record<string, unknown> = { id }
  for (const method of methodNames) {
    chart[method] = vi.fn()
  }
  return chart as unknown as WorkspaceCoreChart
}

function hostFixture(options: { marker?: string | null; widgetId?: string } = {}) {
  const host = document.createElement('div')
  const widget = document.createElement('div')
  widget.className = 'klinecharts-pro-widget'
  if (options.marker !== null) {
    widget.setAttribute('k-line-chart-id', options.marker ?? 'k_line_chart_1')
  }
  widget.id = options.widgetId ?? ''
  host.append(widget)
  return { host, widget }
}

function recoverFixture(overrides: {
  version?: () => string
  marker?: string | null
  widgetId?: string
  returnedId?: string
  removeMethod?: typeof methodNames[number]
} = {}) {
  const { host, widget } = hostFixture(overrides)
  const chart = fakeCoreChart(overrides.returnedId)
  if (overrides.removeMethod) {
    Reflect.deleteProperty(chart, overrides.removeMethod)
  }
  const init = vi.fn().mockReturnValue(chart)
  const dependencies: CoreBridgeDependencies = {
    expectedVersion: '9.8.12',
    version: overrides.version ?? (() => '9.8.12'),
    init,
  }
  return {
    result: recoverKLineChartsCore(host, dependencies),
    widget,
    chart,
    init,
  }
}

describe('KLineCharts core bridge', () => {
  beforeEach(() => {
    registry.clear()
    registryInit.mockClear()
  })

  it('recovers the already-registered core chart from the Pro widget', () => {
    const { host, widget } = hostFixture()
    const chart = fakeCoreChart()
    const init = vi.fn().mockReturnValue(chart)

    const result = recoverKLineChartsCore(host, {
      expectedVersion: '9.8.12',
      version: () => '9.8.12',
      init,
    })

    expect(result).toEqual({ ok: true, chart })
    expect(widget.id).toBe('k_line_chart_1')
    expect(init).toHaveBeenCalledOnce()
    expect(init).toHaveBeenCalledWith(widget)
  })

  it.each([
    ['wrong runtime', { version: () => '10.0.0' }],
    ['missing registry marker', { marker: null }],
    ['conflicting widget id', { widgetId: 'different' }],
    ['mismatched chart id', { returnedId: 'chart_2' }],
    ['missing required method', { removeMethod: 'createOverlay' as const }],
  ])('rejects %s without throwing', (_label, overrides) => {
    expect(() => recoverFixture(overrides)).not.toThrow()
    expect(recoverFixture(overrides).result.ok).toBe(false)
  })

  it('does not call init when the registry marker is absent', () => {
    const { result, init } = recoverFixture({ marker: null })

    expect(result).toMatchObject({ ok: false, code: 'marker' })
    expect(init).not.toHaveBeenCalled()
  })

  it('does not call init when the widget id conflicts with the registry marker', () => {
    const { result, init } = recoverFixture({ widgetId: 'different' })

    expect(result).toMatchObject({ ok: false })
    expect(init).not.toHaveBeenCalled()
  })

  it('requires exactly one Pro widget', () => {
    const host = document.createElement('div')
    const dependencies: CoreBridgeDependencies = {
      expectedVersion: '9.8.12',
      version: () => '9.8.12',
      init: vi.fn(),
    }

    expect(recoverKLineChartsCore(host, dependencies)).toMatchObject({ ok: false, code: 'widget' })
    host.append(hostFixture().widget, hostFixture().widget)
    expect(recoverKLineChartsCore(host, dependencies)).toMatchObject({ ok: false, code: 'widget' })
    expect(dependencies.init).not.toHaveBeenCalled()
  })

  it('checks the runtime version before reading the Pro DOM', () => {
    const host = document.createElement('div')
    Object.defineProperty(host, 'querySelectorAll', {
      value: vi.fn(() => { throw new Error('DOM touched') }),
    })

    const result = recoverKLineChartsCore(host, {
      expectedVersion: '9.8.12',
      version: () => '10.0.0',
      init: vi.fn(),
    })

    expect(result).toMatchObject({ ok: false, code: 'version' })
  })

  it('uses the deduplicated module init registry and returns the registered instance', () => {
    const { host, widget } = hostFixture()
    const registered = fakeCoreChart()
    const different = fakeCoreChart('new_chart')
    registry.set('k_line_chart_1', registered as Chart)
    registry.set('different', different as Chart)

    const result = recoverKLineChartsCore(host)

    expect(result).toEqual({ ok: true, chart: registered })
    expect(result.ok && result.chart).not.toBe(different)
    expect(registryInit).toHaveBeenCalledOnce()
    expect(registryInit).toHaveBeenCalledWith(widget)
  })
})
