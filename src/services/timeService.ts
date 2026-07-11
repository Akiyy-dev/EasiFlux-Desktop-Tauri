import { tauriInvoke } from '../composables/useTauriCommand'
import type { TimeSnapshot, TimeSource, TimeSyncStatus } from '../types/models'

const SYNC_STATUS_VALUES: TimeSyncStatus[] = ['syncing', 'synced', 'failed', 'localFallback']
const SOURCE_VALUES: TimeSource[] = ['server', 'local']

let started = false
let timer: ReturnType<typeof globalThis.setInterval> | null = null
const listeners = new Set<(snapshot: TimeSnapshot) => void>()

let snapshot: TimeSnapshot = {
  serverTimeMs: Date.now(),
  localTimeMs: Date.now(),
  offsetMs: 0,
  syncStatus: 'localFallback',
  source: 'local',
  lastSyncAt: null,
  lastAttemptAt: null,
  lastError: null,
}

let anchorServerMs = snapshot.serverTimeMs
let anchorPerfMs = performance.now()

function isSyncStatus(value: unknown): value is TimeSyncStatus {
  return typeof value === 'string' && SYNC_STATUS_VALUES.includes(value as TimeSyncStatus)
}

function isSource(value: unknown): value is TimeSource {
  return typeof value === 'string' && SOURCE_VALUES.includes(value as TimeSource)
}

export function normalizeTimeSnapshot(raw: Partial<TimeSnapshot> | null | undefined): TimeSnapshot {
  const localTimeMs = raw?.localTimeMs ?? Date.now()
  const serverTimeMs = raw?.serverTimeMs ?? localTimeMs
  return {
    serverTimeMs,
    localTimeMs,
    offsetMs: raw?.offsetMs ?? 0,
    syncStatus: isSyncStatus(raw?.syncStatus) ? raw.syncStatus : 'localFallback',
    source: isSource(raw?.source) ? raw.source : 'local',
    lastSyncAt: raw?.lastSyncAt ?? null,
    lastAttemptAt: raw?.lastAttemptAt ?? null,
    lastError: raw?.lastError ?? null,
  }
}

function notify(): void {
  for (const listener of listeners) {
    listener(snapshot)
  }
}

function applySnapshot(next: TimeSnapshot): void {
  snapshot = normalizeTimeSnapshot(next)
  anchorServerMs = snapshot.serverTimeMs
  anchorPerfMs = performance.now()
  notify()
}

export function getTimeSnapshot(): TimeSnapshot {
  return snapshot
}

export function serverNowMs(): number {
  return Math.max(0, Math.round(anchorServerMs + (performance.now() - anchorPerfMs)))
}

export function localNowMs(): number {
  return Date.now()
}

export function subscribeTimeSnapshot(listener: (value: TimeSnapshot) => void): () => void {
  listeners.add(listener)
  listener(snapshot)
  return () => {
    listeners.delete(listener)
  }
}

export function onTimeUpdated(next: TimeSnapshot): void {
  applySnapshot(next)
}

export async function refreshTimeSnapshot(): Promise<TimeSnapshot> {
  const result = await tauriInvoke<TimeSnapshot>('get_time_snapshot')
  applySnapshot(result)
  return snapshot
}

export async function syncTimeNow(): Promise<TimeSnapshot> {
  const result = await tauriInvoke<TimeSnapshot>('sync_time_now')
  applySnapshot(result)
  return snapshot
}

export function startTimeService(): void {
  if (started) {
    return
  }
  started = true
  void refreshTimeSnapshot().catch(() => {
    applySnapshot({
      ...snapshot,
      syncStatus: 'localFallback',
      source: 'local',
      lastError: '时间快照加载失败',
    })
  })
  timer = globalThis.setInterval(() => {
    notify()
  }, 1000)
}

export function stopTimeService(): void {
  if (timer) {
    globalThis.clearInterval(timer)
    timer = null
  }
  started = false
}

export function timeServiceRunning(): boolean {
  return started
}
