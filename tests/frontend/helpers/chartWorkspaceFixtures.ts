import type {
  ChartOverlaySnapshot,
  ChartPreferencesV1,
  ChartWorkspaceKey,
  ChartWorkspaceSaveResult,
  ChartWorkspaceSnapshot,
  ChartViewStateV1,
  SaveChartWorkspaceRequest,
} from '../../../src/types/chartWorkspace'
import type { Kline } from '../../../src/types/models'

export const key = (symbol = 'BTCUSDT', interval = '1'): ChartWorkspaceKey => ({
  symbol,
  interval,
})

export const bar = (openTime: number, symbol = 'BTCUSDT', interval = '1'): Kline => ({
  symbol,
  interval,
  openTime,
  open: '1',
  high: '2',
  low: '0.5',
  close: '1.5',
  volume: '10',
})

export const segment = (id = 'segment-1'): ChartOverlaySnapshot => ({
  id,
  groupId: 'group-1',
  pane: { kind: 'candle' },
  name: 'segment',
  lock: false,
  visible: true,
  zLevel: 0,
  mode: 'normal',
  modeSensitivity: 8,
  points: [{ timestamp: 1_000, value: 1 }],
  extendData: null,
  styles: null,
})

export function viewStateFor(
  workspaceKey: ChartWorkspaceKey,
  revision = 0,
  overlays: ChartOverlaySnapshot[] = [],
): ChartViewStateV1 {
  return {
    schemaVersion: 1,
    symbol: workspaceKey.symbol,
    interval: workspaceKey.interval,
    revision,
    savedAtMs: 0,
    overlays,
    viewport: {},
  }
}

export const viewState = (revision = 0, overlays: ChartOverlaySnapshot[] = []) =>
  viewStateFor(key(), revision, overlays)

export const samplePreferences = (revision = 0): ChartPreferencesV1 => ({
  schemaVersion: 1,
  revision,
  savedAtMs: 0,
  mainIndicators: ['MA', 'EMA'],
  subIndicators: ['VOL', 'MACD'],
})

export const sampleSnapshot = (
  klines: Kline[] = [],
  workspaceKey = key(),
): ChartWorkspaceSnapshot => ({
  key: workspaceKey,
  klines,
  viewState: viewStateFor(workspaceKey),
  preferences: samplePreferences(),
})

export const sampleSaveRequest = (
  patch: Partial<SaveChartWorkspaceRequest> = {},
): SaveChartWorkspaceRequest => ({
  key: key(),
  viewState: viewState(),
  preferences: samplePreferences(),
  ...patch,
})

export const successfulSave = (
  viewRevision = 0,
  preferencesRevision = 0,
): ChartWorkspaceSaveResult => ({
  key: key(),
  viewRevision,
  preferencesRevision,
  savedAtMs: 1_000,
  klineSaved: true,
  viewStateSaved: true,
  preferencesSaved: true,
  klineError: null,
  viewStateError: null,
  preferencesError: null,
})

export function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  let settled = false
  const promise = new Promise<T>((res, rej) => {
    resolve = (value) => { settled = true; res(value) }
    reject = (reason) => { settled = true; rej(reason) }
  })
  return { promise, resolve, reject, get settled() { return settled } }
}
