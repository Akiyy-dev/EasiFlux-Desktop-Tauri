<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { usePluginStore } from '../../stores/plugin'
import type { PluginCatalogItem, PluginCommandInfo } from '../../types/plugin'

const props = defineProps<{ plugin: PluginCatalogItem }>()
const store = usePluginStore()
const result = ref<PluginCommandInfo | null>(null)
const commands = computed(() => {
  if (!store.commandsAvailable || props.plugin.manifest.schemaVersion !== 2) return []
  return props.plugin.manifest.contributions.filter((command) => (
    store.runCommand(props.plugin.manifest.id, command.contributionId) !== null
  ))
})

// Flush synchronously: even a pending transition that begins and finishes in
// one render cycle permanently revokes the old result. Re-enable never replays it.
watch(
  [() => store.commandContextKey, () => props.plugin],
  () => { result.value = null },
  { flush: 'sync', deep: true },
)

function run(contributionId: string): void {
  result.value = store.runCommand(props.plugin.manifest.id, contributionId)
}
</script>

<template>
  <section v-if="commands.length" class="plugin-commands" aria-label="插件只读命令">
    <div class="plugin-commands__actions">
      <button
        v-for="command in commands"
        :key="command.contributionId"
        class="ef-btn ef-btn-secondary ef-btn-sm"
        data-testid="plugin-command-button"
        type="button"
        @click="run(command.contributionId)"
      >
        {{ command.title }}
      </button>
    </div>
    <div
      v-if="result"
      class="plugin-commands__result"
      data-testid="plugin-command-result"
      role="status"
      aria-live="polite"
    >
      <p class="plugin-commands__attribution">
        {{ result.pluginName }}（{{ result.pluginId }}）· 插件提供的信息，未经认证
      </p>
      <h3>{{ result.title }}</h3>
      <p class="plugin-commands__text">
        {{ result.text }}
      </p>
      <button class="ef-btn ef-btn-secondary ef-btn-sm" type="button" @click="result = null">
        关闭信息
      </button>
    </div>
  </section>
</template>
