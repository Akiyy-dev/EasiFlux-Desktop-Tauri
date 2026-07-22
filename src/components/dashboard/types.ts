import type { NavKey } from '../../types/navigation'

export type DashboardNavTarget = Extract<NavKey, 'trading' | 'account' | 'news' | 'plugins'>

export type DashboardActivityType = 'update' | 'announcement' | 'news'

export type DashboardActivityItem = {
  id: string
  type: DashboardActivityType
  title: string
  summary: string
  timeLabel: string
}

export type DashboardQuickAction = {
  key: DashboardNavTarget | 'positions' | 'assets'
  label: string
  description: string
}
