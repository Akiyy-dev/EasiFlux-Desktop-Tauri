export type AccountSessionEpochDecision = 'reject' | 'accept' | 'advance'

export function decideAccountSessionEpoch(
  currentEpoch: number,
  incomingEpoch: number,
): AccountSessionEpochDecision {
  if (incomingEpoch < currentEpoch) return 'reject'
  if (incomingEpoch > currentEpoch) return 'advance'
  return 'accept'
}
