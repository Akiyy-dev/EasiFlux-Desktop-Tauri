import type { NavKey } from '../../types/navigation'

export type DashboardNavTarget = Extract<NavKey, 'trading' | 'plugins'>

export type DashboardActivityType = 'update' | 'announcement'

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
