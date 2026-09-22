import { tauriInvoke } from '../composables/useTauriCommand'
import type { PluginStrategyExecutionIntent } from '../types/plugin'
import {
  PLUGIN_STRATEGY_CAPABILITIES,
  type PluginStrategyCapability,
  type PluginStrategyStartInput,
  type StrategyAccess,
  type StrategyPolicy,
  type StrategyReceipt,
  type StrategyRunList,
  type StrategyRunReason,
  type StrategyRunStatus,
  type StrategyRunView,
} from '../types/pluginStrategy'
import type { PluginWorkflowAccount } from '../types/pluginWorkflow'

const INVALID_RESPONSE_ERROR = '插件策略服务返回的数据无效，请重试。'
const GENERIC_ERROR = '插件策略操作失败，请重试。'
const U64_MAX = 18_446_744_073_709_551_615n
const textEncoder = new TextEncoder()

const ACCESS_KEYS = [
  'schemaVersion', 'pluginId', 'contributionId', 'catalogGeneration', 'revision',
  'account', 'requestedCapabilities', 'authorizationToken', 'expiresAtMs',
] as const
const ACCOUNT_KEYS = ['accountId', 'sessionEpoch', 'environment'] as const
const POLICY_KEYS = [
  'intervalMs', 'maxOrderQty', 'maxTotalQty', 'maxActions', 'maxRunSeconds', 'reduceOnly',
] as const
const RUN_KEYS = [
  'schemaVersion', 'runId', 'requestId', 'pluginId', 'contributionId', 'account', 'symbol',
  'status', 'reason', 'policy', 'capabilities', 'inputJson', 'startedAtMs', 'expiresAtMs',
  'sequence', 'actionsSubmitted', 'totalSubmittedQty', 'lastMessage', 'lastReceipt',
] as const
const RECEIPT_KEYS = [
  'sequence', 'kind', 'status', 'submissionId', 'orderId', 'errorCode',
] as const
const LIST_KEYS = ['schemaVersion', 'runs'] as const

const STATUSES = new Set<StrategyRunStatus>([
  'running', 'paused', 'stopping', 'stopped', 'recoveryRequired', 'faulted', 'completed',
])
const RUN_REASONS = new Set<StrategyRunReason>([
  'plugin_strategy_paused', 'plugin_strategy_stopped', 'plugin_strategy_restarted',
  'plugin_strategy_expired', 'plugin_strategy_limit_reached', 'plugin_strategy_stale',
  'plugin_strategy_data_unavailable', 'plugin_strategy_compute_failed',
  'plugin_strategy_invalid_output', 'plugin_strategy_recovery_required',
  'plugin_strategy_storage_unavailable', 'plugin_strategy_ack_failed',
  'plugin_strategy_unavailable',
])
const CAPABILITIES = new Set<PluginStrategyCapability>(PLUGIN_STRATEGY_CAPABILITIES)
const ERROR_MESSAGES: Readonly<Record<string, string>> = {
  plugin_strategy_invalid_request: '策略请求无效，请重新检查后再试。',
  plugin_strategy_denied: '当前账户或插件没有此策略操作权限。',
  plugin_strategy_token_invalid: '一次性策略凭据已过期、失效或被控制操作撤销，请重新获取。',
  plugin_strategy_capacity: '策略运行或恢复记录已达到容量上限。',
  plugin_strategy_busy: '策略服务正在处理另一项操作，请重新读取状态。',
  plugin_strategy_stale: '插件、账户或授权上下文已经变化，请重新读取并获取新凭据。',
  plugin_strategy_recovery_required: '存在未决策略动作，必须先核对结果。',
  plugin_strategy_storage_unavailable: '策略持久化暂不可用，自动交易保持关闭。',
  plugin_strategy_unavailable: '策略运行服务暂不可用。',
}
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/
const DECIMAL_PATTERN = /^\+?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/
const DECIMAL_MAX: ComparableDecimal = {
  digits: '79228162514264337593543950335',
  magnitude: 29n,
}

function invalidResponse(): never {
  throw new Error(INVALID_RESPONSE_ERROR)
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function exactObject(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!isObject(value)) invalidResponse()
  const actual = Object.keys(value)
  if (actual.length !== keys.length
    || !keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))) invalidResponse()
  return value
}

function boundedString(value: unknown, maxBytes: number, allowEmpty = false): string {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)
    || textEncoder.encode(value).byteLength > maxBytes) invalidResponse()
  return value
}

function canonicalU64(value: unknown): string {
  const parsed = boundedString(value, 20)
  if (!/^(?:0|[1-9][0-9]*)$/.test(parsed) || BigInt(parsed) > U64_MAX) invalidResponse()
  return parsed
}

function uuid(value: unknown): string {
  const parsed = boundedString(value, 36)
  if (!UUID_PATTERN.test(parsed)) invalidResponse()
  return parsed
}

function pluginId(value: unknown): string {
  return boundedString(value, 128)
}

function parseAccount(value: unknown): PluginWorkflowAccount {
  const account = exactObject(value, ACCOUNT_KEYS)
  return {
    accountId: boundedString(account.accountId, 128),
    sessionEpoch: canonicalU64(account.sessionEpoch),
    environment: boundedString(account.environment, 256),
  }
}

function sameAccount(left: PluginWorkflowAccount, right: PluginWorkflowAccount): boolean {
  return left.accountId === right.accountId
    && left.sessionEpoch === right.sessionEpoch
    && left.environment === right.environment
}

function sameAccountScope(left: PluginWorkflowAccount, right: PluginWorkflowAccount): boolean {
  return left.accountId === right.accountId && left.environment === right.environment
}

function parseCapabilities(value: unknown): PluginStrategyCapability[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > CAPABILITIES.size) {
    invalidResponse()
  }
  const parsed = value.map((candidate) => {
    if (typeof candidate !== 'string' || !CAPABILITIES.has(candidate as PluginStrategyCapability)) {
      return invalidResponse()
    }
    return candidate as PluginStrategyCapability
  })
  if (new Set(parsed).size !== parsed.length) invalidResponse()
  return parsed
}

function parseRunCapabilities(value: unknown): PluginStrategyCapability[] {
  const parsed = parseCapabilities(value)
  if (!parsed.includes('account.read') || !parsed.includes('strategy.run')
    || (parsed.includes('trade.cancel') && !parsed.includes('orders.read'))) {
    invalidResponse()
  }
  return parsed
}

function sameCapabilities(
  left: readonly PluginStrategyCapability[],
  right: readonly PluginStrategyCapability[],
): boolean {
  return left.length === right.length && left.every((capability, index) => capability === right[index])
}

interface ComparableDecimal {
  digits: string
  magnitude: bigint
}

function decimal(value: unknown, positive: boolean): { raw: string; comparable: ComparableDecimal } {
  const raw = boundedString(value, 64)
  if (!DECIMAL_PATTERN.test(raw)) invalidResponse()
  const unsigned = raw.startsWith('+') ? raw.slice(1) : raw
  const exponentAt = unsigned.search(/[eE]/)
  const mantissa = exponentAt === -1 ? unsigned : unsigned.slice(0, exponentAt)
  const exponent = exponentAt === -1 ? 0n : BigInt(unsigned.slice(exponentAt + 1))
  const dot = mantissa.indexOf('.')
  const fractionalDigits = dot === -1 ? 0 : mantissa.length - dot - 1
  const allDigits = mantissa.replace('.', '')
  let digits = allDigits.replace(/^0+/, '')
  if (positive && digits.length === 0) invalidResponse()
  let effectiveScale = BigInt(fractionalDigits) - exponent
  while (effectiveScale > 0n && digits.endsWith('0')) {
    digits = digits.slice(0, -1)
    effectiveScale -= 1n
  }
  const comparable = digits.length === 0
    ? { digits: '0', magnitude: 0n }
    : { digits, magnitude: BigInt(digits.length) - effectiveScale }
  if (effectiveScale > 28n || compareDecimal(comparable, DECIMAL_MAX) > 0) invalidResponse()
  return {
    raw,
    comparable,
  }
}

function compareDecimal(left: ComparableDecimal, right: ComparableDecimal): number {
  if (left.digits === '0') return right.digits === '0' ? 0 : -1
  if (right.digits === '0') return 1
  if (left.magnitude !== right.magnitude) return left.magnitude < right.magnitude ? -1 : 1
  const length = Math.max(left.digits.length, right.digits.length)
  const leftDigits = left.digits.padEnd(length, '0')
  const rightDigits = right.digits.padEnd(length, '0')
  return leftDigits === rightDigits ? 0 : leftDigits < rightDigits ? -1 : 1
}

export function parsePluginStrategyPolicy(value: unknown): StrategyPolicy {
  const candidate = exactObject(value, POLICY_KEYS)
  if (!Number.isInteger(candidate.intervalMs)
    || (candidate.intervalMs as number) < 5_000 || (candidate.intervalMs as number) > 60_000
    || !Number.isInteger(candidate.maxActions)
    || (candidate.maxActions as number) < 1 || (candidate.maxActions as number) > 1_000
    || !Number.isInteger(candidate.maxRunSeconds)
    || (candidate.maxRunSeconds as number) < 60 || (candidate.maxRunSeconds as number) > 86_400
    || typeof candidate.reduceOnly !== 'boolean') invalidResponse()
  const maxOrderQty = decimal(candidate.maxOrderQty, true)
  const maxTotalQty = decimal(candidate.maxTotalQty, true)
  if (compareDecimal(maxOrderQty.comparable, maxTotalQty.comparable) > 0) invalidResponse()
  return {
    intervalMs: candidate.intervalMs as number,
    maxOrderQty: maxOrderQty.raw,
    maxTotalQty: maxTotalQty.raw,
    maxActions: candidate.maxActions as number,
    maxRunSeconds: candidate.maxRunSeconds as number,
    reduceOnly: candidate.reduceOnly,
  }
}

function authorityRequest(intent: PluginStrategyExecutionIntent) {
  return {
    pluginId: intent.pluginId,
    contributionId: intent.contributionId,
    expectedCatalogGeneration: intent.expectedCatalogGeneration,
    expectedRevision: intent.expectedRevision,
  }
}

export function parsePluginStrategyAccess(
  value: unknown,
  intent: PluginStrategyExecutionIntent,
): StrategyAccess {
  const access = exactObject(value, ACCESS_KEYS)
  const account = parseAccount(access.account)
  const requestedCapabilities = parseCapabilities(access.requestedCapabilities)
  if (access.schemaVersion !== 1
    || pluginId(access.pluginId) !== intent.pluginId
    || pluginId(access.contributionId) !== intent.contributionId
    || canonicalU64(access.catalogGeneration) !== intent.expectedCatalogGeneration
    || canonicalU64(access.revision) !== intent.expectedRevision
    || !sameCapabilities(requestedCapabilities, intent.requestedCapabilities)) invalidResponse()
  return {
    schemaVersion: 1,
    pluginId: intent.pluginId,
    contributionId: intent.contributionId,
    catalogGeneration: intent.expectedCatalogGeneration,
    revision: intent.expectedRevision,
    account,
    requestedCapabilities,
    authorizationToken: boundedString(access.authorizationToken, 512),
    expiresAtMs: canonicalU64(access.expiresAtMs),
  }
}

function parseInputJson(value: unknown): string {
  const raw = boundedString(value, 4096)
  if (textEncoder.encode(raw).byteLength > 4096) invalidResponse()
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return invalidResponse()
  }
  if (!isObject(parsed)) invalidResponse()
  return raw
}

export function parsePluginStrategySymbol(value: unknown): string {
  const normalized = typeof value === 'string' ? value.trim().toUpperCase() : ''
  if (!/^[A-Z0-9]{1,32}$/.test(normalized)) invalidResponse()
  return normalized
}

function parseOptionalString(value: unknown, maxBytes: number): string | null {
  return value === null ? null : boundedString(value, maxBytes, true)
}

function parseReceipt(value: unknown): StrategyReceipt | null {
  if (value === null) return null
  const receipt = exactObject(value, RECEIPT_KEYS)
  if ((receipt.kind !== 'placeOrder' && receipt.kind !== 'cancelOrder')
    || (receipt.status !== 'accepted' && receipt.status !== 'rejected' && receipt.status !== 'unknown')) {
    invalidResponse()
  }
  const parsedErrorCode = parseOptionalString(receipt.errorCode, 128)
  const errorCode: StrategyReceipt['errorCode'] = parsedErrorCode === null
    || parsedErrorCode === 'plugin_strategy_rejected'
    || parsedErrorCode === 'plugin_strategy_unknown'
    ? parsedErrorCode
    : invalidResponse()
  if ((receipt.status === 'accepted' && errorCode !== null)
    || (receipt.status === 'rejected' && errorCode !== 'plugin_strategy_rejected')
    || (receipt.status === 'unknown' && errorCode !== 'plugin_strategy_unknown')) invalidResponse()
  return {
    sequence: canonicalU64(receipt.sequence),
    kind: receipt.kind,
    status: receipt.status,
    submissionId: parseOptionalString(receipt.submissionId, 128),
    orderId: parseOptionalString(receipt.orderId, 128),
    errorCode,
  }
}

interface RunExpectation {
  runId?: string
  requestId?: string
  intent?: PluginStrategyExecutionIntent
  access?: StrategyAccess
  start?: PluginStrategyStartInput
  previousRun?: StrategyRunView
}

export function parsePluginStrategyRunView(
  value: unknown,
  expectation: RunExpectation = {},
): StrategyRunView {
  const run = exactObject(value, RUN_KEYS)
  const runId = uuid(run.runId)
  const requestId = uuid(run.requestId)
  const account = parseAccount(run.account)
  const policy = parsePluginStrategyPolicy(run.policy)
  const capabilities = parseRunCapabilities(run.capabilities)
  const status = typeof run.status === 'string' && STATUSES.has(run.status as StrategyRunStatus)
    ? run.status as StrategyRunStatus
    : invalidResponse()
  const startedAtMs = canonicalU64(run.startedAtMs)
  const expiresAtMs = canonicalU64(run.expiresAtMs)
  const sequence = canonicalU64(run.sequence)
  if (run.schemaVersion !== 1
    || !Number.isInteger(run.actionsSubmitted) || (run.actionsSubmitted as number) < 0
    || (run.actionsSubmitted as number) > policy.maxActions
    || BigInt(expiresAtMs) - BigInt(startedAtMs) !== BigInt(policy.maxRunSeconds) * 1000n) {
    invalidResponse()
  }
  const totalSubmittedQty = decimal(run.totalSubmittedQty, false)
  const maxTotalQty = decimal(policy.maxTotalQty, true)
  if (compareDecimal(totalSubmittedQty.comparable, maxTotalQty.comparable) > 0) invalidResponse()
  const receipt = parseReceipt(run.lastReceipt)
  if (receipt !== null && BigInt(receipt.sequence) > BigInt(sequence)) invalidResponse()
  const parsedReason = parseOptionalString(run.reason, 2000)
  if (parsedReason !== null && !RUN_REASONS.has(parsedReason as StrategyRunReason)) invalidResponse()
  const reason = parsedReason as StrategyRunReason | null
  const parsed: StrategyRunView = {
    schemaVersion: 1,
    runId,
    requestId,
    pluginId: pluginId(run.pluginId),
    contributionId: pluginId(run.contributionId),
    account,
    symbol: parsePluginStrategySymbol(run.symbol),
    status,
    reason,
    policy,
    capabilities,
    inputJson: parseInputJson(run.inputJson),
    startedAtMs,
    expiresAtMs,
    sequence,
    actionsSubmitted: run.actionsSubmitted as number,
    totalSubmittedQty: totalSubmittedQty.raw,
    lastMessage: boundedString(run.lastMessage, 2000, true),
    lastReceipt: receipt,
  }
  if ((expectation.runId !== undefined && parsed.runId !== expectation.runId)
    || (expectation.requestId !== undefined && parsed.requestId !== expectation.requestId)) {
    invalidResponse()
  }
  if (expectation.intent && (
    parsed.pluginId !== expectation.intent.pluginId
    || parsed.contributionId !== expectation.intent.contributionId
  )) invalidResponse()
  if (expectation.access && !sameAccount(parsed.account, expectation.access.account)) invalidResponse()
  if (expectation.start && (
    parsed.symbol !== expectation.start.symbol
    || parsed.inputJson !== expectation.start.inputJson
    || !sameCapabilities(parsed.capabilities, expectation.start.capabilities)
    || JSON.stringify(parsed.policy) !== JSON.stringify(expectation.start.policy)
    || (expectation.start.resumeRunId !== null && parsed.runId !== expectation.start.resumeRunId)
  )) invalidResponse()
  if (expectation.previousRun && (
    parsed.runId !== expectation.previousRun.runId
    || parsed.startedAtMs !== expectation.previousRun.startedAtMs
    || parsed.expiresAtMs !== expectation.previousRun.expiresAtMs
    || BigInt(parsed.sequence) < BigInt(expectation.previousRun.sequence)
    || parsed.actionsSubmitted < expectation.previousRun.actionsSubmitted
    || compareDecimal(
      decimal(parsed.totalSubmittedQty, false).comparable,
      decimal(expectation.previousRun.totalSubmittedQty, false).comparable,
    ) < 0
  )) invalidResponse()
  return parsed
}

function parseRunList(value: unknown): StrategyRunList {
  const list = exactObject(value, LIST_KEYS)
  if (list.schemaVersion !== 1 || !Array.isArray(list.runs) || list.runs.length > 32) {
    invalidResponse()
  }
  const runs = list.runs.map((run) => parsePluginStrategyRunView(run))
  if (new Set(runs.map((run) => run.runId)).size !== runs.length) invalidResponse()
  return { schemaVersion: 1, runs }
}

function validateStart(
  intent: PluginStrategyExecutionIntent,
  access: StrategyAccess,
  value: PluginStrategyStartInput,
  previousRun?: StrategyRunView,
): PluginStrategyStartInput {
  const requestId = uuid(value.requestId)
  const resumeRunId = value.resumeRunId === null ? null : uuid(value.resumeRunId)
  const symbol = parsePluginStrategySymbol(value.symbol)
  const inputJson = parseInputJson(value.inputJson)
  const capabilities = parseCapabilities(value.capabilities)
  const policy = parsePluginStrategyPolicy(value.policy)
  const isResume = resumeRunId !== null
  if (value.acknowledgeAutomaticTrading !== true
    || !capabilities.includes('account.read') || !capabilities.includes('strategy.run')
    || (capabilities.includes('trade.cancel') && !capabilities.includes('orders.read'))
    || capabilities.some((capability) => !access.requestedCapabilities.includes(capability))
    || !sameCapabilities(access.requestedCapabilities, intent.requestedCapabilities)
    || isResume !== (previousRun !== undefined)) invalidResponse()
  if (previousRun && (
    previousRun.runId !== resumeRunId
    || previousRun.pluginId !== intent.pluginId
    || previousRun.contributionId !== intent.contributionId
    || !sameAccountScope(previousRun.account, access.account)
    || previousRun.symbol !== symbol
    || previousRun.inputJson !== inputJson
    || !sameCapabilities(previousRun.capabilities, capabilities)
    || JSON.stringify(previousRun.policy) !== JSON.stringify(policy)
  )) invalidResponse()
  return {
    requestId, resumeRunId, symbol, inputJson, capabilities, policy,
    acknowledgeAutomaticTrading: true,
  }
}

export async function getPluginStrategyAccess(
  intent: PluginStrategyExecutionIntent,
): Promise<StrategyAccess> {
  const value = await tauriInvoke<unknown>('get_plugin_strategy_access', {
    request: authorityRequest(intent),
  })
  return parsePluginStrategyAccess(value, intent)
}

export async function startPluginStrategy(
  intent: PluginStrategyExecutionIntent,
  access: StrategyAccess,
  input: PluginStrategyStartInput,
  previousRun?: StrategyRunView,
): Promise<StrategyRunView> {
  const start = validateStart(intent, access, input, previousRun)
  const value = await tauriInvoke<unknown>('start_plugin_strategy', {
    request: {
      authorizationToken: access.authorizationToken,
      requestId: start.requestId,
      resumeRunId: start.resumeRunId,
      symbol: start.symbol,
      inputJson: start.inputJson,
      capabilities: [...start.capabilities],
      policy: { ...start.policy },
      acknowledgeAutomaticTrading: true,
    },
  })
  return parsePluginStrategyRunView(value, {
    requestId: start.requestId, intent, access, start, previousRun,
  })
}

export async function listPluginStrategies(): Promise<StrategyRunList> {
  return parseRunList(await tauriInvoke<unknown>('list_plugin_strategies'))
}

export async function controlPluginStrategy(
  runId: string,
  action: 'pause' | 'stop',
): Promise<StrategyRunView> {
  const expectedRunId = uuid(runId)
  if (action !== 'pause' && action !== 'stop') invalidResponse()
  const value = await tauriInvoke<unknown>('control_plugin_strategy', {
    request: { runId: expectedRunId, action },
  })
  return parsePluginStrategyRunView(value, { runId: expectedRunId })
}

export async function stopAllPluginStrategies(): Promise<StrategyRunList> {
  return parseRunList(await tauriInvoke<unknown>('stop_all_plugin_strategies'))
}

export async function reconcilePluginStrategy(runId: string): Promise<StrategyRunView> {
  const expectedRunId = uuid(runId)
  const value = await tauriInvoke<unknown>('reconcile_plugin_strategy', { runId: expectedRunId })
  return parsePluginStrategyRunView(value, { runId: expectedRunId })
}

export function pluginStrategyErrorMessage(error: unknown): string {
  if (isObject(error) && Object.keys(error).length === 2
    && typeof error.code === 'string' && typeof error.message === 'string') {
    return ERROR_MESSAGES[error.code] ?? GENERIC_ERROR
  }
  return error instanceof Error && error.message === INVALID_RESPONSE_ERROR
    ? INVALID_RESPONSE_ERROR
    : GENERIC_ERROR
}
