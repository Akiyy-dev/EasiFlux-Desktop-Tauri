import type { Balance, Order, Position } from './models'

export const PLUGIN_WORKFLOW_CAPABILITIES = [
  'account.read',
  'balances.read',
  'positions.read',
  'orders.read',
  'market.read',
  'trade.place',
  'trade.cancel',
] as const

export type PluginWorkflowCapability = typeof PLUGIN_WORKFLOW_CAPABILITIES[number]

export interface PluginWorkflowAccount {
  accountId: string
  sessionEpoch: string
  environment: string
}

export interface PluginWorkflowAccess {
  schemaVersion: 1
  pluginId: string
  contributionId: string
  catalogGeneration: string
  revision: string
  account: PluginWorkflowAccount
  requestedCapabilities: PluginWorkflowCapability[]
  grantedCapabilities: PluginWorkflowCapability[]
  grantRevision: string
}

export interface PluginWorkflowSection<T> {
  items: T[]
  fetchedAtMs: string
  partial: boolean
}

export interface PluginWorkflowTicker {
  symbol: string
  lastPrice: string
  bidPrice: string
  askPrice: string
  markPrice: string
}

export interface PluginWorkflowMarketSection {
  ticker: PluginWorkflowTicker
  fetchedAtMs: string
}

export interface PluginWorkflowSnapshot {
  schemaVersion: 1
  account: PluginWorkflowAccount
  capturedAtMs: string
  symbol: string
  grantedCapabilities: PluginWorkflowCapability[]
  balances: PluginWorkflowSection<Balance> | null
  positions: PluginWorkflowSection<Position> | null
  orders: PluginWorkflowSection<Order> | null
  market: PluginWorkflowMarketSection | null
}

export interface PluginWorkflowPlaceOrder {
  symbol: string
  side: 'Buy' | 'Sell'
  orderType: 'Market' | 'Limit'
  qty: string
  price: string | null
  timeInForce: 'GTC' | 'IOC' | 'FOK'
  positionIdx: 1 | 2
  reduceOnly: boolean
}

export interface PluginWorkflowCancelOrder {
  symbol: string
  orderId: string
}

export type PluginWorkflowOutput =
  | { kind: 'display'; text: string }
  | { kind: 'placeOrder'; order: PluginWorkflowPlaceOrder }
  | { kind: 'cancelOrder'; order: PluginWorkflowCancelOrder }

export interface PluginWorkflowConfirmation {
  token: string
  expiresAtMs: string
  submissionId: string | null
}

export interface PluginWorkflowResult {
  schemaVersion: 1
  requestId: string
  pluginId: string
  contributionId: string
  catalogGeneration: string
  revision: string
  account: PluginWorkflowAccount
  grantRevision: string
  snapshot: PluginWorkflowSnapshot
  output: PluginWorkflowOutput
  confirmation: PluginWorkflowConfirmation | null
}

export type PluginWorkflowReceiptStatus = 'accepted' | 'rejected' | 'unknown'

export interface PluginWorkflowTradeReceipt {
  schemaVersion: 1
  token: string
  account: PluginWorkflowAccount
  action: 'placeOrder' | 'cancelOrder'
  status: PluginWorkflowReceiptStatus
  submissionId: string | null
  order: Order | null
  errorCode: string | null
}

export interface PluginWorkflowRunInput {
  requestId: string
  symbol: string
  inputJson: string
}
