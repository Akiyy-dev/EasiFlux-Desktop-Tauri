import type { PluginWorkflowAccount, PluginWorkflowCapability } from './pluginWorkflow'

export const PLUGIN_STRATEGY_CAPABILITIES = [
  'account.read',
  'balances.read',
  'positions.read',
  'orders.read',
  'market.read',
  'trade.place',
  'trade.cancel',
  'strategy.run',
] as const

export type PluginStrategyCapability =
  | PluginWorkflowCapability
  | 'strategy.run'

export interface StrategyAccess {
  schemaVersion: 1
  pluginId: string
  contributionId: string
  catalogGeneration: string
  revision: string
  account: PluginWorkflowAccount
  requestedCapabilities: PluginStrategyCapability[]
  authorizationToken: string
  expiresAtMs: string
}

export interface StrategyPolicy {
  intervalMs: number
  maxOrderQty: string
  maxTotalQty: string
  maxActions: number
  maxRunSeconds: number
  reduceOnly: boolean
}

export interface StrategyStartRequest {
  authorizationToken: string
  requestId: string
  resumeRunId: string | null
  symbol: string
  inputJson: string
  capabilities: PluginStrategyCapability[]
  policy: StrategyPolicy
  acknowledgeAutomaticTrading: true
}

export type StrategyRunStatus =
  | 'running'
  | 'paused'
  | 'stopping'
  | 'stopped'
  | 'recoveryRequired'
  | 'faulted'
  | 'completed'

export type StrategyRunReason =
  | 'plugin_strategy_paused'
  | 'plugin_strategy_stopped'
  | 'plugin_strategy_restarted'
  | 'plugin_strategy_expired'
  | 'plugin_strategy_limit_reached'
  | 'plugin_strategy_stale'
  | 'plugin_strategy_data_unavailable'
  | 'plugin_strategy_compute_failed'
  | 'plugin_strategy_invalid_output'
  | 'plugin_strategy_recovery_required'
  | 'plugin_strategy_storage_unavailable'
  | 'plugin_strategy_ack_failed'
  | 'plugin_strategy_unavailable'

export interface StrategyReceipt {
  sequence: string
  kind: 'placeOrder' | 'cancelOrder'
  status: 'accepted' | 'rejected' | 'unknown'
  submissionId: string | null
  orderId: string | null
  errorCode: 'plugin_strategy_rejected' | 'plugin_strategy_unknown' | null
}

export interface StrategyRunView {
  schemaVersion: 1
  runId: string
  requestId: string
  pluginId: string
  contributionId: string
  account: PluginWorkflowAccount
  symbol: string
  status: StrategyRunStatus
  reason: StrategyRunReason | null
  policy: StrategyPolicy
  capabilities: PluginStrategyCapability[]
  inputJson: string
  startedAtMs: string
  expiresAtMs: string
  sequence: string
  actionsSubmitted: number
  totalSubmittedQty: string
  lastMessage: string
  lastReceipt: StrategyReceipt | null
}

export interface StrategyRunList {
  schemaVersion: 1
  runs: StrategyRunView[]
}

export type PluginStrategyStartInput = Omit<StrategyStartRequest, 'authorizationToken'>
