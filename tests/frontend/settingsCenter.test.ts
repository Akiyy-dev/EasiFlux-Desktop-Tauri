import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it } from 'vitest'
import SettingsCenterPage from '../../src/components/settings/SettingsCenterPage.vue'
import { SETTINGS_SECTION_BY_KEY, SETTINGS_SECTION_GROUPS } from '../../src/components/settings/settingsSections'
import { useAppStore } from '../../src/stores/app'

const placeholderDescriptions = {
  plugins: '管理插件的启用状态、权限与插件级配置。',
  notifications: '配置通知渠道、提醒方式与免打扰规则。',
  searchCommands: '配置全局搜索与命令面板的行为。',
  hotkeys: '查看并管理应用快捷键。',
  workspace: '配置窗口布局、工作区保存与恢复行为。',
  appearance: '配置主题、颜色与界面显示方式。',
  languageRegion: '配置语言、地区与时间格式。',
} as const

let pinia: Pinia

function mountCenter(props: Record<string, unknown> = {}) {
  return mount(SettingsCenterPage, {
    props,
    global: {
      plugins: [pinia],
      stubs: {
        GeneralSettingsPanel: {
          template: '<div data-testid="general-settings-stub" />',
        },
        AccountSettingsPage: {
          props: ['initialSection'],
          template: '<div data-testid="account-settings-stub" />',
        },
      },
    },
  })
}

describe('SettingsCenterPage presentation', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
  })

  it('keeps the immutable metadata ordered and complete', () => {
    expect(SETTINGS_SECTION_GROUPS.map((group) => group.key)).toEqual([
      'basic', 'features', 'personalization', 'system',
    ])
    expect(Object.keys(SETTINGS_SECTION_BY_KEY)).toEqual([
      'general', 'account', 'plugins', 'notifications', 'searchCommands', 'hotkeys',
      'workspace', 'appearance', 'languageRegion', 'about',
    ])
    expect(SETTINGS_SECTION_GROUPS.flatMap((group) => group.items.map((item) => item.key)))
      .toEqual(Object.keys(SETTINGS_SECTION_BY_KEY))
  })

  it('renders ordered navigation groups and General by default', () => {
    const wrapper = mountCenter()

    expect(wrapper.findAll('[data-testid^="settings-nav-"]').map((item) => item.text()))
      .toEqual([
        '通用', '账户', '插件', '通知', '搜索与命令', '快捷键',
        '工作区与窗口', '外观', '语言与地区', '关于',
      ])
    expect(wrapper.findAll('[data-testid="settings-group-heading"]')).toHaveLength(4)
    expect(wrapper.get('[data-testid="settings-nav-general"]').attributes('aria-current'))
      .toBe('page')
    expect(wrapper.get('[data-testid="general-settings-stub"]').exists()).toBe(true)
  })

  it.each(Object.entries(placeholderDescriptions))(
    'renders the %s placeholder without interactive controls',
    async (key, description) => {
      const wrapper = mountCenter()
      await wrapper.get(`[data-testid="settings-nav-${key}"]`).trigger('click')

      const placeholder = wrapper.get('[data-testid="settings-placeholder"]')
      expect(placeholder.text()).toContain(description)
      expect(placeholder.text()).toContain('规划中')
      expect(placeholder.findAll('button, a[href], input, select, textarea, [role="switch"], [tabindex]'))
        .toHaveLength(0)
    },
  )

  it('shows the ready app version in About', async () => {
    useAppStore().markReady('0.4.1-test')
    const wrapper = mountCenter()
    await wrapper.get('[data-testid="settings-nav-about"]').trigger('click')

    expect(wrapper.text()).toContain('EasiFlux')
    expect(wrapper.text()).toContain('0.4.1-test')
  })

  it('forwards the Account deep link without mounting private panels', () => {
    const wrapper = mountCenter({ initialSection: 'account', initialAccountSection: 'assets' })

    expect(wrapper.findComponent('[data-testid="account-settings-stub"]').props('initialSection'))
      .toBe('assets')
  })

  it('emits back once from the header control', async () => {
    const wrapper = mountCenter()
    await wrapper.get('[data-testid="settings-back"]').trigger('click')

    expect(wrapper.emitted('back')).toHaveLength(1)
  })
})
