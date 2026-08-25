import type { SettingsSection } from '../../types/navigation'

export interface SettingsSectionDefinition {
  readonly key: SettingsSection
  readonly label: string
  readonly description: string
}

export interface SettingsSectionGroup {
  readonly key: 'basic' | 'features' | 'personalization' | 'system'
  readonly label: string
  readonly items: readonly SettingsSectionDefinition[]
}

function section(
  key: SettingsSection,
  label: string,
  description: string,
): SettingsSectionDefinition {
  return Object.freeze({ key, label, description })
}

const general = section('general', '通用', '配置应用的基础偏好。')
const account = section('account', '账户', '管理账户与账户级设置。')
const plugins = section('plugins', '插件', '管理插件的启用状态、权限与插件级配置。')
const notifications = section('notifications', '通知', '配置实时应用内 Toast 偏好与当前账户通知清理。')
const searchCommands = section('searchCommands', '搜索与命令', '配置全局搜索与命令面板的行为。')
const hotkeys = section('hotkeys', '快捷键', '查看并管理应用快捷键。')
const workspace = section('workspace', '工作区与窗口', '配置窗口布局、工作区保存与恢复行为。')
const appearance = section('appearance', '外观', '配置主题、颜色与界面显示方式。')
const languageRegion = section('languageRegion', '语言与地区', '配置语言、地区与时间格式。')
const about = section('about', '关于', '查看 EasiFlux 应用信息。')

export const SETTINGS_SECTION_GROUPS: readonly SettingsSectionGroup[] = Object.freeze([
  Object.freeze({
    key: 'basic' as const,
    label: '基础',
    items: Object.freeze([general, account]),
  }),
  Object.freeze({
    key: 'features' as const,
    label: '功能',
    items: Object.freeze([plugins, notifications, searchCommands, hotkeys]),
  }),
  Object.freeze({
    key: 'personalization' as const,
    label: '个性化',
    items: Object.freeze([workspace, appearance, languageRegion]),
  }),
  Object.freeze({
    key: 'system' as const,
    label: '系统',
    items: Object.freeze([about]),
  }),
])

export const SETTINGS_SECTION_BY_KEY: Readonly<Record<SettingsSection, SettingsSectionDefinition>>
  = Object.freeze({
    general,
    account,
    plugins,
    notifications,
    searchCommands,
    hotkeys,
    workspace,
    appearance,
    languageRegion,
    about,
  })
