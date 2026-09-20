<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { usePluginCommandResult } from '../../composables/usePluginCommandResult'
import { usePluginStore } from '../../stores/plugin'
import { pluginPageLabel } from '../../services/pluginNavigation'
import type { PluginCommandSummary } from '../../types/plugin'
import PluginComputeDialog from './PluginComputeDialog.vue'
import PluginCommandResult from './PluginCommandResult.vue'

const props = withDefaults(defineProps<{ navigationAvailable?: boolean }>(), {
  navigationAvailable: false,
})
const emit = defineEmits<{
  'open-page': [intent: { pluginId: string; contributionId: string }]
}>()
const store = usePluginStore()
const query = ref('')
const { result, computeIntent, run, clear } = usePluginCommandResult(
  undefined,
  (intent) => emit('open-page', intent),
)

const matches = computed(() => {
  const needle = query.value.trim().toLowerCase()
  return store.availableCommands.filter((command) => (
    [command.title, command.contributionId, command.pluginName, command.pluginId]
      .some((value) => value.toLowerCase().includes(needle))
  ))
})

watch(query, clear, { flush: 'sync' })

function commandActionLabel(command: PluginCommandSummary): string {
  if (command.actionId === 'host.showInfo') return '显示信息'
  if (command.actionId === 'host.openPage') {
    return `打开页面：${pluginPageLabel(command.destination)}`
  }
  return '运行计算'
}
</script>

<template>
  <section
    class="plugin-command-workbench"
    data-testid="plugin-command-workbench"
    aria-labelledby="plugin-command-workbench-title"
  >
    <header class="plugin-command-workbench__header">
      <div>
        <h2 id="plugin-command-workbench-title">
          命令工作台
        </h2>
        <p>集中查找当前已启用插件提供的信息、导航与本地计算命令；只有明确点击后才会执行。</p>
      </div>
      <p
        class="plugin-command-workbench__count"
        data-testid="plugin-command-count"
        role="status"
        aria-live="polite"
      >
        {{ matches.length }} 个匹配 / {{ store.availableCommands.length }} 个可用
      </p>
    </header>

    <label class="plugin-command-workbench__search" for="plugin-command-search">
      <span>搜索已启用命令</span>
      <input
        id="plugin-command-search"
        v-model="query"
        data-testid="plugin-command-search"
        type="search"
        placeholder="命令名称、贡献 ID、插件名称或插件 ID"
      >
    </label>

    <p
      v-if="!store.commandsAvailable"
      class="plugin-marketplace-page__empty"
      data-testid="plugin-command-unavailable"
    >
      当前状态尚未确认，暂不可使用命令。请等待当前操作完成或重新扫描确认状态。
    </p>
    <p
      v-else-if="store.availableCommands.length === 0"
      class="plugin-marketplace-page__empty"
      data-testid="plugin-command-empty"
    >
      当前没有已启用的插件命令。启用受支持的本地插件后，可在此处明确点击使用。
    </p>
    <p
      v-else-if="matches.length === 0"
      class="plugin-marketplace-page__empty"
      data-testid="plugin-command-no-match"
    >
      没有匹配命令，请调整工作台搜索内容。
    </p>
    <ul v-else class="plugin-command-workbench__list">
      <li
        v-for="command in matches"
        :key="`${command.pluginId}:${command.contributionId}`"
        class="plugin-command-workbench__item ef-card"
      >
        <div class="plugin-command-workbench__identity">
          <h3>{{ command.title }}</h3>
          <p>{{ command.pluginName }}</p>
          <dl>
            <div>
              <dt>插件 ID</dt>
              <dd>{{ command.pluginId }}</dd>
            </div>
            <div>
              <dt>贡献 ID</dt>
              <dd>{{ command.contributionId }}</dd>
            </div>
          </dl>
        </div>
        <button
          class="ef-btn ef-btn-secondary ef-btn-sm"
          data-testid="plugin-workbench-command"
          type="button"
          :disabled="command.actionId === 'host.openPage' && !props.navigationAvailable"
          :aria-label="`${commandActionLabel(command)}，命令 ${command.title}，插件 ${command.pluginName}（${command.pluginId}），贡献 ${command.contributionId}`"
          @click="run(command.pluginId, command.contributionId, props.navigationAvailable)"
        >
          {{ commandActionLabel(command) }}
        </button>
      </li>
    </ul>

    <p
      v-if="!props.navigationAvailable && store.availableCommands.some((command) => command.actionId === 'host.openPage')"
      class="plugin-marketplace-page__empty"
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
