import type {
  PluginCommandContribution,
  PluginManifest,
  PluginManifestDiff,
  PluginManifestField,
} from '../types/plugin'

const FIELD_ORDER: readonly PluginManifestField[] = [
  'schemaVersion',
  'publisherId',
  'publisher',
  'name',
  'description',
  'version',
  'requestedCapabilities',
]

function requestedCapabilitiesEqual(current: PluginManifest, incoming: PluginManifest): boolean {
  return current.requestedCapabilities.length === incoming.requestedCapabilities.length
    && current.requestedCapabilities.every((capability, index) => (
      capability === incoming.requestedCapabilities[index]
    ))
}

function fieldChanged(
  field: PluginManifestField,
  current: PluginManifest,
  incoming: PluginManifest,
): boolean {
  if (field === 'requestedCapabilities') {
    return !requestedCapabilitiesEqual(current, incoming)
  }
  return current[field] !== incoming[field]
}

function commandEqual(
  current: PluginCommandContribution,
  incoming: PluginCommandContribution,
): boolean {
  if (
    current.kind !== incoming.kind
    || current.contributionId !== incoming.contributionId
    || current.title !== incoming.title
    || current.actionId !== incoming.actionId
  ) return false
  if (current.actionId === 'host.showInfo' && incoming.actionId === 'host.showInfo') {
    return current.params.title === incoming.params.title
      && current.params.text === incoming.params.text
  }
  if (current.actionId === 'host.openPage' && incoming.actionId === 'host.openPage') {
    return current.params.destination === incoming.params.destination
  }
  return false
}

export function comparePluginManifests(
  current: PluginManifest,
  incoming: PluginManifest,
): PluginManifestDiff {
  if (current.id !== incoming.id) {
    throw new Error('Plugin manifest comparison requires matching plugin IDs.')
  }

  const changedFields = FIELD_ORDER.filter((field) => fieldChanged(field, current, incoming))
  const currentById = new Map(current.contributions.map((command) => [
    command.contributionId,
    command,
  ]))
  const incomingById = new Map(incoming.contributions.map((command) => [
    command.contributionId,
    command,
  ]))
  const added = incoming.contributions.filter((command) => !currentById.has(command.contributionId))
  const removed = current.contributions.filter((command) => !incomingById.has(command.contributionId))
  const changed = incoming.contributions.flatMap((after) => {
    const before = currentById.get(after.contributionId)
    return before && !commandEqual(before, after) ? [{ before, after }] : []
  })
  const currentCommon = current.contributions
    .filter((command) => incomingById.has(command.contributionId))
    .map((command) => command.contributionId)
  const incomingCommon = incoming.contributions
    .filter((command) => currentById.has(command.contributionId))
    .map((command) => command.contributionId)
  const orderChanged = currentCommon.length !== incomingCommon.length
    || currentCommon.some((id, index) => id !== incomingCommon[index])
  const sameContent = changedFields.length === 0
    && added.length === 0
    && removed.length === 0
    && changed.length === 0
    && !orderChanged

  return {
    changedFields,
    added,
    removed,
    changed,
    orderChanged,
    sameContent,
    publisherIdChanged: current.publisherId !== incoming.publisherId,
  }
}
