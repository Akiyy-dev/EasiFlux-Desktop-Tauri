import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import PluginCard from '../../src/components/plugins/PluginCard.vue'
import type { PluginCatalogItem, PluginStatus } from '../../src/types/plugin'

function pluginFixture(
  status: PluginStatus = 'disabled',
  overrides: Partial<PluginCatalogItem> = {},
): PluginCatalogItem {
  return {
    manifest: {
      schemaVersion: 1,
      id: 'com.easiflux.analytics.overview',
      name: '行情概览',
      version: '1.2.3',
      description: '展示受信任的行情摘要。',
      publisherId: 'com.easiflux',
      publisher: 'EasiFlux 团队',
      contributions: [],
      requestedCapabilities: [],
    },
    source: 'builtIn',
    grantedCapabilities: [],
    status,
    canToggle: status !== 'blocked',
    statusReasonCode: status === 'blocked' ? 'stateUnavailable' : null,
    ...overrides,
  }
}

function mountCard(
  plugin: PluginCatalogItem = pluginFixture(),
  pending = false,
  error: string | null = null,
) {
  return mount(PluginCard, { props: { plugin, pending, error } })
}

describe('PluginCard', () => {
  it('renders catalog metadata and Phase 0 permission summaries as inert text', () => {
    const plugin = pluginFixture('disabled', {
      manifest: {
        ...pluginFixture().manifest,
        name: '<img src=x onerror=alert(1)>',
      },
    })
    const wrapper = mountCard(plugin)

    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('1.2.3')
    expect(wrapper.text()).toContain('展示受信任的行情摘要。')
    expect(wrapper.text()).toContain('EasiFlux 团队')
    expect(wrapper.text()).toContain('com.easiflux')
    expect(wrapper.text()).toContain('内置 · 随应用提供')
    expect(wrapper.get('[data-testid="requested-capabilities"]').text())
      .toContain('无需额外权限')
    expect(wrapper.get('[data-testid="granted-capabilities"]').text())
      .toContain('无需额外权限')
  })

  it('discloses local metadata-only preferences without implying verified publishing', () => {
    const wrapper = mountCard(pluginFixture('disabled', { source: 'localDeclarative' }))

    expect(wrapper.text()).toContain('本地声明式包 · 已发现，未执行')
    expect(wrapper.text()).toContain('启用仅记录宿主偏好，不会运行插件代码')
    expect(wrapper.text()).not.toMatch(/已认证|已验证|签名认证|可信发布者|内置 · 随应用提供/)
    const control = wrapper.get<HTMLInputElement>('[role="switch"]')
    expect(control.attributes('type')).toBe('checkbox')
    expect(control.attributes('aria-label')).toBe('行情概览，当前已停用')
    expect(control.attributes('aria-describedby')).toBe('plugin-card-com.easiflux.analytics.overview-status')
    control.element.click()
    expect(wrapper.emitted('toggle')).toEqual([['com.easiflux.analytics.overview', true]])
    expect(control.element.checked).toBe(false)
  })

  it.each([
    ['enabled', '已启用', true],
    ['disabled', '已停用', false],
    ['blocked', '已阻止', false],
  ] as const)('shows %s as visible and accessible switch state', (status, label, checked) => {
    const wrapper = mountCard(pluginFixture(status))
    const statusElement = wrapper.get('[data-testid="plugin-status"]')
    const control = wrapper.get<HTMLInputElement>('[role="switch"]')
    const describedIds = control.attributes('aria-describedby').split(' ')

    expect(statusElement.text()).toContain(label)
    expect(control.element.checked).toBe(checked)
    expect(control.attributes('aria-label')).toContain('行情概览')
    expect(control.attributes('aria-label')).toContain(label)
    expect(describedIds).toEqual([statusElement.attributes('id')])
    expect(wrapper.element.querySelector(`[id="${describedIds[0]}"]`)).not.toBeNull()
  })

  it('disables unavailable and pending cards while exposing a sanitized reason and busy state', async () => {
    const blocked = mountCard(pluginFixture('blocked'))
    const blockedControl = blocked.get<HTMLInputElement>('[role="switch"]')

    expect(blockedControl.element.disabled).toBe(true)
    expect(blocked.get('[data-testid="plugin-status"]').text())
      .toContain('插件状态暂时不可用，请稍后重试。')
    expect(blocked.text()).not.toContain('stateUnavailable')

    const policyDisabled = mountCard(pluginFixture('disabled', { canToggle: false }))
    expect(policyDisabled.get<HTMLInputElement>('[role="switch"]').element.disabled).toBe(true)

    const pending = mountCard(pluginFixture('disabled'), true)
    expect(pending.get('article').attributes('aria-busy')).toBe('true')
    expect(pending.get<HTMLInputElement>('[role="switch"]').element.disabled).toBe(true)
    expect(blocked.get('article').attributes('aria-busy')).toBeUndefined()
  })

  it('emits the requested confirmed-state transition and ignores non-toggleable input', () => {
    const enabled = mountCard(pluginFixture('enabled'))
    enabled.get<HTMLInputElement>('[role="switch"]').element.click()
    expect(enabled.emitted('toggle')).toEqual([
      ['com.easiflux.analytics.overview', false],
    ])

    const disabled = mountCard(pluginFixture('disabled'))
    disabled.get<HTMLInputElement>('[role="switch"]').element.click()
    expect(disabled.emitted('toggle')).toEqual([
      ['com.easiflux.analytics.overview', true],
    ])

    const blocked = mountCard(pluginFixture('blocked'))
    blocked.get<HTMLInputElement>('[role="switch"]').element.click()
    expect(blocked.emitted('toggle')).toBeUndefined()
  })

  it('keeps a native checkbox click on the confirmed prop while emitting one request', () => {
    const wrapper = mountCard(pluginFixture('disabled'))
    const control = wrapper.get<HTMLInputElement>('[role="switch"]')

    control.element.click()

    expect(control.element.checked).toBe(false)
    expect(wrapper.emitted('toggle')).toEqual([
      ['com.easiflux.analytics.overview', true],
    ])
  })

  it('announces a card action error and references only existing description elements', () => {
    const wrapper = mountCard(pluginFixture(), false, '暂时无法保存启用状态。')
    const alert = wrapper.get('[role="alert"]')
    const control = wrapper.get('[role="switch"]')
    const describedIds = control.attributes('aria-describedby').split(' ')

    expect(alert.text()).toBe('暂时无法保存启用状态。')
    expect(describedIds).toEqual([
      wrapper.get('[data-testid="plugin-status"]').attributes('id'),
      alert.attributes('id'),
    ])
    for (const id of describedIds) {
      expect(wrapper.element.querySelector(`[id="${id}"]`)).not.toBeNull()
    }
    expect(describedIds.every(Boolean)).toBe(true)
  })
})
