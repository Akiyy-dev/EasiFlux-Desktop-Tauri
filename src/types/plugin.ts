export type PluginSource = 'builtIn' | 'localDeclarative'
export type LocalDiscoveryStatus = 'available' | 'degraded' | 'unavailable'
export type PluginStatus = 'enabled' | 'disabled' | 'blocked'
export type PluginAvailability = 'available' | 'unavailable'
export type PluginAvailabilityReason = 'stateUnavailable' | 'catalogInvalid'

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
  status: PluginStatus
  statusReasonCode: PluginAvailabilityReason | null
  canToggle: boolean
  grantedCapabilities: []
}

export interface PluginCatalogSnapshot {
  schemaVersion: 2
  revision: string
  catalogGeneration: string
  availability: PluginAvailability
  availabilityReasonCode: PluginAvailabilityReason | null
  localDiscovery: PluginLocalDiscoverySummary
  plugins: PluginCatalogItem[]
}

export interface PluginCatalogMutationResult {
  schemaVersion: 2
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
      schemaVersion: 1
      status: 'imported'
      pluginId: string
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'notImported'
      disabledDecisionSaved: boolean
      reasonCode: LocalManifestImportCommitFailure
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'importedNotVisible'
      pluginId: string
      reasonCode: 'plugin_import_publication_unconfirmed'
      snapshot: PluginCatalogSnapshot
    }
