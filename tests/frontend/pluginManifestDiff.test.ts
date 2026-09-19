import { describe, expect, it } from 'vitest'
import { comparePluginManifests } from '../../src/services/pluginManifestDiff'
import type { PluginCommandContribution, PluginManifest } from '../../src/types/plugin'

function info(
  contributionId: string,
  title = 'Show guide',
  text = 'Read-only guide',
): PluginCommandContribution {
  return {
    kind: 'command', contributionId, title, actionId: 'host.showInfo',
    params: { title: 'Guide', text },
  }
}

function page(
  contributionId: string,
  destination: 'home' | 'trading' | 'charts' | 'settings.general' | 'settings.notifications',
  title = `Open ${destination}`,
): PluginCommandContribution {
  return {
    kind: 'command', contributionId, title, actionId: 'host.openPage',
    params: { destination },
  }
}

function manifest(
  schemaVersion: 1 | 2 | 3 = 3,
  contributions: PluginCommandContribution[] = [],
  overrides: Partial<PluginManifest> = {},
): PluginManifest {
  return {
    schemaVersion,
    id: 'com.example.workspace',
    publisherId: 'com.example',
    publisher: 'Example',
    name: 'Workspace',
    description: 'Workspace shortcuts',
    version: '1.0.0',
    contributions,
    requestedCapabilities: [],
    ...overrides,
  } as PluginManifest
}

describe('comparePluginManifests', () => {
  it('reports every metadata field independently and keeps publisher identity distinct', () => {
    const current = manifest(1)
    const incoming = manifest(3, [page('workspace.home', 'home')], {
      publisherId: 'org.example',
      publisher: 'Example Display',
      name: 'Workspace Next',
      description: 'Changed description',
      version: '2.0.0',
      requestedCapabilities: ['network'],
    } as unknown as Partial<PluginManifest>)

    const diff = comparePluginManifests(current, incoming)

    expect(diff.changedFields).toEqual([
      'schemaVersion', 'publisherId', 'publisher', 'name', 'description', 'version',
      'requestedCapabilities',
    ])
    expect(diff.publisherIdChanged).toBe(true)
    expect(diff.sameContent).toBe(false)
  })

  it('treats reordered object keys as equal and never mutates either manifest', () => {
    const current = manifest(3, [info('workspace.guide')])
    const incoming = {
      requestedCapabilities: [],
      contributions: [{
        params: { text: 'Read-only guide', title: 'Guide' },
        actionId: 'host.showInfo',
        title: 'Show guide',
        contributionId: 'workspace.guide',
        kind: 'command',
      }],
      version: '1.0.0',
      description: 'Workspace shortcuts',
      name: 'Workspace',
      publisher: 'Example',
      publisherId: 'com.example',
      id: 'com.example.workspace',
      schemaVersion: 3,
    } as PluginManifest
    const beforeCurrent = structuredClone(current)
    const beforeIncoming = structuredClone(incoming)
    Object.freeze(current)
    Object.freeze(incoming)

    expect(comparePluginManifests(current, incoming)).toEqual({
      changedFields: [], added: [], removed: [], changed: [],
      orderChanged: false, sameContent: true, publisherIdChanged: false,
    })
    expect(current).toEqual(beforeCurrent)
    expect(incoming).toEqual(beforeIncoming)
  })

  it('rejects unrelated plugin IDs instead of comparing them', () => {
    expect(() => comparePluginManifests(
      manifest(),
      manifest(3, [], { id: 'com.example.other' }),
    )).toThrow('matching plugin IDs')
  })

  it('keeps build-only version text and same-version command text changes visible', () => {
    const build = comparePluginManifests(
      manifest(2, [info('workspace.guide')], { version: '1.0.0+old' }),
      manifest(2, [info('workspace.guide')], { version: '1.0.0+new' }),
    )
    expect(build.changedFields).toEqual(['version'])
    expect(build.sameContent).toBe(false)

    const text = comparePluginManifests(
      manifest(2, [info('workspace.guide')]),
      manifest(2, [info('workspace.guide', 'Show guide', 'Changed text')]),
    )
    expect(text.changedFields).toEqual([])
    expect(text.changed.map(({ after }) => after.contributionId)).toEqual(['workspace.guide'])
    expect(text.sameContent).toBe(false)
  })

  it('reports added, removed, action-specific changes, and relative common-command order', () => {
    const current = manifest(3, [
      page('workspace.charts', 'charts'),
      page('workspace.trading', 'trading'),
      page('workspace.notifications', 'settings.notifications'),
      info('workspace.help'),
    ])
    const incoming = manifest(3, [
      page('workspace.notifications', 'settings.notifications'),
      page('workspace.charts', 'home', 'Open home'),
      page('workspace.general', 'settings.general'),
      info('workspace.help'),
    ])

    const diff = comparePluginManifests(current, incoming)

    expect(diff.added.map(({ contributionId }) => contributionId)).toEqual(['workspace.general'])
    expect(diff.removed.map(({ contributionId }) => contributionId)).toEqual(['workspace.trading'])
    expect(diff.changed.map(({ after }) => after.contributionId)).toEqual(['workspace.charts'])
    expect(diff.orderChanged).toBe(true)
    expect(diff.publisherIdChanged).toBe(false)
  })

  it('compares v1, v2, and v3 without treating schema changes as command equality', () => {
    const v1 = manifest(1)
    const v2 = manifest(2, [info('workspace.open')])
    const v3 = manifest(3, [page('workspace.open', 'home')])

    const first = comparePluginManifests(v1, v2)
    expect(first.changedFields).toEqual(['schemaVersion'])
    expect(first.added.map(({ contributionId }) => contributionId)).toEqual(['workspace.open'])
    const second = comparePluginManifests(v2, v3)
    expect(second.changedFields).toEqual(['schemaVersion'])
    expect(second.changed.map(({ before, after }) => [before.actionId, after.actionId]))
      .toEqual([['host.showInfo', 'host.openPage']])
  })
})
