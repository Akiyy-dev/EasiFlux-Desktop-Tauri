<script setup lang="ts">
import { computed } from 'vue'
import { usePluginCommandResult } from '../../composables/usePluginCommandResult'
import { usePluginStore } from '../../stores/plugin'
import type { PluginCatalogItem } from '../../types/plugin'
import PluginCommandResult from './PluginCommandResult.vue'

const props = defineProps<{ plugin: PluginCatalogItem }>()
const store = usePluginStore()
const { result, run, clear } = usePluginCommandResult(() => props.plugin)
const commands = computed(() => store.availableCommands.filter(
  (command) => command.pluginId === props.plugin.manifest.id,
))
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
        @click="run(command.pluginId, command.contributionId)"
      >
        {{ command.title }}
      </button>
    </div>
    <PluginCommandResult
      v-if="result"
      :result="result"
      @close="clear"
    />
  </section>
</template>
