import { tauriInvoke } from '../composables/useTauriCommand'
import type {
  CancelLocalManifestImportResult,
  CommitLocalManifestImportResult,
  LocalManifestImportCommitFailure,
  ManagedOwnershipSummary,
  PluginAvailability,
  PluginAvailabilityReason,
  PluginCatalogItem,
  PluginCatalogMutationResult,
  PluginCatalogSnapshot,
  PluginLocalDiscoverySummary,
  PluginManifestV1,
  PluginManagement,
  PluginStatus,
  PrepareLocalManifestImportResult,
  ReadyLocalManifestImport,
  RemoveManagedLocalPluginFailure,
  RemoveManagedLocalPluginResult,
} from '../types/plugin'

const INVALID_RESPONSE_ERROR = '插件服务返回的数据无效，请重试。'
const GENERIC_ERROR = '插件操作失败，请重试。'
const U64_MAX = 18_446_744_073_709_551_615n
const textEncoder = new TextEncoder()

export type PluginErrorCode =
  | 'plugin_invalid_id'
  | 'plugin_not_found'
  | 'plugin_catalog_invalid'
  | 'plugin_state_unavailable'
  | 'plugin_state_persist_failed'
  | 'plugin_revision_exhausted'
  | 'plugin_not_toggleable'
  | 'plugin_catalog_stale'
  | 'plugin_catalog_generation_exhausted'
  | 'plugin_state_capacity_exceeded'
  | 'plugin_import_busy'
  | 'plugin_import_dialog_unavailable'
  | 'plugin_import_source_rejected'
  | 'plugin_import_manifest_invalid'
  | 'plugin_import_token_invalid'
  | 'plugin_import_id_conflict'
  | 'plugin_import_discovery_unavailable'
  | 'plugin_import_capacity_exceeded'
  | 'plugin_import_staging_capacity_exceeded'
  | 'plugin_import_write_failed'
  | 'plugin_import_publication_unconfirmed'
  | 'plugin_ownership_unavailable'
  | 'plugin_ownership_persist_failed'
  | 'plugin_ownership_capacity_exceeded'
  | 'plugin_ownership_revision_exhausted'
  | 'plugin_ownership_conflict'
  | 'plugin_import_ownership_not_registered'
  | 'plugin_remove_discovery_unavailable'
  | 'plugin_remove_not_managed'
  | 'plugin_remove_requires_disabled'
  | 'plugin_remove_storage_unavailable'
  | 'plugin_remove_identity_changed'
  | 'plugin_remove_staging_capacity_exceeded'
  | 'plugin_remove_write_failed'
  | 'plugin_remove_publication_unconfirmed'

const ERROR_MESSAGES = {
  plugin_invalid_id: '插件标识无效。',
  plugin_not_found: '未找到该插件。',
  plugin_catalog_invalid: '插件目录不可用，请稍后重试。',
  plugin_state_unavailable: '插件状态暂不可用，请重试。',
  plugin_state_persist_failed: '保存插件状态失败，请重试。',
  plugin_revision_exhausted: '插件状态版本已达到上限，请联系支持。',
  plugin_not_toggleable: '该插件当前无法更改启用状态。',
  plugin_catalog_stale: '插件目录已更新，请刷新后重试。',
  plugin_catalog_generation_exhausted: '插件目录版本已达到上限，请联系支持。',
  plugin_state_capacity_exceeded: '插件状态容量已达到上限，请联系支持。',
  plugin_import_busy: '已有清单导入操作，请先完成或取消。',
  plugin_import_dialog_unavailable: '无法打开文件选择器，请重试。',
  plugin_import_source_rejected: '无法安全读取所选文件，请选择普通本地 JSON 文件。',
  plugin_import_manifest_invalid: '清单格式或内容不符合当前插件要求。',
  plugin_import_token_invalid: '预览已过期或失效，请重新选择清单。',
  plugin_import_id_conflict: '已存在相同插件 ID；当前不支持覆盖或更新。',
  plugin_import_discovery_unavailable: '请先修复本地插件发现问题，再导入清单。',
  plugin_import_capacity_exceeded: '本地插件数量或读取预算已达上限。',
  plugin_import_staging_capacity_exceeded: '导入暂存区需要人工检查和清理。',
  plugin_import_write_failed: '无法完成清单写入，请检查后重试。',
  plugin_import_publication_unconfirmed: '清单已写入，但目录结果尚未确认，请重新扫描。',
  plugin_ownership_unavailable: '所有权记录暂不可用，不能受管导入或移除。',
  plugin_ownership_persist_failed: '无法保存受管所有权记录，请重新扫描。',
  plugin_ownership_capacity_exceeded: '受管所有权记录已达上限。',
  plugin_ownership_revision_exhausted: '受管所有权记录版本已达到上限，请联系支持。',
  plugin_ownership_conflict: '所有权记录与当前包不一致，不会删除。',
  plugin_import_ownership_not_registered: '包已导入，但未建立可移除的受管所有权，请勿重复导入。',
  plugin_remove_discovery_unavailable: '当前本地发现状态不足以安全确认移除目标。',
  plugin_remove_not_managed: '当前包不是 EasiFlux 受管包。',
  plugin_remove_requires_disabled: '请先停用再移除。',
  plugin_remove_storage_unavailable: '无法安全打开或准备当前包与移除暂存区。',
  plugin_remove_identity_changed: '确认后包内容或对象已变化，请重新扫描。',
  plugin_remove_staging_capacity_exceeded: '移除暂存区需要人工检查。',
  plugin_remove_write_failed: '未能完成包隔离，请重新扫描。',
  plugin_remove_publication_unconfirmed: '移除可能已提交，但目录结果未确认，请重新扫描。',
} satisfies Record<PluginErrorCode, string>

const SNAPSHOT_KEYS = [
  'schemaVersion',
  'revision',
  'catalogGeneration',
  'availability',
  'availabilityReasonCode',
  'localDiscovery',
  'managedOwnership',
  'plugins',
] as const
const ITEM_KEYS = [
  'manifest',
  'source',
  'management',
  'canRemove',
  'toggleBlockReasonCode',
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
const MUTATION_KEYS = ['schemaVersion', 'revision', 'catalogGeneration', 'plugin'] as const
const LOCAL_DISCOVERY_KEYS = ['status', 'rejectedPackageCount'] as const
const MANAGED_OWNERSHIP_KEYS = [
  'status', 'conflictingEntryCount', 'rollbackPendingCount', 'cleanupPendingCount',
] as const
const ERROR_KEYS = ['code', 'message'] as const
const PREPARE_CANCELLED_KEYS = ['schemaVersion', 'status'] as const
const PREPARE_READY_KEYS = [
  'schemaVersion',
  'status',
  'token',
  'expiresInSeconds',
  'catalogGeneration',
  'manifest',
] as const
const IMPORTED_KEYS = ['schemaVersion', 'status', 'pluginId', 'snapshot'] as const
const NOT_IMPORTED_KEYS = [
  'schemaVersion',
  'status',
  'disabledDecisionSaved',
  'reasonCode',
  'snapshot',
] as const
const IMPORTED_NOT_VISIBLE_KEYS = [
  'schemaVersion',
  'status',
  'pluginId',
  'reasonCode',
  'snapshot',
] as const

const IMPORT_COMMIT_FAILURES = new Set<LocalManifestImportCommitFailure>([
  'plugin_catalog_stale',
  'plugin_catalog_invalid',
  'plugin_catalog_generation_exhausted',
  'plugin_state_unavailable',
  'plugin_state_persist_failed',
  'plugin_state_capacity_exceeded',
  'plugin_revision_exhausted',
  'plugin_import_id_conflict',
  'plugin_import_discovery_unavailable',
  'plugin_import_capacity_exceeded',
  'plugin_import_staging_capacity_exceeded',
  'plugin_import_write_failed',
  'plugin_ownership_unavailable',
  'plugin_ownership_capacity_exceeded',
  'plugin_ownership_revision_exhausted',
])

const FALSE_ONLY_REMOVE_CODES = new Set<RemoveManagedLocalPluginFailure>([
  'plugin_catalog_stale', 'plugin_catalog_invalid', 'plugin_catalog_generation_exhausted',
  'plugin_state_unavailable', 'plugin_state_persist_failed', 'plugin_state_capacity_exceeded',
  'plugin_revision_exhausted', 'plugin_ownership_unavailable', 'plugin_ownership_capacity_exceeded',
  'plugin_ownership_revision_exhausted', 'plugin_ownership_conflict',
  'plugin_remove_discovery_unavailable', 'plugin_remove_not_managed', 'plugin_remove_requires_disabled',
  'plugin_remove_storage_unavailable', 'plugin_remove_staging_capacity_exceeded',
])
const TRUE_ONLY_REMOVE_CODES = new Set<RemoveManagedLocalPluginFailure>([
  'plugin_ownership_persist_failed', 'plugin_remove_write_failed',
])

// Find the checked literal instead of asserting an untrusted string into a union.
function requireMember<T extends string>(value: unknown, allowed: ReadonlySet<T>): T {
  for (const member of allowed) {
    if (member === value) return member
  }
  return invalidResponse()
}

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
  // Match Rust's Unicode whitespace policy plus U+FEFF, retaining trim's rejections.
  if (/^[\p{White_Space}\uFEFF]*$/u.test(text) || textEncoder.encode(text).byteLength > maxBytes) {
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

function parseManagement(value: unknown): PluginManagement {
  switch (value) {
    case 'builtIn':
    case 'managed':
    case 'external':
    case 'removalPending':
    case 'ownershipConflict':
    case 'ownershipUnavailable':
      return value
    default:
      return invalidResponse()
  }
}

function parseCatalogItem(value: unknown): PluginCatalogItem {
  const item = requireExactObject(value, ITEM_KEYS)
  const manifest = parseManifest(item.manifest)
  if (item.source !== 'builtIn' && item.source !== 'localDeclarative') invalidResponse()
  const status = parseStatus(item.status)
  const statusReasonCode = parseReason(item.statusReasonCode)
  const management = parseManagement(item.management)
  if (typeof item.canToggle !== 'boolean' || typeof item.canRemove !== 'boolean') invalidResponse()
  const toggleBlockReasonCode = item.toggleBlockReasonCode
  if (toggleBlockReasonCode !== null && toggleBlockReasonCode !== 'removalPending') invalidResponse()
  requireEmptyArray(item.grantedCapabilities)

  if (
    (item.source === 'builtIn') !== (management === 'builtIn')
    || (status === 'blocked') !== (statusReasonCode !== null)
    || (management === 'removalPending') !== (toggleBlockReasonCode === 'removalPending')
    || (management === 'removalPending' && status !== 'disabled')
    || item.canToggle !== (status !== 'blocked' && toggleBlockReasonCode === null)
    || item.canRemove !== (management === 'managed' && status === 'disabled')
  ) invalidResponse()

  return {
    manifest,
    source: item.source,
    management,
    canRemove: item.canRemove,
    toggleBlockReasonCode,
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
  if (snapshot.schemaVersion !== 3 || !Array.isArray(snapshot.plugins)) invalidResponse()
  const revision = requireCanonicalRevision(snapshot.revision)
  const catalogGeneration = requireCanonicalRevision(snapshot.catalogGeneration)
  const localDiscovery = parseLocalDiscovery(snapshot.localDiscovery)
  const managedOwnership = parseManagedOwnership(snapshot.managedOwnership)
  const availability = parseAvailability(snapshot.availability)
  const availabilityReasonCode = parseReason(snapshot.availabilityReasonCode)
  const plugins = snapshot.plugins.map(parseCatalogItem)

  if (
    localDiscovery.status === 'unavailable'
    && plugins.some((plugin) => plugin.source === 'localDeclarative')
  ) invalidResponse()

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
      || plugin.canRemove
      || plugin.statusReasonCode !== availabilityReasonCode
    ))
  ) invalidResponse()

  return {
    schemaVersion: 3,
    revision,
    catalogGeneration,
    localDiscovery,
    managedOwnership,
    availability,
    availabilityReasonCode,
    plugins,
  }
}

function parseMutation(value: unknown): PluginCatalogMutationResult {
  const mutation = requireExactObject(value, MUTATION_KEYS)
  if (mutation.schemaVersion !== 3) invalidResponse()
  const revision = requireCanonicalRevision(mutation.revision)
  const catalogGeneration = requireCanonicalRevision(mutation.catalogGeneration)
  const plugin = parseCatalogItem(mutation.plugin)
  if (plugin.status === 'blocked' || !plugin.canToggle) invalidResponse()
  return { schemaVersion: 3, revision, catalogGeneration, plugin }
}

function requireBoundedCount(value: unknown, maximum: number): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0 || value > maximum) {
    invalidResponse()
  }
  return value
}

function parseManagedOwnership(value: unknown): ManagedOwnershipSummary {
  const summary = requireExactObject(value, MANAGED_OWNERSHIP_KEYS)
  const { status } = summary
  if (status !== 'available' && status !== 'degraded' && status !== 'unavailable') invalidResponse()
  const conflictingEntryCount = requireBoundedCount(summary.conflictingEntryCount, 176)
  const rollbackPendingCount = requireBoundedCount(summary.rollbackPendingCount, 160)
  const cleanupPendingCount = requireBoundedCount(summary.cleanupPendingCount, 160)
  const total = conflictingEntryCount + rollbackPendingCount + cleanupPendingCount
  if (total > 176 || (status === 'degraded' ? total === 0 : total !== 0)) invalidResponse()
  return { status, conflictingEntryCount, rollbackPendingCount, cleanupPendingCount }
}

function parseLocalDiscovery(value: unknown): PluginLocalDiscoverySummary {
  const summary = requireExactObject(value, LOCAL_DISCOVERY_KEYS)
  const { status, rejectedPackageCount } = summary
  if (status !== 'available' && status !== 'degraded' && status !== 'unavailable') {
    invalidResponse()
  }
  if (
    typeof rejectedPackageCount !== 'number'
    || !Number.isInteger(rejectedPackageCount)
    || rejectedPackageCount < 0
    || rejectedPackageCount > 256
    || (status === 'degraded' ? rejectedPackageCount === 0 : rejectedPackageCount !== 0)
  ) invalidResponse()
  return { status, rejectedPackageCount }
}

function requireImportToken(value: unknown): string {
  const token = requireString(value)
  if (!/^[0-9a-f]{32}$/.test(token)) invalidResponse()
  return token
}

function parseImportCommitFailure(value: unknown): LocalManifestImportCommitFailure {
  return requireMember(value, IMPORT_COMMIT_FAILURES)
}

function manifestsMatch(left: PluginManifestV1, right: PluginManifestV1): boolean {
  return left.schemaVersion === right.schemaVersion
    && left.id === right.id
    && left.publisherId === right.publisherId
    && left.publisher === right.publisher
    && left.name === right.name
    && left.description === right.description
    && left.version === right.version
    && left.contributions.length === right.contributions.length
    && left.requestedCapabilities.length === right.requestedCapabilities.length
}

function parsePrepareImport(value: unknown): PrepareLocalManifestImportResult {
  if (!isObject(value)) invalidResponse()

  if (value.status === 'cancelled') {
    const result = requireExactObject(value, PREPARE_CANCELLED_KEYS)
    if (result.schemaVersion !== 1) invalidResponse()
    return { schemaVersion: 1, status: 'cancelled' }
  }

  if (value.status === 'ready') {
    const result = requireExactObject(value, PREPARE_READY_KEYS)
    if (result.schemaVersion !== 1 || result.expiresInSeconds !== 300) invalidResponse()
    return {
      schemaVersion: 1,
      status: 'ready',
      token: requireImportToken(result.token),
      expiresInSeconds: 300,
      catalogGeneration: requireCanonicalRevision(result.catalogGeneration),
      manifest: parseManifest(result.manifest),
    }
  }

  return invalidResponse()
}

function parseCancelImport(value: unknown): CancelLocalManifestImportResult {
  const result = requireExactObject(value, PREPARE_CANCELLED_KEYS)
  if (result.schemaVersion !== 1 || result.status !== 'cancelled') invalidResponse()
  return { schemaVersion: 1, status: 'cancelled' }
}

function parseCommitImport(
  value: unknown,
  expectedManifest: PluginManifestV1,
): CommitLocalManifestImportResult {
  if (!isObject(value)) invalidResponse()

  if (value.status === 'imported' || value.status === 'importedExternal') {
    const external = value.status === 'importedExternal'
    const result = requireExactObject(value, external ? IMPORTED_NOT_VISIBLE_KEYS : IMPORTED_KEYS)
    if (
      result.schemaVersion !== 2
      || (external && result.reasonCode !== 'plugin_import_ownership_not_registered')
    ) invalidResponse()
    const pluginId = requireReverseDomainId(result.pluginId)
    const snapshot = parseSnapshot(result.snapshot)
    const matchingItems = snapshot.plugins.filter((plugin) => plugin.manifest.id === pluginId)
    if (
      pluginId !== expectedManifest.id
      || snapshot.availability !== 'available'
      || matchingItems.length !== 1
      || matchingItems[0].source !== 'localDeclarative'
      || matchingItems[0].status !== 'disabled'
      || matchingItems[0].management !== (external ? 'external' : 'managed')
      || matchingItems[0].canRemove !== !external
      || !manifestsMatch(matchingItems[0].manifest, expectedManifest)
    ) invalidResponse()
    return external
      ? { schemaVersion: 2, status: 'importedExternal', pluginId, reasonCode: 'plugin_import_ownership_not_registered', snapshot }
      : { schemaVersion: 2, status: 'imported', pluginId, snapshot }
  }

  if (value.status === 'notImported') {
    const result = requireExactObject(value, NOT_IMPORTED_KEYS)
    if (result.schemaVersion !== 2 || typeof result.disabledDecisionSaved !== 'boolean') {
      invalidResponse()
    }
    const reasonCode = parseImportCommitFailure(result.reasonCode)
    if (
      result.disabledDecisionSaved
      && reasonCode !== 'plugin_import_write_failed'
      && reasonCode !== 'plugin_state_persist_failed'
    ) {
      invalidResponse()
    }
    return {
      schemaVersion: 2,
      status: 'notImported',
      disabledDecisionSaved: result.disabledDecisionSaved,
      reasonCode,
      snapshot: parseSnapshot(result.snapshot),
    }
  }

  if (value.status === 'importedNotVisible') {
    const result = requireExactObject(value, IMPORTED_NOT_VISIBLE_KEYS)
    if (
      result.schemaVersion !== 2
      || result.reasonCode !== 'plugin_import_publication_unconfirmed'
    ) invalidResponse()
    const pluginId = requireReverseDomainId(result.pluginId)
    if (pluginId !== expectedManifest.id) invalidResponse()
    return {
      schemaVersion: 2,
      status: 'importedNotVisible',
      pluginId,
      reasonCode: 'plugin_import_publication_unconfirmed',
      snapshot: parseSnapshot(result.snapshot),
    }
  }

  return invalidResponse()
}

function parseRemoveResult(value: unknown, requestedId: string): RemoveManagedLocalPluginResult {
  if (!isObject(value) || value.schemaVersion !== 1) invalidResponse()

  if (value.status === 'notRemoved') {
    const result = requireExactObject(value, NOT_IMPORTED_KEYS)
    if (typeof result.disabledDecisionSaved !== 'boolean') invalidResponse()
    const reasonCode = result.reasonCode === 'plugin_remove_identity_changed'
      ? result.reasonCode
      : requireMember(result.reasonCode, result.disabledDecisionSaved ? TRUE_ONLY_REMOVE_CODES : FALSE_ONLY_REMOVE_CODES)
    return {
      schemaVersion: 1,
      status: 'notRemoved',
      disabledDecisionSaved: result.disabledDecisionSaved,
      reasonCode,
      snapshot: parseSnapshot(result.snapshot),
    }
  }

  const { status } = value
  if (status !== 'removed' && status !== 'removedCleanupPending' && status !== 'removedCatalogUnconfirmed') {
    invalidResponse()
  }
  const result = requireExactObject(value, status === 'removedCatalogUnconfirmed' ? IMPORTED_NOT_VISIBLE_KEYS : IMPORTED_KEYS)
  const pluginId = requireReverseDomainId(result.pluginId)
  if (pluginId !== requestedId) invalidResponse()
  if (status === 'removedCatalogUnconfirmed' && result.reasonCode !== 'plugin_remove_publication_unconfirmed') {
    invalidResponse()
  }
  const snapshot = parseSnapshot(result.snapshot)
  if (status === 'removedCatalogUnconfirmed') {
    return { schemaVersion: 1, status, pluginId, reasonCode: 'plugin_remove_publication_unconfirmed', snapshot }
  }
  if (snapshot.plugins.some((plugin) => (
    plugin.manifest.id === pluginId
    && (plugin.management === 'managed' || plugin.management === 'removalPending')
  ))) invalidResponse()
  if (status === 'removedCleanupPending' && snapshot.managedOwnership.cleanupPendingCount === 0) invalidResponse()
  return { schemaVersion: 1, status, pluginId, snapshot }
}

export async function getPluginCatalog(): Promise<PluginCatalogSnapshot> {
  return parseSnapshot(await tauriInvoke<unknown>('get_plugin_catalog'))
}

export async function reloadPluginCatalog(): Promise<PluginCatalogSnapshot> {
  return parseSnapshot(await tauriInvoke<unknown>('reload_plugin_catalog'))
}

export async function setPluginEnabled(
  id: string,
  enabled: boolean,
  expectedCatalogGeneration: string,
): Promise<PluginCatalogMutationResult> {
  const result = parseMutation(await tauriInvoke<unknown>('set_plugin_enabled', {
    id,
    enabled,
    expectedCatalogGeneration,
  }))
  const expectedStatus = enabled ? 'enabled' : 'disabled'
  if (
    result.plugin.manifest.id !== id
    || result.plugin.status !== expectedStatus
    || result.catalogGeneration !== expectedCatalogGeneration
  ) invalidResponse()
  return result
}

export async function prepareLocalManifestImport(): Promise<PrepareLocalManifestImportResult> {
  return parsePrepareImport(await tauriInvoke<unknown>('prepare_local_manifest_import'))
}

export async function cancelLocalManifestImport(
  token: string,
): Promise<CancelLocalManifestImportResult> {
  return parseCancelImport(await tauriInvoke<unknown>('cancel_local_manifest_import', { token }))
}

export async function commitLocalManifestImport(
  preview: ReadyLocalManifestImport,
): Promise<CommitLocalManifestImportResult> {
  const value = await tauriInvoke<unknown>('commit_local_manifest_import', {
    token: preview.token,
    expectedCatalogGeneration: preview.catalogGeneration,
  })
  return parseCommitImport(value, preview.manifest)
}

export async function removeManagedLocalPlugin(
  id: string,
  expectedCatalogGeneration: string,
): Promise<RemoveManagedLocalPluginResult> {
  const value = await tauriInvoke<unknown>('remove_managed_local_plugin', {
    id,
    expectedCatalogGeneration,
  })
  return parseRemoveResult(value, id)
}

function isPluginErrorCode(value: string): value is PluginErrorCode {
  return Object.prototype.hasOwnProperty.call(ERROR_MESSAGES, value)
}

export function pluginErrorCode(error: unknown): PluginErrorCode | null {
  if (!hasExactKeys(error, ERROR_KEYS)) return null
  if (typeof error.code !== 'string' || typeof error.message !== 'string') return null
  return isPluginErrorCode(error.code) ? error.code : null
}

export function pluginErrorMessage(error: unknown): string {
  const code = pluginErrorCode(error)
  return code === null ? GENERIC_ERROR : ERROR_MESSAGES[code]
}
