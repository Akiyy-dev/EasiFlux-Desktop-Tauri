import type { PluginSource } from '../../types/plugin'

export const pluginSourceLabels: Record<PluginSource, string> = {
  builtIn: '内置 · 随应用提供',
  localDeclarative: '本地声明式包 · 已发现，未执行',
}

export function pluginSourceLabel(source: PluginSource): string {
  return pluginSourceLabels[source]
}
