import type { PluginManagement, PluginSource } from '../../types/plugin'

export const pluginSourceLabels: Record<PluginSource, string> = {
  builtIn: '内置 · 随应用提供',
  localDeclarative: '本地声明式包 · 已发现，未执行',
}

export function pluginSourceLabel(source: PluginSource): string {
  return pluginSourceLabels[source]
}

export const pluginManagementLabels: Record<PluginManagement, string> = {
  builtIn: '内置 · 随应用提供',
  managed: '本地声明式包 · EasiFlux 管理',
  external: '本地声明式包 · 外部放置，应用不会删除',
  ownershipConflict: '管理记录不一致，需要退出应用后人工检查',
  ownershipUnavailable: '所有权状态暂不可用，应用不会删除',
  removalPending: '移除准备待回退，当前不可启停或移除',
}

export function pluginManagementLabel(management: PluginManagement): string {
  return pluginManagementLabels[management]
}
