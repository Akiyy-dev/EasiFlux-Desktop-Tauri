import { tauriInvoke } from '../composables/useTauriCommand'
import type { PluginWorkflowExecutionIntent } from '../types/plugin'
import {
  PLUGIN_WORKFLOW_CAPABILITIES,
  type PluginWorkflowAccess,
  type PluginWorkflowAccount,
  type PluginWorkflowCapability,
  type PluginWorkflowConfirmation,
  type PluginWorkflowOutput,
  type PluginWorkflowResult,
  type PluginWorkflowRunInput,
  type PluginWorkflowSection,
  type PluginWorkflowSnapshot,
  type PluginWorkflowTradeReceipt,
} from '../types/pluginWorkflow'
import type { Balance, Order, OrderStatus, Position } from '../types/models'

const INVALID_RESPONSE_ERROR = '插件工作流服务返回的数据无效，请重试。'
const GENERIC_ERROR = '插件工作流操作失败，请重试。'
const U64_MAX = 18_446_744_073_709_551_615n
const textEncoder = new TextEncoder()
const ACCOUNT_KEYS = ['accountId', 'sessionEpoch', 'environment'] as const
const ACCESS_KEYS = [
  'schemaVersion', 'pluginId', 'contributionId', 'catalogGeneration', 'revision',
  'account', 'requestedCapabilities', 'grantedCapabilities', 'grantRevision',
] as const
const RESULT_KEYS = [
  'schemaVersion', 'requestId', 'pluginId', 'contributionId', 'catalogGeneration',
  'revision', 'account', 'grantRevision', 'snapshot', 'output', 'confirmation',
] as const
const SNAPSHOT_KEYS = [
  'schemaVersion', 'account', 'capturedAtMs', 'symbol', 'grantedCapabilities',
  'balances', 'positions', 'orders', 'market',
] as const
const SECTION_KEYS = ['items', 'fetchedAtMs', 'partial'] as const
const MARKET_KEYS = ['ticker', 'fetchedAtMs'] as const
const TICKER_KEYS = ['symbol', 'lastPrice', 'bidPrice', 'askPrice', 'markPrice'] as const
const BALANCE_KEYS = ['asset', 'available', 'frozen', 'total'] as const
const POSITION_KEYS = [
  'symbol', 'side', 'size', 'entryPrice', 'leverage', 'unrealisedPnl', 'positionIdx',
] as const
const ORDER_KEYS = [
  'orderId', 'symbol', 'side', 'orderType', 'price', 'qty', 'status', 'orderLinkId',
  'filledQty', 'avgPrice',
] as const
const CONFIRMATION_KEYS = ['token', 'expiresAtMs', 'submissionId'] as const
const RECEIPT_KEYS = [
  'schemaVersion', 'token', 'account', 'action', 'status', 'submissionId', 'order', 'errorCode',
] as const

const RECEIPT_ERROR_MESSAGES: Readonly<Record<string, string>> = {
  plugin_workflow_token_invalid: '交易确认已过期、已使用或已失效。',
  plugin_workflow_stale: '账户、权限或插件状态已变化。',
  plugin_workflow_rejected: '交易请求被拒绝。',
  plugin_workflow_unknown: '交易结果尚不确定。',
}
const CONFIRMATION_NOT_SUBMITTED_CODES = new Set([
  'plugin_workflow_invalid_request',
  'plugin_workflow_token_invalid',
  'plugin_workflow_stale',
])

type WireObject = Record<string, unknown>

function invalidResponse(): never {
  throw new Error(INVALID_RESPONSE_ERROR)
}

function isObject(value: unknown): value is WireObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function exactObject(value: unknown, keys: readonly string[]): WireObject {
  if (!isObject(value)) invalidResponse()
  const actual = Object.keys(value)
  if (actual.length !== keys.length
    || keys.some((key) => !Object.prototype.hasOwnProperty.call(value, key))) {
    invalidResponse()
  }
  return value
}

function boundedString(value: unknown, maxBytes: number, allowEmpty = false): string {
  if (typeof value !== 'string'
    || (!allowEmpty && value.length === 0)
    || value !== value.trim()
    || textEncoder.encode(value).byteLength > maxBytes
    || [...value].some((character) => {
      const codePoint = character.codePointAt(0) ?? 0
      return codePoint <= 31
        || (codePoint >= 127 && codePoint <= 159)
        || codePoint === 0xfeff
    })) invalidResponse()
  return value
}

function plainText(value: unknown, maxBytes: number): string {
  if (typeof value !== 'string' || textEncoder.encode(value).byteLength > maxBytes) {
    invalidResponse()
  }
  return value
}

function pluginId(value: unknown): string {
  const id = boundedString(value, 128)
  const segments = id.split('.')
  if (segments.length < 2 || segments.some((segment) => (
    segment.length === 0
    || textEncoder.encode(segment).byteLength > 63
    || !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(segment)
  ))) invalidResponse()
  return id
}

function canonicalCounter(value: unknown): string {
  if (typeof value !== 'string' || value.length > 20
    || !/^(?:0|[1-9][0-9]*)$/.test(value)
    || BigInt(value) > U64_MAX) invalidResponse()
  return value
}

function token(value: unknown): string {
  return boundedString(value, 256)
}

function requestId(value: unknown): string {
  const parsed = boundedString(value, 64)
  if (!/^[A-Za-z0-9-]+$/.test(parsed)) invalidResponse()
  return parsed
}

function symbol(value: unknown): string {
  if (typeof value !== 'string' || !/^[A-Z0-9]{1,32}$/.test(value)) invalidResponse()
  return value
}

function decimal(value: unknown, positive = false): string {
  if (typeof value !== 'string' || textEncoder.encode(value).byteLength > 64
    || !/^-?[0-9]+(?:\.[0-9]+)?$/.test(value)) invalidResponse()
  if (positive && (value.startsWith('-') || /^0+(?:\.0+)?$/.test(value))) invalidResponse()
  return value
}

function parseAccount(value: unknown): PluginWorkflowAccount {
  const account = exactObject(value, ACCOUNT_KEYS)
  return {
    accountId: boundedString(account.accountId, 128),
    sessionEpoch: canonicalCounter(account.sessionEpoch),
    environment: boundedString(account.environment, 256),
  }
}

function sameAccount(left: PluginWorkflowAccount, right: PluginWorkflowAccount): boolean {
  return left.accountId === right.accountId
    && left.sessionEpoch === right.sessionEpoch
    && left.environment === right.environment
}

function parseCapabilities(value: unknown): PluginWorkflowCapability[] {
  if (!Array.isArray(value) || value.length > PLUGIN_WORKFLOW_CAPABILITIES.length) {
    invalidResponse()
  }
  const parsed = value.map((candidate) => {
    if (typeof candidate !== 'string'
      || !PLUGIN_WORKFLOW_CAPABILITIES.includes(candidate as PluginWorkflowCapability)) {
      return invalidResponse()
    }
    return candidate as PluginWorkflowCapability
  })
  if (new Set(parsed).size !== parsed.length) invalidResponse()
  return parsed
}

function sameCapabilities(
  left: readonly PluginWorkflowCapability[],
  right: readonly PluginWorkflowCapability[],
): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}

function validateGrantedCapabilities(
  requested: readonly PluginWorkflowCapability[],
  granted: readonly PluginWorkflowCapability[],
): void {
  if (granted.some((capability) => !requested.includes(capability))) invalidResponse()
  if (granted.length > 0 && !granted.includes('account.read')) invalidResponse()
}

export function parseWorkflowAccess(
  value: unknown,
  intent: PluginWorkflowExecutionIntent,
): PluginWorkflowAccess {
  const access = exactObject(value, ACCESS_KEYS)
  const requestedCapabilities = parseCapabilities(access.requestedCapabilities)
  const grantedCapabilities = parseCapabilities(access.grantedCapabilities)
  if (access.schemaVersion !== 1
    || pluginId(access.pluginId) !== intent.pluginId
    || pluginId(access.contributionId) !== intent.contributionId
    || canonicalCounter(access.catalogGeneration) !== intent.expectedCatalogGeneration
    || canonicalCounter(access.revision) !== intent.expectedRevision
    || !sameCapabilities(requestedCapabilities, intent.requestedCapabilities)) invalidResponse()
  validateGrantedCapabilities(requestedCapabilities, grantedCapabilities)
  return {
    schemaVersion: 1,
    pluginId: intent.pluginId,
    contributionId: intent.contributionId,
    catalogGeneration: intent.expectedCatalogGeneration,
    revision: intent.expectedRevision,
    account: parseAccount(access.account),
    requestedCapabilities,
    grantedCapabilities,
    grantRevision: canonicalCounter(access.grantRevision),
  }
}

function authorityRequest(intent: PluginWorkflowExecutionIntent) {
  pluginId(intent.pluginId)
  pluginId(intent.contributionId)
  canonicalCounter(intent.expectedCatalogGeneration)
  canonicalCounter(intent.expectedRevision)
  return {
    pluginId: intent.pluginId,
    contributionId: intent.contributionId,
    expectedCatalogGeneration: intent.expectedCatalogGeneration,
    expectedRevision: intent.expectedRevision,
  }
}

export async function getPluginWorkflowAccess(
  intent: PluginWorkflowExecutionIntent,
): Promise<PluginWorkflowAccess> {
  const value = await tauriInvoke<unknown>('get_plugin_workflow_access', {
    request: authorityRequest(intent),
  })
  return parseWorkflowAccess(value, intent)
}

export async function setPluginWorkflowGrants(
  intent: PluginWorkflowExecutionIntent,
  access: PluginWorkflowAccess,
  capabilities: readonly PluginWorkflowCapability[],
): Promise<PluginWorkflowAccess> {
  const requested = parseCapabilities([...capabilities])
  validateGrantedCapabilities(access.requestedCapabilities, requested)
  const value = await tauriInvoke<unknown>('set_plugin_workflow_grants', {
    request: {
      ...authorityRequest(intent),
      expectedAccountId: access.account.accountId,
      expectedSessionEpoch: access.account.sessionEpoch,
      expectedGrantRevision: access.grantRevision,
      capabilities: [...requested],
    },
  })
  const parsed = parseWorkflowAccess(value, intent)
  if (!sameAccount(parsed.account, access.account)
    || !sameCapabilities(parsed.grantedCapabilities, requested)) invalidResponse()
  return parsed
}

function parseBalance(value: unknown): Balance {
  const balance = exactObject(value, BALANCE_KEYS)
  return {
    asset: symbol(balance.asset),
    available: decimal(balance.available),
    frozen: decimal(balance.frozen),
    total: decimal(balance.total),
  }
}

function parsePosition(value: unknown, expectedSymbol: string): Position {
  const position = exactObject(value, POSITION_KEYS)
  const parsedSymbol = symbol(position.symbol)
  if (parsedSymbol !== expectedSymbol || !Number.isInteger(position.positionIdx)
    || (position.positionIdx as number) < 0 || (position.positionIdx as number) > 2
    || (position.side !== 'Buy' && position.side !== 'Sell')) {
    invalidResponse()
  }
  return {
    symbol: parsedSymbol,
    side: position.side,
    size: decimal(position.size),
    entryPrice: decimal(position.entryPrice),
    leverage: decimal(position.leverage),
    unrealisedPnl: decimal(position.unrealisedPnl),
    positionIdx: position.positionIdx as number,
  }
}

const ORDER_STATUSES = new Set<OrderStatus>([
  'New', 'PartiallyFilled', 'Filled', 'Cancelled', 'Rejected', 'Unknown',
])

function parseOrder(value: unknown, expectedSymbol?: string): Order {
  const order = exactObject(value, ORDER_KEYS)
  const parsedSymbol = symbol(order.symbol)
  if (expectedSymbol !== undefined && parsedSymbol !== expectedSymbol) invalidResponse()
  if (typeof order.status !== 'string' || !ORDER_STATUSES.has(order.status as OrderStatus)
    || (order.side !== 'Buy' && order.side !== 'Sell')
    || (order.orderType !== 'Market' && order.orderType !== 'Limit')) {
    invalidResponse()
  }
  const orderLinkId = order.orderLinkId === null
    ? undefined
    : boundedString(order.orderLinkId, 128, true)
  return {
    orderId: boundedString(order.orderId, 128),
    symbol: parsedSymbol,
    side: order.side,
    orderType: order.orderType,
    price: decimal(order.price),
    qty: decimal(order.qty),
    status: order.status as OrderStatus,
    ...(orderLinkId === undefined ? {} : { orderLinkId }),
    filledQty: decimal(order.filledQty),
    avgPrice: decimal(order.avgPrice),
  }
}

function parseSection<T>(
  value: unknown,
  capturedAtMs: string,
  parseItem: (item: unknown) => T,
): PluginWorkflowSection<T> {
  const section = exactObject(value, SECTION_KEYS)
  if (!Array.isArray(section.items) || section.items.length > 100 || section.partial !== true) {
    invalidResponse()
  }
  const fetchedAtMs = canonicalCounter(section.fetchedAtMs)
  if (BigInt(fetchedAtMs) > BigInt(capturedAtMs)) invalidResponse()
  return { items: section.items.map(parseItem), fetchedAtMs, partial: true }
}

function parseSnapshot(
  value: unknown,
  access: PluginWorkflowAccess,
  expectedSymbol: string,
): PluginWorkflowSnapshot {
  const snapshot = exactObject(value, SNAPSHOT_KEYS)
  const capturedAtMs = canonicalCounter(snapshot.capturedAtMs)
  const account = parseAccount(snapshot.account)
  const parsedSymbol = symbol(snapshot.symbol)
  const grantedCapabilities = parseCapabilities(snapshot.grantedCapabilities)
  if (snapshot.schemaVersion !== 1 || !sameAccount(account, access.account)
    || parsedSymbol !== expectedSymbol
    || !sameCapabilities(grantedCapabilities, access.grantedCapabilities)) invalidResponse()

  const section = <T>(
    capability: PluginWorkflowCapability,
    candidate: unknown,
    parser: (item: unknown) => T,
  ): PluginWorkflowSection<T> | null => {
    const granted = grantedCapabilities.includes(capability)
    if (!granted) {
      if (candidate !== null) invalidResponse()
      return null
    }
    if (candidate === null) invalidResponse()
    return parseSection(candidate, capturedAtMs, parser)
  }
  const balances = section('balances.read', snapshot.balances, parseBalance)
  const positions = section(
    'positions.read', snapshot.positions,
    (item) => parsePosition(item, parsedSymbol),
  )
  const orders = section('orders.read', snapshot.orders, (item) => parseOrder(item, parsedSymbol))

  let market = null
  if (grantedCapabilities.includes('market.read')) {
    const parsedMarket = exactObject(snapshot.market, MARKET_KEYS)
    const tickerValue = exactObject(parsedMarket.ticker, TICKER_KEYS)
    const tickerSymbol = symbol(tickerValue.symbol)
    const fetchedAtMs = canonicalCounter(parsedMarket.fetchedAtMs)
    if (tickerSymbol !== parsedSymbol || BigInt(fetchedAtMs) > BigInt(capturedAtMs)) {
      invalidResponse()
    }
    market = {
      ticker: {
        symbol: tickerSymbol,
        lastPrice: decimal(tickerValue.lastPrice),
        bidPrice: decimal(tickerValue.bidPrice),
        askPrice: decimal(tickerValue.askPrice),
        markPrice: decimal(tickerValue.markPrice),
      },
      fetchedAtMs,
    }
  } else if (snapshot.market !== null) invalidResponse()

  return {
    schemaVersion: 1, account, capturedAtMs, symbol: parsedSymbol,
    grantedCapabilities, balances, positions, orders, market,
  }
}

function parseOutput(value: unknown, snapshot: PluginWorkflowSnapshot): PluginWorkflowOutput {
  if (!isObject(value) || typeof value.kind !== 'string') invalidResponse()
  if (value.kind === 'display') {
    const output = exactObject(value, ['kind', 'text'])
    return { kind: 'display', text: plainText(output.text, 2000) }
  }
  const output = exactObject(value, ['kind', 'order'])
  if (value.kind === 'cancelOrder') {
    const order = exactObject(output.order, ['symbol', 'orderId'])
    const parsedSymbol = symbol(order.symbol)
    const orderId = boundedString(order.orderId, 128)
    if (parsedSymbol !== snapshot.symbol
      || !snapshot.grantedCapabilities.includes('trade.cancel')
      || !snapshot.grantedCapabilities.includes('orders.read')
      || !snapshot.orders?.items.some((candidate) => (
        candidate.orderId === orderId
        && (candidate.status === 'New' || candidate.status === 'PartiallyFilled')
      ))) invalidResponse()
    return { kind: 'cancelOrder', order: { symbol: parsedSymbol, orderId } }
  }
  if (value.kind !== 'placeOrder') invalidResponse()
  const order = exactObject(output.order, [
    'symbol', 'side', 'orderType', 'qty', 'price', 'timeInForce', 'positionIdx', 'reduceOnly',
  ])
  const parsedSymbol = symbol(order.symbol)
  const orderType = order.orderType
  const price = order.price === null ? null : decimal(order.price, true)
  if (parsedSymbol !== snapshot.symbol
    || !snapshot.grantedCapabilities.includes('trade.place')
    || (order.side !== 'Buy' && order.side !== 'Sell')
    || (orderType !== 'Market' && orderType !== 'Limit')
    || (order.timeInForce !== 'GTC' && order.timeInForce !== 'IOC' && order.timeInForce !== 'FOK')
    || !Number.isInteger(order.positionIdx)
    || ![1, 2].includes(order.positionIdx as number)
    || typeof order.reduceOnly !== 'boolean'
    || (orderType === 'Market' && (price !== null || order.timeInForce !== 'IOC'))
    || (orderType === 'Limit' && price === null)) invalidResponse()
  const expectedPositionIdx = order.reduceOnly
    ? order.side === 'Sell' ? 1 : 2
    : order.side === 'Buy' ? 1 : 2
  if (order.positionIdx !== expectedPositionIdx) invalidResponse()
  return {
    kind: 'placeOrder',
    order: {
      symbol: parsedSymbol,
      side: order.side,
      orderType,
      qty: decimal(order.qty, true),
      price,
      timeInForce: order.timeInForce,
      positionIdx: order.positionIdx as 1 | 2,
      reduceOnly: order.reduceOnly,
    },
  }
}

function parseConfirmation(
  value: unknown,
  output: PluginWorkflowOutput,
  snapshot: PluginWorkflowSnapshot,
): PluginWorkflowConfirmation | null {
  if (output.kind === 'display') {
    if (value !== null) invalidResponse()
    return null
  }
  const confirmation = exactObject(value, CONFIRMATION_KEYS)
  const submissionId = confirmation.submissionId === null
    ? null
    : boundedString(confirmation.submissionId, 64)
  const expiresAtMs = canonicalCounter(confirmation.expiresAtMs)
  if (BigInt(expiresAtMs) < BigInt(snapshot.capturedAtMs)
    || (output.kind === 'placeOrder' && submissionId === null)
    || (output.kind === 'cancelOrder' && submissionId !== null)) invalidResponse()
  return { token: token(confirmation.token), expiresAtMs, submissionId }
}

function parseWorkflowResult(
  value: unknown,
  intent: PluginWorkflowExecutionIntent,
  access: PluginWorkflowAccess,
  run: PluginWorkflowRunInput,
): PluginWorkflowResult {
  const result = exactObject(value, RESULT_KEYS)
  const account = parseAccount(result.account)
  if (result.schemaVersion !== 1
    || requestId(result.requestId) !== run.requestId
    || pluginId(result.pluginId) !== intent.pluginId
    || pluginId(result.contributionId) !== intent.contributionId
    || canonicalCounter(result.catalogGeneration) !== intent.expectedCatalogGeneration
    || canonicalCounter(result.revision) !== intent.expectedRevision
    || canonicalCounter(result.grantRevision) !== access.grantRevision
    || !sameAccount(account, access.account)) invalidResponse()
  const snapshot = parseSnapshot(result.snapshot, access, run.symbol)
  const output = parseOutput(result.output, snapshot)
  return {
    schemaVersion: 1,
    requestId: run.requestId,
    pluginId: intent.pluginId,
    contributionId: intent.contributionId,
    catalogGeneration: intent.expectedCatalogGeneration,
    revision: intent.expectedRevision,
    account,
    grantRevision: access.grantRevision,
    snapshot,
    output,
    confirmation: parseConfirmation(result.confirmation, output, snapshot),
  }
}

export function parsePluginWorkflowInput(raw: string):
  | { ok: true; inputJson: string }
  | { ok: false; error: string } {
  if (textEncoder.encode(raw).byteLength > 4096) {
    return { ok: false, error: '插件输入不能超过 4096 字节。' }
  }
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    return { ok: false, error: '插件输入必须是有效的 JSON 对象。' }
  }
  if (!isObject(value)) {
    return { ok: false, error: '插件输入必须是 JSON 对象，不能是数组或标量。' }
  }
  return { ok: true, inputJson: raw }
}

export function parsePluginWorkflowSymbol(raw: string):
  | { ok: true; symbol: string }
  | { ok: false; error: string } {
  const normalized = raw.trim().toUpperCase()
  return /^[A-Z0-9]{1,32}$/.test(normalized)
    ? { ok: true, symbol: normalized }
    : { ok: false, error: '交易对必须为 1 至 32 位大写字母或数字。' }
}

export async function runPluginWorkflow(
  intent: PluginWorkflowExecutionIntent,
  access: PluginWorkflowAccess,
  run: PluginWorkflowRunInput,
): Promise<PluginWorkflowResult> {
  requestId(run.requestId)
  const parsedSymbol = symbol(run.symbol)
  const parsedInput = parsePluginWorkflowInput(run.inputJson)
  if (!parsedInput.ok) throw new Error(parsedInput.error)
  const value = await tauriInvoke<unknown>('run_plugin_workflow', {
    request: {
      ...authorityRequest(intent),
      requestId: run.requestId,
      expectedAccountId: access.account.accountId,
      expectedSessionEpoch: access.account.sessionEpoch,
      expectedGrantRevision: access.grantRevision,
      symbol: parsedSymbol,
      inputJson: parsedInput.inputJson,
    },
  })
  return parseWorkflowResult(value, intent, access, {
    requestId: run.requestId, symbol: parsedSymbol, inputJson: parsedInput.inputJson,
  })
}

function parseReceipt(
  value: unknown,
  confirmation: PluginWorkflowConfirmation,
  output: PluginWorkflowOutput,
  account: PluginWorkflowAccount,
): PluginWorkflowTradeReceipt {
  if (output.kind === 'display') invalidResponse()
  const receipt = exactObject(value, RECEIPT_KEYS)
  const parsedAccount = parseAccount(receipt.account)
  const expectedAction = output.kind
  if (receipt.schemaVersion !== 1
    || token(receipt.token) !== confirmation.token
    || !sameAccount(parsedAccount, account)
    || receipt.action !== expectedAction
    || (receipt.status !== 'accepted' && receipt.status !== 'rejected' && receipt.status !== 'unknown')) {
    invalidResponse()
  }
  const submissionId = receipt.submissionId === null
    ? null
    : boundedString(receipt.submissionId, 64)
  if (submissionId !== confirmation.submissionId) invalidResponse()
  const parsedOrder = receipt.order === null ? null : parseOrder(receipt.order)
  const errorCode = receipt.errorCode === null
    ? null
    : boundedString(receipt.errorCode, 128)
  if ((receipt.status === 'accepted' && (parsedOrder === null || errorCode !== null))
    || (receipt.status !== 'accepted' && (parsedOrder !== null
      || errorCode === null || !errorCode.startsWith('plugin_workflow_')))) invalidResponse()
  return {
    schemaVersion: 1,
    token: confirmation.token,
    account: parsedAccount,
    action: expectedAction,
    status: receipt.status,
    submissionId,
    order: parsedOrder,
    errorCode,
  }
}

export async function confirmPluginWorkflow(
  confirmation: PluginWorkflowConfirmation,
  output: PluginWorkflowOutput,
  account: PluginWorkflowAccount,
): Promise<PluginWorkflowTradeReceipt> {
  const confirmationToken = token(confirmation.token)
  const value = await tauriInvoke<unknown>('confirm_plugin_workflow', { token: confirmationToken })
  return parseReceipt(value, confirmation, output, account)
}

function nativeWorkflowErrorCode(error: unknown): string | null {
  if (!isObject(error) || Object.keys(error).length !== 2
    || typeof error.code !== 'string' || typeof error.message !== 'string'
    || !error.code.startsWith('plugin_workflow_')) return null
  return error.code
}

export function isPluginWorkflowConfirmationNotSubmitted(error: unknown): boolean {
  const code = nativeWorkflowErrorCode(error)
  return code !== null && CONFIRMATION_NOT_SUBMITTED_CODES.has(code)
}

export function pluginWorkflowErrorMessage(error: unknown): string {
  const code = nativeWorkflowErrorCode(error)
  if (code !== null) return RECEIPT_ERROR_MESSAGES[code] ?? GENERIC_ERROR
  return error instanceof Error && error.message === INVALID_RESPONSE_ERROR
    ? INVALID_RESPONSE_ERROR
    : GENERIC_ERROR
}
