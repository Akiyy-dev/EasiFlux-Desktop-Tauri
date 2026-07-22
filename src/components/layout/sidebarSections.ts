import type { NavKey, SidebarSectionKey } from '../../types/navigation'

export interface SidebarSection {
  title: string
  items: Array<{ key: SidebarSectionKey; label: string }>
}

export const sectionsByNav: Partial<Record<NavKey, SidebarSection[]>> = {
  home: [{
    title: 'EasiFlux',
    items: [
      { key: 'welcome', label: '欢迎页' },
      { key: 'updates', label: '最近更新' },
    ],
  }],
  plugins: [{
    title: '插件',
    items: [
      { key: 'installed', label: '已安装插件' },
      { key: 'market', label: '插件市场' },
      { key: 'manage', label: '插件管理' },
    ],
  }],
  account: [{
    title: '账户',
    items: [
      { key: 'api', label: 'API 管理' },
      { key: 'assets', label: '资产总览' },
      { key: 'risk', label: '风险控制' },
    ],
  }],
}
