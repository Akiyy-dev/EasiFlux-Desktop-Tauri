export function normalizeEpochMs(value: number): number | null {
  if (!Number.isFinite(value) || value <= 0) {
    return null
  }
  if (value >= 1_000_000_000_000) {
    return Math.round(value)
  }
  if (value >= 1_000_000_000) {
    return Math.round(value * 1000)
  }
  return null
}

export function formatClockTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString()
}

export function formatDateTime(epochMs: number): string {
  return new Date(epochMs).toLocaleString()
}
