import type { NavigationTarget } from '../types/navigation'
import type { PluginPageDestination } from '../types/plugin'

const destinationLabels: Record<PluginPageDestination, string> = {
  home: '首页',
  trading: '交易页',
  charts: '图表工作区',
  'settings.general': '通用设置',
  'settings.notifications': '通知设置',
  'settings.about': '关于',
}

export function pluginPageLabel(destination: PluginPageDestination): string {
  return destinationLabels[destination]
}

export function pluginNavigationTarget(destination: unknown): NavigationTarget | null {
  switch (destination) {
    case 'home': return { page: 'home' }
    case 'trading': return { page: 'trading' }
    case 'charts': return { page: 'charts' }
    case 'settings.general': return { page: 'settings', settingsSection: 'general' }
    case 'settings.notifications': return { page: 'settings', settingsSection: 'notifications' }
    case 'settings.about': return { page: 'settings', settingsSection: 'about' }
    default: return null
  }
}
