export type AccountSection = 'api' | 'assets' | 'risk'

export type NavKey =
  | 'home'
  | 'trading'
  | 'charts'
  | 'news'
  | 'account'
  | 'plugins'
  | 'settings'

export interface NavigationTarget {
  page: NavKey
  section?: AccountSection
}

export type NonAccountSection = 'welcome' | 'updates' | 'installed' | 'market' | 'manage'
export type SidebarSectionKey = AccountSection | NonAccountSection

export type SidebarTarget =
  | { page: 'account'; section: AccountSection }
  | { page: Exclude<NavKey, 'account'>; section: NonAccountSection }
