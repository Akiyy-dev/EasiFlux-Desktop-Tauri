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

function hostOnlyManifest(): PluginManifestV4 {
  return {
    ...manifest('AGFzbQEAAAA='),
    contributions: [{
      kind: 'command', contributionId: 'analytics.guide', title: 'Show guide',
      actionId: 'host.showInfo', params: { title: 'Guide', text: 'Read only' },
    }],
  }
}

describe('compute manifest comparison', () => {
  it('discloses execution intent and memory-only data for same-content compute comparison', () => {
    const current = item('AGFzbQEAAAA=')
    const wrapper = mount(PluginManifestComparison, {
      props: {
        current,
        incoming: structuredClone(current.manifest),
        versionRelation: 'samePrecedence',
      },
    })

    const notice = wrapper.get('[data-testid="plugin-compute-comparison-notice"]')
    expect(wrapper.get('[data-testid="plugin-manifest-unchanged"]').exists()).toBe(true)
    expect(notice.text()).toContain('可执行的本地 WebAssembly 代码')
    expect(notice.text()).toContain('明确点击运行')
    expect(notice.text()).toContain('输入和结果仅保存在内存中')
  })

  it('keeps the compute disclosure when only the current manifest contains code', () => {
    const wrapper = mount(PluginManifestComparison, {
      props: {
        current: item('AGFzbQEAAAA='),
        incoming: hostOnlyManifest(),
        versionRelation: 'samePrecedence',
      },
    })

    expect(wrapper.get('[data-testid="plugin-compute-comparison-notice"]').text())
      .toContain('比较本身不会执行代码')
    expect(wrapper.find('[data-testid="plugin-compute-code-change"]').exists()).toBe(false)
  })

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

  it('discloses v5 workflow permission and confirmation changes without rendering module bytes', () => {
    const current = item('AGFzbQEAAAA=')
    const incoming = {
      ...manifest('AGFzbQEAAAA='),
      schemaVersion: 5 as const,
      requestedCapabilities: ['account.read', 'balances.read', 'trade.place'] as const,
      contributions: [{
        kind: 'command' as const,
        contributionId: 'trader.prepare',
        title: 'Prepare order',
        actionId: 'sandbox.accountWorkflow' as const,
        params: {
          runtime: 'wasm-v1' as const,
          abi: 'account-json-v1' as const,
          moduleBase64: 'AGFzbQEAAAA=',
          defaultInput: '{"qty":"0.001"}',
        },
      }],
    }
    const wrapper = mount(PluginManifestComparison, {
      props: { current, incoming, versionRelation: 'incomingHigher' },
    })

    expect(wrapper.get('[data-testid="plugin-workflow-comparison-notice"]').text())
      .toContain('启用不会授权')
    expect(wrapper.get('[data-testid="plugin-workflow-comparison-notice"]').text())
      .toContain('真实交易仍需单独确认')
    expect(wrapper.text()).toContain('account.read、balances.read、trade.place')
    expect(wrapper.text()).toContain('{"qty":"0.001"}')
    expect(wrapper.text()).not.toContain('AGFzbQEAAAA=')
  })
})
