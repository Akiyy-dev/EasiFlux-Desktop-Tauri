export const ACCOUNT_SWITCHING_MESSAGE = '账户切换中，请稍候重试。'
export const ACCOUNT_SYNCING_MESSAGE =
  '账户同步中，交易暂时暂停。'
export const ACCOUNT_SYNC_FAILED_MESSAGE =
  '账户同步失败，交易已暂停。请重试同步。'

export function assertTradingMutationAllowed(blockedMessage: string | null): void {
  if (blockedMessage) {
    throw new Error(blockedMessage)
  }
}
