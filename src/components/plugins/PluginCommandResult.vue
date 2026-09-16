<script setup lang="ts">
import type { PluginCommandInfo } from '../../types/plugin'

defineProps<{ result: PluginCommandInfo }>()
const emit = defineEmits<{ close: [] }>()
</script>

<template>
  <div
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
    <button class="ef-btn ef-btn-secondary ef-btn-sm" type="button" @click="emit('close')">
      关闭信息
    </button>
  </div>
</template>
