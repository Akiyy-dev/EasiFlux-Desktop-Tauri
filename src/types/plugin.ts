export type PluginSource = 'builtIn'
export type PluginStatus = 'enabled' | 'disabled' | 'blocked'
export type PluginAvailability = 'available' | 'unavailable'
export type PluginAvailabilityReason = 'stateUnavailable' | 'catalogInvalid'

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
  schemaVersion: 1
  revision: string
  availability: PluginAvailability
  availabilityReasonCode: PluginAvailabilityReason | null
  plugins: PluginCatalogItem[]
}

export interface PluginCatalogMutationResult {
  revision: string
  plugin: PluginCatalogItem
}
