export const ACCOUNT_SWITCHING_MESSAGE = 'Account switch in progress. Try again shortly.'
export const ACCOUNT_SYNCING_MESSAGE =
  'Account synchronization in progress. Trading is temporarily paused.'
export const ACCOUNT_SYNC_FAILED_MESSAGE =
  'Trading paused because account synchronization failed. Retry synchronization.'

export function assertTradingMutationAllowed(blockedMessage: string | null): void {
  if (blockedMessage) {
    throw new Error(blockedMessage)
  }
}
