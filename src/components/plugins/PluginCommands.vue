<script setup lang="ts">
import { computed } from 'vue'
import { usePluginCommandResult } from '../../composables/usePluginCommandResult'
import { usePluginStore } from '../../stores/plugin'
import type { PluginCatalogItem, PluginCommandSummary } from '../../types/plugin'
import { pluginPageLabel } from '../../services/pluginNavigation'
import PluginComputeDialog from './PluginComputeDialog.vue'
import PluginCommandResult from './PluginCommandResult.vue'

const props = withDefaults(defineProps<{
  plugin: PluginCatalogItem
  navigationAvailable?: boolean
}>(), { navigationAvailable: false })
const emit = defineEmits<{
  'open-page': [intent: { pluginId: string; contributionId: string }]
}>()
const store = usePluginStore()
const { result, computeIntent, run, clear } = usePluginCommandResult(
  () => props.plugin,
  (intent) => emit('open-page', intent),
)
const commands = computed(() => store.availableCommands.filter(
  (command) => command.pluginId === props.plugin.manifest.id,
))

function commandLabel(command: PluginCommandSummary): string {
  if (command.actionId === 'host.showInfo') return `显示信息：${command.title}`
  if (command.actionId === 'host.openPage') {
    return `打开页面：${pluginPageLabel(command.destination)}`
  }
  return `运行计算：${command.title}`
}
</script>

<template>
  <section v-if="commands.length" class="plugin-commands" aria-label="插件命令">
    <div class="plugin-commands__actions">
      <button
        v-for="command in commands"
        :key="command.contributionId"
        class="ef-btn ef-btn-secondary ef-btn-sm"
        data-testid="plugin-command-button"
        type="button"
        :disabled="command.actionId === 'host.openPage' && !props.navigationAvailable"
        @click="run(command.pluginId, command.contributionId, props.navigationAvailable)"
      >
        {{ commandLabel(command) }}
      </button>
    </div>
    <p
      v-if="!props.navigationAvailable && commands.some((command) => command.actionId === 'host.openPage')"
      class="plugin-commands__navigation-unavailable"
    >
      当前宿主不提供页面导航；信息显示与本地计算命令仍可使用。
    </p>
    <PluginCommandResult
      v-if="result"
      :result="result"
      @close="clear"
    />
    <PluginComputeDialog
      v-if="computeIntent"
      :key="`${computeIntent.expectedCatalogGeneration}:${computeIntent.expectedRevision}:${computeIntent.pluginId}:${computeIntent.contributionId}`"
      :intent="computeIntent"
      @close="clear"
    />
  </section>
</template>
