import {
  init as klineChartsInit,
  version as klineChartsVersion,
  type Chart,
} from 'klinecharts'

export interface CoreBridgeDependencies {
  expectedVersion: '9.8.12'
  version: () => string
  init: (container: HTMLElement) => Chart | null
}

export type WorkspaceCoreChart = Pick<Chart,
  | 'id'
  | 'applyNewData'
  | 'createOverlay'
  | 'getOverlayById'
  | 'overrideOverlay'
  | 'removeOverlay'
  | 'createIndicator'
  | 'getIndicatorByPaneId'
  | 'overrideIndicator'
  | 'removeIndicator'
  | 'getBarSpace'
  | 'setBarSpace'
  | 'getVisibleRange'
  | 'getDataList'
  | 'scrollToTimestamp'
  | 'subscribeAction'
  | 'unsubscribeAction'
  | 'resize'
>

export type CoreBridgeResult =
  | { ok: true; chart: WorkspaceCoreChart }
  | {
    ok: false
    code: 'version' | 'widget' | 'marker' | 'instance' | 'shape'
    message: string
  }

type CoreBridgeFailureCode = Extract<CoreBridgeResult, { ok: false }>['code']

const DEFAULT_DEPENDENCIES: CoreBridgeDependencies = {
  expectedVersion: '9.8.12',
  version: klineChartsVersion,
  init: klineChartsInit,
}

const REQUIRED_METHODS: ReadonlyArray<Exclude<keyof WorkspaceCoreChart, 'id'>> = [
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
]

function failure(code: CoreBridgeFailureCode, message: string): CoreBridgeResult {
  return { ok: false, code, message }
}

export function recoverKLineChartsCore(
  proHost: HTMLElement,
  dependencies: CoreBridgeDependencies = DEFAULT_DEPENDENCIES,
): CoreBridgeResult {
  let runtimeVersion: string
  try {
    runtimeVersion = dependencies.version()
  } catch {
    return failure('version', 'Unable to read the KLineCharts runtime version')
  }
  if (runtimeVersion !== dependencies.expectedVersion) {
    return failure(
      'version',
      `Unsupported KLineCharts runtime ${runtimeVersion}; expected ${dependencies.expectedVersion}`,
    )
  }

  let widgets: NodeListOf<Element>
  try {
    widgets = proHost.querySelectorAll('.klinecharts-pro-widget')
  } catch {
    return failure('widget', 'Unable to inspect the KLineCharts Pro widget')
  }
  if (widgets.length !== 1 || !(widgets[0] instanceof HTMLElement)) {
    return failure('widget', 'Expected exactly one KLineCharts Pro widget')
  }
  const widget = widgets[0]
  const marker = widget.getAttribute('k-line-chart-id')?.trim()
  if (!marker) {
    return failure('marker', 'KLineCharts Pro widget has no registry marker')
  }
  if (widget.id && widget.id !== marker) {
    return failure('marker', 'KLineCharts Pro widget id conflicts with its registry marker')
  }
  if (!widget.id) {
    widget.id = marker
  }

  let chart: Chart | null
  try {
    chart = dependencies.init(widget)
  } catch {
    return failure('instance', 'KLineCharts registry lookup failed')
  }
  if (!chart || chart.id !== marker) {
    return failure('instance', 'KLineCharts registry returned a different chart instance')
  }

  const candidate = chart as unknown as Record<PropertyKey, unknown>
  const missingMethod = REQUIRED_METHODS.find((method) => typeof candidate[method] !== 'function')
  if (missingMethod) {
    return failure('shape', `KLineCharts chart is missing required method ${missingMethod}`)
  }

  return { ok: true, chart }
}
