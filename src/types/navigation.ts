export type AccountSettingsSection = 'api' | 'assets' | 'risk'
export type AccountSection = AccountSettingsSection

export type SettingsSection =
  | 'general'
  | 'account'
  | 'plugins'
  | 'notifications'
  | 'searchCommands'
  | 'hotkeys'
  | 'workspace'
  | 'appearance'
  | 'languageRegion'
  | 'about'

export type NavKey =
  | 'home'
  | 'trading'
  | 'charts'
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
