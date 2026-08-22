export type AccountSettingsSection = 'api' | 'assets' | 'risk'

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

export type PrimaryPage = 'home' | 'trading' | 'charts' | 'plugins' | 'settings'
export type NavKey = PrimaryPage

export type SettingsNavigationTarget =
  | { page: 'settings'; settingsSection?: Exclude<SettingsSection, 'account'> }
  | { page: 'settings'; settingsSection: 'account'; accountSection?: AccountSettingsSection }

export type NavigationTarget =
  | { page: Exclude<PrimaryPage, 'settings'> }
  | SettingsNavigationTarget

export type NavigationRequest = PrimaryPage | NavigationTarget

export type HomeSection = 'welcome' | 'updates'
export type PluginSection = 'installed' | 'market' | 'manage'
export type SidebarSectionKey = HomeSection | PluginSection
export type SidebarTarget =
  | { page: 'home'; section: HomeSection }
  | { page: 'plugins'; section: PluginSection }
