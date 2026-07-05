/**
 * Format normalized 24h percent change for display.
 * Backend stores decimal percent (e.g. 0.1829 = +0.18%).
 */
export function formatChange24hPct(raw: string | undefined | null): string {
  const pct = Number.parseFloat(raw ?? '0')
  if (!Number.isFinite(pct)) {
    return '0.00%'
  }
  const sign = pct > 0 ? '+' : ''
  return `${sign}${pct.toFixed(2)}%`
}

export function change24hPctValue(raw: string | undefined | null): number {
  const value = Number.parseFloat(raw ?? '0')
  if (!Number.isFinite(value)) {
    return 0
  }
  return value
}
