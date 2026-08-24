import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { compileStyle, parse } from 'vue/compiler-sfc'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AppShell from '../../src/components/layout/AppShell.vue'
import NavigationRail from '../../src/components/layout/NavigationRail.vue'
import Sidebar from '../../src/components/layout/Sidebar.vue'
import DashboardPage from '../../src/components/dashboard/DashboardPage.vue'
import DashboardQuickActions from '../../src/components/dashboard/DashboardQuickActions.vue'
import SettingsCenterPage from '../../src/components/settings/SettingsCenterPage.vue'
import { SETTINGS_SECTION_BY_KEY, SETTINGS_SECTION_GROUPS } from '../../src/components/settings/settingsSections'
import { useAppStore } from '../../src/stores/app'

vi.mock('../../src/composables/useChartWorkspaceAutosaveHost', () => ({
  useChartWorkspaceAutosaveHost: vi.fn(),
}))
vi.mock('../../src/components/layout/TradingLayout.vue', () => ({
  default: {
    name: 'TradingLayout',
    props: { active: Boolean },
    template: '<div data-testid="trading-layout" />',
  },
}))
vi.mock('../../src/components/market/KlineChart.vue', () => ({
  default: {
    name: 'KlineChart',
    props: { mode: String, active: Boolean },
    template: '<div data-testid="kline-chart" />',
  },
}))

const placeholderDescriptions = {
  plugins: '管理插件的启用状态、权限与插件级配置。',
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
        NotificationSettingsPanel: {
          template: '<section data-testid="notification-settings-stub"><h2 id="notification-settings-title">通知设置</h2></section>',
        },
      },
    },
  })
}

function mountShell() {
  return mount(AppShell, {
    global: {
      plugins: [pinia],
      stubs: {
        ChartWorkspacePage: {
          name: 'ChartWorkspacePage',
          props: { active: Boolean },
          template: '<div data-testid="chart-workspace-page" />',
        },
        GeneralSettingsPanel: {
          template: '<div data-testid="general-settings-stub" />',
        },
        AccountSettingsPage: {
          props: ['initialSection'],
          template: '<div data-testid="account-settings-stub" :data-initial-section="initialSection" />',
        },
        NotificationSettingsPanel: {
          template: '<section data-testid="notification-settings-stub"><h2 id="notification-settings-title">通知设置</h2></section>',
        },
      },
    },
  })
}

async function clickRail(wrapper: ReturnType<typeof mountShell>, label: string): Promise<void> {
  await wrapper.getComponent(NavigationRail).get(`button[aria-label="${label}"]`).trigger('click')
  await flushPromises()
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
      const paragraphs = placeholder.findAll('p')
      expect(paragraphs).toHaveLength(2)
      expect(paragraphs[0].text()).toBe(description)
      expect(paragraphs[1].text()).toBe('规划中')
      expect(placeholder.findAll('button, a[href], input, select, textarea, [role="switch"], [tabindex]'))
        .toHaveLength(0)
    },
  )

  it('compiles child navigation styles without a parent scope attribute', () => {
    const pageSource = readFileSync(
      resolve(process.cwd(), 'src/components/settings/SettingsCenterPage.vue'),
      'utf8',
    )
    const cssSource = readFileSync(
      resolve(process.cwd(), 'src/components/settings/SettingsCenterPage.css'),
      'utf8',
    )
    const { descriptor } = parse(pageSource)
    const style = descriptor.styles[0]
    const compiled = compileStyle({
      source: cssSource,
      filename: 'SettingsCenterPage.css',
      id: 'data-v-settings-center',
      scoped: style.scoped,
    })

    expect(style.src).toBe('./SettingsCenterPage.css')
    expect(style.scoped).not.toBe(true)
    expect(compiled.errors).toEqual([])
    expect(compiled.code).toMatch(/\.settings-sidebar-item\s*\{[^}]*width:\s*100%/)
    expect(compiled.code).toMatch(/\.settings-sidebar-item\.active\s*\{[^}]*background:\s*var\(--accent\)/)
    expect(compiled.code).not.toContain('[data-v-settings-center]')
  })

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

  it('mounts the real notification settings section instead of its former placeholder', () => {
    const wrapper = mountCenter({ initialSection: 'notifications' })

    expect(wrapper.get('[data-testid="notification-settings-stub"]').text()).toContain('通知设置')
    expect(wrapper.find('[data-testid="settings-placeholder"]').exists()).toBe(false)
  })

  it('emits back once from the header control', async () => {
    const wrapper = mountCenter()
    await wrapper.get('[data-testid="settings-back"]').trigger('click')

    expect(wrapper.emitted('back')).toHaveLength(1)
  })
})

describe('AppShell settings navigation', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
  })

  it('does not expose news navigation or a Dashboard news action', () => {
    const wrapper = mountShell()

    expect(wrapper.find('button[aria-label^="新闻"]').exists()).toBe(false)
    expect(wrapper.getComponent(DashboardQuickActions).text()).not.toContain('新闻中心')
  })

  it('uses the settings gear as primary navigation and removes the account rail item', async () => {
    const wrapper = mountShell()

    expect(wrapper.find('button[aria-label="账户"]').exists()).toBe(false)
    await clickRail(wrapper, '设置')

    expect(wrapper.getComponent(NavigationRail).props('active')).toBe('settings')
    expect(wrapper.findComponent(SettingsCenterPage).exists()).toBe(true)
    expect(wrapper.get('[data-testid="settings-nav-general"]').attributes('aria-current'))
      .toBe('page')
    expect(wrapper.get('[data-testid="general-settings-stub"]').exists()).toBe(true)
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)
  })

  it('returns from settings to the previous non-settings page', async () => {
    const wrapper = mountShell()

    await clickRail(wrapper, '交易')
    await clickRail(wrapper, '设置')
    await wrapper.get('[data-testid="settings-back"]').trigger('click')
    await flushPromises()

    expect(wrapper.getComponent(NavigationRail).props('active')).toBe('trading')
    expect(wrapper.findComponent(SettingsCenterPage).exists()).toBe(false)
    expect(wrapper.get('[data-testid="trading-layout"]').isVisible()).toBe(true)
  })

  it('leaves settings immediately when a primary page is selected', async () => {
    const wrapper = mountShell()

    await clickRail(wrapper, '设置')
    expect(wrapper.findComponent(SettingsCenterPage).exists()).toBe(true)
    await clickRail(wrapper, '插件')

    expect(wrapper.getComponent(NavigationRail).props('active')).toBe('plugins')
    expect(wrapper.findComponent(SettingsCenterPage).exists()).toBe(false)
    expect(wrapper.findComponent(Sidebar).exists()).toBe(true)
  })

  it('mounts the generic Sidebar only for Home and Plugins', async () => {
    const wrapper = mountShell()

    expect(wrapper.getComponent(NavigationRail).props('active')).toBe('home')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(true)

    await clickRail(wrapper, '交易')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)

    await clickRail(wrapper, '图表')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)

    await clickRail(wrapper, '插件')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(true)

    await clickRail(wrapper, '设置')
    expect(wrapper.findComponent(Sidebar).exists()).toBe(false)
  })

  it('keeps an active settings session but remounts General for a later gear entry', async () => {
    const wrapper = mountShell()

    await clickRail(wrapper, '设置')
    await wrapper.get('[data-testid="settings-nav-notifications"]').trigger('click')
    expect(wrapper.get('[data-testid="settings-nav-notifications"]').attributes('aria-current'))
      .toBe('page')

    await clickRail(wrapper, '设置')
    expect(wrapper.get('[data-testid="settings-nav-notifications"]').attributes('aria-current'))
      .toBe('page')

    await clickRail(wrapper, '首页')
    await clickRail(wrapper, '设置')
    expect(wrapper.get('[data-testid="settings-nav-general"]').attributes('aria-current'))
      .toBe('page')
    expect(wrapper.get('[data-testid="general-settings-stub"]').exists()).toBe(true)
  })

  it('opens Dashboard assets as an account/assets deep link without persisting it', async () => {
    const wrapper = mountShell()
    const quickActions = wrapper.getComponent(DashboardPage).getComponent(DashboardQuickActions)
    const assetsAction = quickActions.findAll('button')
      .find((button) => button.text().includes('查看资产'))

    expect(assetsAction).toBeDefined()
    await assetsAction!.trigger('click')
    await flushPromises()

    expect(wrapper.getComponent(NavigationRail).props('active')).toBe('settings')
    expect(wrapper.get('[data-testid="settings-nav-account"]').attributes('aria-current'))
      .toBe('page')
    expect(wrapper.get('[data-testid="account-settings-stub"]').attributes('data-initial-section'))
      .toBe('assets')

    await clickRail(wrapper, '首页')
    await clickRail(wrapper, '设置')
    expect(wrapper.get('[data-testid="settings-nav-general"]').attributes('aria-current'))
      .toBe('page')
    expect(wrapper.find('[data-testid="account-settings-stub"]').exists()).toBe(false)
  })
})
