/**
 * Format 24h change for display.
 * Values with |v| < 1 (and non-zero) are treated as ratios (0.034 -> +3.40%).
 */
export function formatChange24hPct(raw: string | undefined | null): string {
  const value = Number.parseFloat(raw ?? '0')
  if (!Number.isFinite(value)) {
    return '0.00%'
  }

  const pct = Math.abs(value) < 1 && value !== 0 ? value * 100 : value
  const sign = pct > 0 ? '+' : ''
  return `${sign}${pct.toFixed(2)}%`
}

export function change24hPctValue(raw: string | undefined | null): number {
  const value = Number.parseFloat(raw ?? '0')
  if (!Number.isFinite(value)) {
    return 0
  }
  return Math.abs(value) < 1 && value !== 0 ? value * 100 : value
}
