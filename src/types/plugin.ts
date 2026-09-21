export type PluginSource = 'builtIn' | 'localDeclarative'
export type LocalDiscoveryStatus = 'available' | 'degraded' | 'unavailable'
export type PluginStatus = 'enabled' | 'disabled' | 'blocked'
export type PluginAvailability = 'available' | 'unavailable'
export type PluginAvailabilityReason = 'stateUnavailable' | 'catalogInvalid'
import type { PluginWorkflowCapability } from './pluginWorkflow'
export type PluginManagement =
  | 'builtIn'
  | 'managed'
  | 'external'
  | 'removalPending'
  | 'ownershipConflict'
  | 'ownershipUnavailable'
export type PluginToggleBlockReason = 'removalPending'

export interface ManagedOwnershipSummary {
  status: 'available' | 'degraded' | 'unavailable'
  conflictingEntryCount: number
  rollbackPendingCount: number
  cleanupPendingCount: number
}

export interface PluginLocalDiscoverySummary {
  status: LocalDiscoveryStatus
  rejectedPackageCount: number
}

export interface PluginManifestV1 {
  schemaVersion: 1
  id: string
  publisherId: string
  publisher: string
  name: string
  description: string
  version: string
  contributions: []
  requestedCapabilities: []
}

export type PluginPageDestination =
  | 'home'
  | 'trading'
  | 'charts'
  | 'settings.general'
  | 'settings.notifications'
  | 'settings.about'

export interface PluginShowInfoCommandContribution {
  kind: 'command'
  contributionId: string
  title: string
  actionId: 'host.showInfo'
  params: { title: string; text: string }
}

export interface PluginOpenPageCommandContribution {
  kind: 'command'
  contributionId: string
  title: string
  actionId: 'host.openPage'
  params: { destination: PluginPageDestination }
}

export interface PluginComputeParameterMetadata {
  label: string
  default: number
  min: number
  max: number
}

export interface PluginComputeCommandContribution {
  kind: 'command'
  contributionId: string
  title: string
  actionId: 'sandbox.computeSeries'
  params: {
    runtime: 'wasm-v1'
    abi: 'series-f64-v1'
    moduleBase64: string
    parameter: PluginComputeParameterMetadata
  }
}

export interface PluginWorkflowCommandContribution {
  kind: 'command'
  contributionId: string
  title: string
  actionId: 'sandbox.accountWorkflow'
  params: {
    runtime: 'wasm-v1'
    abi: 'account-json-v1'
    moduleBase64: string
    defaultInput: string
  }
}

export type PluginCommandContribution =
  | PluginShowInfoCommandContribution
  | PluginOpenPageCommandContribution
  | PluginComputeCommandContribution
  | PluginWorkflowCommandContribution

export type PluginV4CommandContribution = Exclude<
  PluginCommandContribution,
  PluginWorkflowCommandContribution
>

export type PluginV3CommandContribution =
  | PluginShowInfoCommandContribution
  | PluginOpenPageCommandContribution

export interface PluginManifestV2 extends Omit<PluginManifestV1, 'schemaVersion' | 'contributions'> {
  schemaVersion: 2
  contributions: PluginShowInfoCommandContribution[]
}

export interface PluginManifestV3 extends Omit<PluginManifestV1, 'schemaVersion' | 'contributions'> {
  schemaVersion: 3
  contributions: PluginV3CommandContribution[]
}

export interface PluginManifestV4 extends Omit<PluginManifestV1, 'schemaVersion' | 'contributions'> {
  schemaVersion: 4
  contributions: PluginV4CommandContribution[]
}

export interface PluginManifestV5 extends Omit<
  PluginManifestV1,
  'schemaVersion' | 'contributions' | 'requestedCapabilities'
> {
  schemaVersion: 5
  contributions: PluginCommandContribution[]
  requestedCapabilities: PluginWorkflowCapability[]
}

export type PluginManifest =
  | PluginManifestV1
  | PluginManifestV2
  | PluginManifestV3
  | PluginManifestV4
  | PluginManifestV5

export type PluginManifestField =
  | 'schemaVersion'
  | 'publisherId'
  | 'publisher'
  | 'name'
  | 'description'
  | 'version'
  | 'requestedCapabilities'

export interface PluginManifestDiff {
  changedFields: PluginManifestField[]
  added: PluginCommandContribution[]
  removed: PluginCommandContribution[]
  changed: { before: PluginCommandContribution; after: PluginCommandContribution }[]
  orderChanged: boolean
  sameContent: boolean
  publisherIdChanged: boolean
}

export interface PluginCommandInfo {
  pluginId: string
  pluginName: string
  contributionId: string
  title: string
  text: string
}

interface PluginCommandSummaryBase {
  pluginId: string
  pluginName: string
  contributionId: string
  title: string
}

export type PluginCommandSummary =
  | (PluginCommandSummaryBase & { actionId: 'host.showInfo' })
  | (PluginCommandSummaryBase & {
      actionId: 'host.openPage'
      destination: PluginPageDestination
    })
  | (PluginCommandSummaryBase & {
      actionId: 'sandbox.computeSeries'
      parameter: PluginComputeParameterMetadata
    })
  | (PluginCommandSummaryBase & {
      actionId: 'sandbox.accountWorkflow'
      requestedCapabilities: PluginWorkflowCapability[]
    })

export type PluginCommandExecution =
  | { actionId: 'host.showInfo'; info: PluginCommandInfo }
  | {
      actionId: 'host.openPage'
      pluginId: string
      pluginName: string
      contributionId: string
      destination: PluginPageDestination
    }
  | {
      actionId: 'sandbox.computeSeries'
      pluginId: string
      pluginName: string
      contributionId: string
      title: string
      runtime: 'wasm-v1'
      abi: 'series-f64-v1'
      parameter: PluginComputeParameterMetadata
      expectedCatalogGeneration: string
      expectedRevision: string
    }
  | {
      actionId: 'sandbox.accountWorkflow'
      pluginId: string
      pluginName: string
      contributionId: string
      title: string
      runtime: 'wasm-v1'
      abi: 'account-json-v1'
      defaultInput: string
      requestedCapabilities: PluginWorkflowCapability[]
      expectedCatalogGeneration: string
      expectedRevision: string
    }

export type PluginComputeExecutionIntent = Extract<
  PluginCommandExecution,
  { actionId: 'sandbox.computeSeries' }
>

export type PluginWorkflowExecutionIntent = Extract<
  PluginCommandExecution,
  { actionId: 'sandbox.accountWorkflow' }
>

export interface PluginComputeRequest {
  requestId: string
  pluginId: string
  contributionId: string
  expectedCatalogGeneration: string
  expectedRevision: string
  values: number[]
  parameter: number
}

export interface PluginComputeResult {
  schemaVersion: 1
  requestId: string
  pluginId: string
  contributionId: string
  catalogGeneration: string
  revision: string
  value: number
  inputCount: number
  parameter: number
}

export interface PluginComputeCancelResult {
  schemaVersion: 1
  requestId: string
  cancelled: boolean
}

export interface PluginCatalogItem {
  manifest: PluginManifest
  source: PluginSource
  management: PluginManagement
  canRemove: boolean
  toggleBlockReasonCode: PluginToggleBlockReason | null
  status: PluginStatus
  statusReasonCode: PluginAvailabilityReason | null
  canToggle: boolean
  grantedCapabilities: []
}

export interface PluginCatalogSnapshot {
  schemaVersion: 3
  revision: string
  catalogGeneration: string
  availability: PluginAvailability
  availabilityReasonCode: PluginAvailabilityReason | null
  localDiscovery: PluginLocalDiscoverySummary
  managedOwnership: ManagedOwnershipSummary
  plugins: PluginCatalogItem[]
}

export interface PluginCatalogMutationResult {
  schemaVersion: 3
  revision: string
  catalogGeneration: string
  plugin: PluginCatalogItem
}

export type LocalManifestImportCommitFailure =
  | 'plugin_catalog_stale'
  | 'plugin_catalog_invalid'
  | 'plugin_catalog_generation_exhausted'
  | 'plugin_state_unavailable'
  | 'plugin_state_persist_failed'
  | 'plugin_state_capacity_exceeded'
  | 'plugin_revision_exhausted'
  | 'plugin_import_id_conflict'
  | 'plugin_import_discovery_unavailable'
  | 'plugin_import_capacity_exceeded'
  | 'plugin_import_staging_capacity_exceeded'
  | 'plugin_import_write_failed'
  | 'plugin_ownership_unavailable'
  | 'plugin_ownership_capacity_exceeded'
  | 'plugin_ownership_revision_exhausted'

export type ImportAssessment =
  | { kind: 'notInCatalog' }
  | {
      kind: 'existingId'
      current: PluginCatalogItem
      versionRelation: 'incomingLower' | 'samePrecedence' | 'incomingHigher'
    }

export type PrepareLocalManifestImportResult =
  | {
      schemaVersion: 1
      status: 'cancelled'
    }
  | {
      schemaVersion: 2
      status: 'ready'
      token: string
      expiresInSeconds: 300
      catalogGeneration: string
      manifest: PluginManifest
      assessment: ImportAssessment
    }

export type ReadyLocalManifestImport = Extract<
  PrepareLocalManifestImportResult,
  { status: 'ready' }
>

export interface CancelLocalManifestImportResult {
  schemaVersion: 1
  status: 'cancelled'
}

export type CommitLocalManifestImportResult =
  | {
      schemaVersion: 2
      status: 'imported'
      pluginId: string
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 2
      status: 'importedExternal'
      pluginId: string
      reasonCode: 'plugin_import_ownership_not_registered'
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 2
      status: 'notImported'
      disabledDecisionSaved: boolean
      reasonCode: LocalManifestImportCommitFailure
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 2
      status: 'importedNotVisible'
      pluginId: string
      reasonCode: 'plugin_import_publication_unconfirmed'
      snapshot: PluginCatalogSnapshot
    }

export type RemoveManagedLocalPluginFailure =
  | 'plugin_catalog_stale'
  | 'plugin_catalog_invalid'
  | 'plugin_catalog_generation_exhausted'
  | 'plugin_state_unavailable'
  | 'plugin_state_persist_failed'
  | 'plugin_state_capacity_exceeded'
  | 'plugin_revision_exhausted'
  | 'plugin_ownership_unavailable'
  | 'plugin_ownership_persist_failed'
  | 'plugin_ownership_capacity_exceeded'
  | 'plugin_ownership_revision_exhausted'
  | 'plugin_ownership_conflict'
  | 'plugin_remove_discovery_unavailable'
  | 'plugin_remove_not_managed'
  | 'plugin_remove_requires_disabled'
  | 'plugin_remove_storage_unavailable'
  | 'plugin_remove_identity_changed'
  | 'plugin_remove_staging_capacity_exceeded'
  | 'plugin_remove_write_failed'

export type RemoveManagedLocalPluginResult =
  | {
      schemaVersion: 1
      status: 'removed'
      pluginId: string
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'removedCleanupPending'
      pluginId: string
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'removedCatalogUnconfirmed'
      pluginId: string
      reasonCode: 'plugin_remove_publication_unconfirmed'
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'notRemoved'
      disabledDecisionSaved: boolean
      reasonCode: RemoveManagedLocalPluginFailure
      snapshot: PluginCatalogSnapshot
    }
