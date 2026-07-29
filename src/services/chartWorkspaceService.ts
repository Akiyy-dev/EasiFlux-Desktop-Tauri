import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  ChartWorkspaceKey,
  ChartWorkspaceSaveResult,
  ChartWorkspaceSnapshot,
  SaveChartWorkspaceRequest,
} from '../types/chartWorkspace'
import type { Kline } from '../types/models'

export interface ChartHistoryRange {
  from?: number
  to?: number
  limit?: number
}

export function loadChartWorkspace(
  key: ChartWorkspaceKey,
  range?: ChartHistoryRange,
): Promise<ChartWorkspaceSnapshot> {
  return tauriInvoke<ChartWorkspaceSnapshot>('load_chart_workspace', {
    symbol: key.symbol,
    interval: key.interval,
    ...(range?.from === undefined ? {} : { from: range.from }),
    ...(range?.to === undefined ? {} : { to: range.to }),
    ...(range?.limit === undefined ? {} : { limit: range.limit }),
  })
}

export function saveChartWorkspace(
  request: SaveChartWorkspaceRequest,
): Promise<ChartWorkspaceSaveResult> {
  return tauriInvoke<ChartWorkspaceSaveResult>('save_chart_workspace', { request })
}

export function refreshChartKlines(
  key: ChartWorkspaceKey,
  range: Required<ChartHistoryRange>,
): Promise<Kline[]> {
  return tauriInvoke<Kline[]>('fetch_klines', {
    symbol: key.symbol,
    interval: key.interval,
    start: range.from,
    end: range.to,
    limit: range.limit,
  })
}
