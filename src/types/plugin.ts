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
