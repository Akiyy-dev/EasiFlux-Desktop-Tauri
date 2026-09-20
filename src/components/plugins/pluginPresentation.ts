import type { PluginManagement, PluginSource, PluginStatus } from '../../types/plugin'
import type { PluginWorkflowCapability } from '../../types/pluginWorkflow'

export const pluginSourceLabels: Record<PluginSource, string> = {
  builtIn: '内置 · 随应用提供',
  localDeclarative: '本地插件包 · 已发现',
}

export function pluginSourceLabel(source: PluginSource): string {
  return pluginSourceLabels[source]
}

export const pluginManagementLabels: Record<PluginManagement, string> = {
  builtIn: '内置 · 随应用提供',
  managed: '本地插件包 · EasiFlux 管理',
  external: '本地插件包 · 外部放置，应用不会删除',
  ownershipConflict: '管理记录不一致，需要退出应用后人工检查',
  ownershipUnavailable: '所有权状态暂不可用，应用不会删除',
  removalPending: '移除准备待回退，当前不可启停或移除',
}

export function pluginManagementLabel(management: PluginManagement): string {
  return pluginManagementLabels[management]
}

export const pluginStatusLabels: Record<PluginStatus, string> = {
  enabled: '已启用',
  disabled: '已停用',
  blocked: '已阻止',
}

export function pluginStatusLabel(status: PluginStatus): string {
  return pluginStatusLabels[status]
}

export const pluginCapabilityLabels: Record<PluginWorkflowCapability, string> = {
  'account.read': '账户会话',
  'balances.read': '余额',
  'positions.read': '持仓',
  'orders.read': '未成交委托',
  'market.read': '市场报价',
  'trade.place': '真实下单提案',
  'trade.cancel': '真实撤单提案',
}
