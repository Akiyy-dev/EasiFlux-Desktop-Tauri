export type PluginSource = 'builtIn' | 'localDeclarative'
export type LocalDiscoveryStatus = 'available' | 'degraded' | 'unavailable'
export type PluginStatus = 'enabled' | 'disabled' | 'blocked'
export type PluginAvailability = 'available' | 'unavailable'
export type PluginAvailabilityReason = 'stateUnavailable' | 'catalogInvalid'
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

export interface PluginCatalogItem {
  manifest: PluginManifestV1
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

export type PrepareLocalManifestImportResult =
  | {
      schemaVersion: 1
      status: 'cancelled'
    }
  | {
      schemaVersion: 1
      status: 'ready'
      token: string
      expiresInSeconds: 300
      catalogGeneration: string
      manifest: PluginManifestV1
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
