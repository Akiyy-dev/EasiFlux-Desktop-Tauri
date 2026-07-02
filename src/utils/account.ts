/** Trim and fall back to `default` when account id is empty. */
export function normalizeAccountId(accountId: string | undefined | null): string {
  const trimmed = accountId?.trim() ?? ''
  return trimmed.length > 0 ? trimmed : 'default'
}
