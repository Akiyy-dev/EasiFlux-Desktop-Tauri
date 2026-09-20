import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import PluginManifestComparison from '../../src/components/plugins/PluginManifestComparison.vue'
import type {
  PluginCatalogItem,
  PluginComputeCommandContribution,
  PluginManifestV4,
} from '../../src/types/plugin'

function command(moduleBase64: string): PluginComputeCommandContribution {
  return {
    kind: 'command', contributionId: 'analytics.average', title: 'Compute average',
    actionId: 'sandbox.computeSeries',
    params: {
      runtime: 'wasm-v1', abi: 'series-f64-v1', moduleBase64,
      parameter: { label: 'Window', default: 3, min: 1, max: 10 },
    },
  }
}

function manifest(moduleBase64: string): PluginManifestV4 {
  return {
    schemaVersion: 4,
    id: 'com.example.analytics',
    publisherId: 'com.example',
    publisher: 'Example',
    name: 'Analytics',
    description: 'Local compute',
    version: '1.0.0',
    requestedCapabilities: [],
    contributions: [command(moduleBase64)],
  }
}

function item(moduleBase64: string): PluginCatalogItem {
  return {
    manifest: manifest(moduleBase64),
    source: 'localDeclarative', management: 'external', canRemove: false,
    toggleBlockReasonCode: null, status: 'disabled', statusReasonCode: null,
    canToggle: true, grantedCapabilities: [],
  }
}

describe('compute manifest comparison', () => {
  it('shows decoded module sizes and a code-change warning without rendering Base64', () => {
    const currentBase64 = 'AGFzbQEAAAA='
    const incomingBase64 = 'AGFzbQEAAAAB'
    const wrapper = mount(PluginManifestComparison, {
      props: {
        current: item(currentBase64),
        incoming: manifest(incomingBase64),
        versionRelation: 'samePrecedence',
      },
    })

    expect(wrapper.get('[data-testid="plugin-compute-code-change"]').text())
      .toContain('代码内容已变化')
    expect(wrapper.text()).toContain('8 字节')
    expect(wrapper.text()).toContain('9 字节')
    expect(wrapper.text()).not.toContain(currentBase64)
    expect(wrapper.text()).not.toContain(incomingBase64)
  })
})
