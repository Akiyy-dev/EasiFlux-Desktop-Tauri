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
    management: 'builtIn',
    canRemove: false,
    toggleBlockReasonCode: null,
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
    const wrapper = mountCard(pluginFixture('disabled', { source: 'localDeclarative', management: 'external' }))

    expect(wrapper.text()).toContain('本地声明式包 · 外部放置，应用不会删除')
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

  it.each([
    ['built-in', { management: 'builtIn', canRemove: false }, '内置 · 随应用提供', false],
    ['external', { source: 'localDeclarative', management: 'external', canRemove: false }, '本地声明式包 · 外部放置，应用不会删除', false],
    ['managed enabled', { source: 'localDeclarative', management: 'managed', status: 'enabled', canRemove: false }, '请先停用', false],
    ['managed disabled', { source: 'localDeclarative', management: 'managed', status: 'disabled', canRemove: true }, '移除本地包', true],
    ['managed blocked', { source: 'localDeclarative', management: 'managed', status: 'blocked', canToggle: false, canRemove: false }, '当前不可移除', false],
    ['ownership conflict', { source: 'localDeclarative', management: 'ownershipConflict', canRemove: false }, '管理记录不一致，需要退出应用后人工检查', false],
    ['ownership unavailable', { source: 'localDeclarative', management: 'ownershipUnavailable', canRemove: false }, '所有权状态暂不可用，应用不会删除', false],
    ['removal pending', { source: 'localDeclarative', management: 'removalPending', canRemove: false, toggleBlockReasonCode: 'removalPending' }, '移除准备待回退，当前不可启停或移除', false],
  ] as const)('renders exact management and removal semantics for %s', (_label, overrides, copy, removable) => {
    const wrapper = mountCard(pluginFixture('disabled', overrides as Partial<PluginCatalogItem>))

    expect(wrapper.text()).toContain(copy)
    expect(wrapper.find('[data-testid="plugin-removal-area"]').exists())
      .toBe(overrides.management === 'managed')
    expect(wrapper.find('[data-testid="plugin-remove-button"]').exists()).toBe(removable)
  })

  it('emits one managed removal request with its connected trigger and guards every other state', async () => {
    const managed = mount(PluginCard, {
      attachTo: document.body,
      props: {
        plugin: pluginFixture('disabled', {
          source: 'localDeclarative', management: 'managed', canRemove: true,
        }),
        pending: false,
        error: null,
      },
    })
    const button = managed.get<HTMLButtonElement>('[data-testid="plugin-remove-button"]')
    await button.trigger('click')
    expect(managed.emitted('remove')).toEqual([[
      'com.easiflux.analytics.overview',
      button.element,
    ]])

    await managed.setProps({ removalDisabled: true })
    expect(button.element.disabled).toBe(true)
    await button.trigger('click')
    expect(managed.emitted('remove')).toHaveLength(1)
    managed.unmount()

    for (const management of ['builtIn', 'external', 'ownershipConflict', 'ownershipUnavailable', 'removalPending'] as const) {
      const guarded = mountCard(pluginFixture('disabled', {
        source: management === 'builtIn' ? 'builtIn' : 'localDeclarative',
        management,
        canRemove: true,
      }))
      expect(guarded.find('[data-testid="plugin-remove-button"]').exists()).toBe(false)
      expect(guarded.emitted('remove')).toBeUndefined()
      guarded.unmount()
    }
  })

  it('blocks toggling for removalPending independently of ownership warning copy', () => {
    const wrapper = mountCard(pluginFixture('disabled', {
      source: 'localDeclarative',
      management: 'removalPending',
      canToggle: true,
      toggleBlockReasonCode: 'removalPending',
    }))
    const control = wrapper.get<HTMLInputElement>('[role="switch"]')

    expect(control.element.disabled).toBe(true)
    expect(wrapper.get('[data-testid="plugin-status"]').text())
      .toContain('移除准备待回退，当前不可启停或移除')
    control.element.click()
    expect(wrapper.emitted('toggle')).toBeUndefined()
  })
})
