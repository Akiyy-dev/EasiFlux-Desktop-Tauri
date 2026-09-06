import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  PluginAvailability,
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogMutationResult,
  PluginCatalogSnapshot,
  PluginManifestV1,
  PluginStatus,
} from '../types/plugin'

const INVALID_RESPONSE_ERROR = '插件服务返回的数据无效，请重试。'
const GENERIC_ERROR = '插件操作失败，请重试。'
const U64_MAX = 18_446_744_073_709_551_615n
const textEncoder = new TextEncoder()

type PluginErrorCode =
  | 'plugin_invalid_id'
  | 'plugin_not_found'
  | 'plugin_catalog_invalid'
  | 'plugin_state_unavailable'
  | 'plugin_state_persist_failed'
  | 'plugin_revision_exhausted'
  | 'plugin_not_toggleable'

const ERROR_MESSAGES = {
  plugin_invalid_id: '插件标识无效。',
  plugin_not_found: '未找到该插件。',
  plugin_catalog_invalid: '插件目录不可用，请稍后重试。',
  plugin_state_unavailable: '插件状态暂不可用，请重试。',
  plugin_state_persist_failed: '保存插件状态失败，请重试。',
  plugin_revision_exhausted: '插件状态版本已达到上限，请联系支持。',
  plugin_not_toggleable: '该插件当前无法更改启用状态。',
} satisfies Record<PluginErrorCode, string>

const SNAPSHOT_KEYS = [
  'schemaVersion',
  'revision',
  'availability',
  'availabilityReasonCode',
  'plugins',
] as const
const ITEM_KEYS = [
  'manifest',
  'source',
  'status',
  'statusReasonCode',
  'canToggle',
  'grantedCapabilities',
] as const
const MANIFEST_KEYS = [
  'schemaVersion',
  'id',
  'publisherId',
  'publisher',
  'name',
  'description',
  'version',
  'contributions',
  'requestedCapabilities',
] as const
const MUTATION_KEYS = ['revision', 'plugin'] as const
const ERROR_KEYS = ['code', 'message'] as const

function invalidResponse(): never {
  throw new Error(INVALID_RESPONSE_ERROR)
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function hasExactKeys(
  value: unknown,
  keys: readonly string[],
): value is Record<string, unknown> {
  if (!isObject(value)) return false
  const actualKeys = Object.keys(value)
  return actualKeys.length === keys.length
    && keys.every((key) => Object.prototype.hasOwnProperty.call(value, key))
}

function requireExactObject(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!hasExactKeys(value, keys)) invalidResponse()
  return value
}

function requireEmptyArray(value: unknown): void {
  if (!Array.isArray(value) || value.length !== 0) invalidResponse()
}

function requireString(value: unknown): string {
  if (typeof value !== 'string') invalidResponse()
  return value
}

function requireDisplayText(value: unknown, maxBytes: number): string {
  const text = requireString(value)
  if (text.trim().length === 0 || textEncoder.encode(text).byteLength > maxBytes) {
    invalidResponse()
  }
  return text
}

function requireReverseDomainId(value: unknown): string {
  const id = requireString(value)
  if (textEncoder.encode(id).byteLength > 128) invalidResponse()
  const segments = id.split('.')
  if (segments.length < 2) invalidResponse()
  for (const segment of segments) {
    if (
      segment.length === 0
      || textEncoder.encode(segment).byteLength > 63
      || !/^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(segment)
    ) invalidResponse()
  }
  return id
}

function requireCanonicalRevision(value: unknown): string {
  const revision = requireString(value)
  if (
    revision.length > 20
    || !/^(?:0|[1-9][0-9]*)$/.test(revision)
    || BigInt(revision) > U64_MAX
  ) {
    invalidResponse()
  }
  return revision
}

function identifiersAreValid(value: string, rejectLeadingZeroNumbers: boolean): boolean {
  if (value.length === 0) return false
  return value.split('.').every((identifier) => (
    identifier.length > 0
    && /^[0-9A-Za-z-]+$/.test(identifier)
    && (!rejectLeadingZeroNumbers || !/^0[0-9]+$/.test(identifier))
  ))
}

function isSemVer(value: string): boolean {
  const plusParts = value.split('+')
  if (plusParts.length > 2) return false
  const [coreAndPreRelease, build] = plusParts
  if (build !== undefined && !identifiersAreValid(build, false)) return false

  const hyphenIndex = coreAndPreRelease.indexOf('-')
  const core = hyphenIndex === -1
    ? coreAndPreRelease
    : coreAndPreRelease.slice(0, hyphenIndex)
  const preRelease = hyphenIndex === -1
    ? undefined
    : coreAndPreRelease.slice(hyphenIndex + 1)
  if (preRelease !== undefined && !identifiersAreValid(preRelease, true)) return false

  const coreIdentifiers = core.split('.')
  return coreIdentifiers.length === 3
    && coreIdentifiers.every((identifier) => (
      identifier.length <= 20
      && /^(?:0|[1-9][0-9]*)$/.test(identifier)
      && BigInt(identifier) <= U64_MAX
    ))
}

function parseManifest(value: unknown): PluginManifestV1 {
  const manifest = requireExactObject(value, MANIFEST_KEYS)
  if (manifest.schemaVersion !== 1) invalidResponse()
  const id = requireReverseDomainId(manifest.id)
  const publisherId = requireReverseDomainId(manifest.publisherId)
  const publisher = requireDisplayText(manifest.publisher, 80)
  const name = requireDisplayText(manifest.name, 80)
  const description = requireDisplayText(manifest.description, 500)
  const version = requireDisplayText(manifest.version, 64)
  if (!isSemVer(version)) invalidResponse()
  requireEmptyArray(manifest.contributions)
  requireEmptyArray(manifest.requestedCapabilities)

  return {
    schemaVersion: 1,
    id,
    publisherId,
    publisher,
    name,
    description,
    version,
    contributions: [],
    requestedCapabilities: [],
  }
}

function parseReason(value: unknown): PluginAvailabilityReason | null {
  if (value === null) return null
  if (value === 'stateUnavailable' || value === 'catalogInvalid') return value
  return invalidResponse()
}

function parseStatus(value: unknown): PluginStatus {
  if (value === 'enabled' || value === 'disabled' || value === 'blocked') return value
  return invalidResponse()
}

function parseCatalogItem(value: unknown): PluginCatalogItem {
  const item = requireExactObject(value, ITEM_KEYS)
  const manifest = parseManifest(item.manifest)
  if (item.source !== 'builtIn') invalidResponse()
  const status = parseStatus(item.status)
  const statusReasonCode = parseReason(item.statusReasonCode)
  if (typeof item.canToggle !== 'boolean') invalidResponse()
  requireEmptyArray(item.grantedCapabilities)

  if (
    (status === 'blocked' && (item.canToggle || statusReasonCode === null))
    || (status !== 'blocked' && (!item.canToggle || statusReasonCode !== null))
  ) invalidResponse()

  return {
    manifest,
    source: 'builtIn',
    status,
    statusReasonCode,
    canToggle: item.canToggle,
    grantedCapabilities: [],
  }
}

function parseAvailability(value: unknown): PluginAvailability {
  if (value === 'available' || value === 'unavailable') return value
  return invalidResponse()
}

function parseSnapshot(value: unknown): PluginCatalogSnapshot {
  const snapshot = requireExactObject(value, SNAPSHOT_KEYS)
  if (snapshot.schemaVersion !== 1 || !Array.isArray(snapshot.plugins)) invalidResponse()
  const revision = requireCanonicalRevision(snapshot.revision)
  const availability = parseAvailability(snapshot.availability)
  const availabilityReasonCode = parseReason(snapshot.availabilityReasonCode)
  const plugins = snapshot.plugins.map(parseCatalogItem)

  for (let index = 1; index < plugins.length; index += 1) {
    if (plugins[index - 1].manifest.id >= plugins[index].manifest.id) invalidResponse()
  }

  if (availability === 'available') {
    if (availabilityReasonCode !== null || plugins.some((plugin) => plugin.status === 'blocked')) {
      invalidResponse()
    }
  } else if (
    availabilityReasonCode === null
    || plugins.some((plugin) => (
      plugin.status !== 'blocked'
      || plugin.canToggle
      || plugin.statusReasonCode !== availabilityReasonCode
    ))
  ) invalidResponse()

  return {
    schemaVersion: 1,
    revision,
    availability,
    availabilityReasonCode,
    plugins,
  }
}

function parseMutation(value: unknown): PluginCatalogMutationResult {
  const mutation = requireExactObject(value, MUTATION_KEYS)
  const revision = requireCanonicalRevision(mutation.revision)
  const plugin = parseCatalogItem(mutation.plugin)
  if (plugin.status === 'blocked') invalidResponse()
  return { revision, plugin }
}

export async function getPluginCatalog(): Promise<PluginCatalogSnapshot> {
  return parseSnapshot(await tauriInvoke<unknown>('get_plugin_catalog'))
}

export async function setPluginEnabled(
  id: string,
  enabled: boolean,
): Promise<PluginCatalogMutationResult> {
  const result = parseMutation(await tauriInvoke<unknown>('set_plugin_enabled', { id, enabled }))
  const expectedStatus = enabled ? 'enabled' : 'disabled'
  if (result.plugin.manifest.id !== id || result.plugin.status !== expectedStatus) invalidResponse()
  return result
}

export function pluginErrorMessage(error: unknown): string {
  if (!hasExactKeys(error, ERROR_KEYS)) return GENERIC_ERROR
  if (typeof error.code !== 'string' || typeof error.message !== 'string') return GENERIC_ERROR
  if (!Object.prototype.hasOwnProperty.call(ERROR_MESSAGES, error.code)) return GENERIC_ERROR
  return ERROR_MESSAGES[error.code as PluginErrorCode]
}
