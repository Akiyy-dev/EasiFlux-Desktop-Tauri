import type { SidebarSectionKey } from '../../types/navigation'

export interface SidebarSection {
  title: string
  items: Array<{ key: SidebarSectionKey; label: string }>
}

export const sectionsByNav: Partial<Record<'home' | 'plugins', SidebarSection[]>> = {
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
}
