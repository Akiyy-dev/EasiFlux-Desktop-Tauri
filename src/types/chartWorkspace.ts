import type { Kline } from './models'

export type JsonValue = null | boolean | number | string | JsonValue[] | {
  [key: string]: JsonValue
}

export interface ChartWorkspaceKey {
  symbol: string
  interval: string
}

export type ChartPaneRef =
  | { kind: 'candle' }
  | { kind: 'indicator'; indicatorName: string }

export interface ChartPointSnapshot {
  timestamp?: number
  dataIndex?: number
  value?: number
}

export interface ChartOverlaySnapshot {
  id: string
  groupId: string
  pane: ChartPaneRef
  name: string
  lock: boolean
  visible: boolean
  zLevel: number
  mode: string
  modeSensitivity: number
  points: ChartPointSnapshot[]
  extendData: JsonValue
  styles: JsonValue
}

export interface ChartViewportSnapshot {
  barSpace?: number
  rightTimestamp?: number
}

export interface ChartViewStateV1 {
  schemaVersion: number
  symbol: string
  interval: string
  revision: number
  savedAtMs: number
  overlays: ChartOverlaySnapshot[]
  viewport: ChartViewportSnapshot
}

export interface ChartPreferencesV1 {
  schemaVersion: number
  revision: number
  savedAtMs: number
  mainIndicators: string[]
  subIndicators: string[]
}

export interface ChartWorkspaceSnapshot {
  key: ChartWorkspaceKey
  klines: Kline[]
  viewState: ChartViewStateV1
  preferences: ChartPreferencesV1
}

export interface SaveChartWorkspaceRequest {
  key: ChartWorkspaceKey
  viewState: ChartViewStateV1 | null
  preferences: ChartPreferencesV1 | null
}

export interface ChartWorkspaceSaveResult {
  key: ChartWorkspaceKey
  viewRevision: number
  preferencesRevision: number
  savedAtMs: number
  klineSaved: boolean
  viewStateSaved: boolean
  preferencesSaved: boolean
  klineError: string | null
  viewStateError: string | null
  preferencesError: string | null
}
