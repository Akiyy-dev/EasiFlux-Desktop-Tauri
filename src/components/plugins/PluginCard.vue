<script setup lang="ts">
import { computed } from 'vue'
import type { PluginCatalogItem } from '../../types/plugin'

const props = defineProps<{
  plugin: PluginCatalogItem
  pending: boolean
  error: string | null
}>()

const emit = defineEmits<{
  toggle: [id: string, enabled: boolean]
}>()

const statusLabels = {
  enabled: '已启用',
  disabled: '已停用',
  blocked: '已阻止',
} as const

const reasonLabels = {
  stateUnavailable: '插件状态暂时不可用，请稍后重试。',
  catalogInvalid: '插件目录校验失败，暂时无法启用。',
} as const

const statusLabel = computed(() => statusLabels[props.plugin.status])
const statusId = computed(() => `plugin-card-${props.plugin.manifest.id}-status`)
const errorId = computed(() => `plugin-card-${props.plugin.manifest.id}-error`)
const describedBy = computed(() => (
  props.error ? `${statusId.value} ${errorId.value}` : statusId.value
))
const disabled = computed(() => (
  props.pending || !props.plugin.canToggle || props.plugin.status === 'blocked'
))
const reasonLabel = computed(() => {
  if (props.plugin.statusReasonCode) {
    return reasonLabels[props.plugin.statusReasonCode]
  }
  if (!props.plugin.canToggle) return '此插件当前不可切换。'
  if (props.pending) return '正在保存启用状态。'
  return null
})

function requestToggle(): void {
  if (disabled.value) return
  emit(
    'toggle',
    props.plugin.manifest.id,
    props.plugin.status !== 'enabled',
  )
}
</script>

<template>
  <article
    class="plugin-card ef-card"
    :aria-busy="pending || undefined"
  >
    <header class="plugin-card__header">
      <div class="plugin-card__identity">
        <h2>{{ plugin.manifest.name }}</h2>
        <span class="plugin-card__version">v{{ plugin.manifest.version }}</span>
      </div>
      <label class="plugin-card__toggle">
        <span>{{ plugin.canToggle ? '启用插件' : '无法切换' }}</span>
        <input
          type="checkbox"
          role="switch"
          :checked="plugin.status === 'enabled'"
          :disabled="disabled"
          :aria-label="`${plugin.manifest.name}，当前${statusLabel}`"
          :aria-describedby="describedBy"
          @change="requestToggle"
        >
      </label>
    </header>

    <p class="plugin-card__description">
      {{ plugin.manifest.description }}
    </p>

    <dl class="plugin-card__metadata">
      <div>
        <dt>插件 ID</dt>
        <dd>{{ plugin.manifest.id }}</dd>
      </div>
      <div>
        <dt>发布者</dt>
        <dd>{{ plugin.manifest.publisher }}（{{ plugin.manifest.publisherId }}）</dd>
      </div>
      <div>
        <dt>来源</dt>
        <dd>内置 · 随应用提供</dd>
      </div>
    </dl>

    <div class="plugin-card__permissions">
      <p data-testid="requested-capabilities">
        <strong>请求权限：</strong>无需额外权限
      </p>
      <p data-testid="granted-capabilities">
        <strong>已授予权限：</strong>无需额外权限
      </p>
    </div>

    <p
      :id="statusId"
      class="plugin-card__status"
      data-testid="plugin-status"
    >
      状态：{{ statusLabel }}<span v-if="reasonLabel">；{{ reasonLabel }}</span>
    </p>
    <p
      v-if="error"
      :id="errorId"
      class="plugin-card__error"
      role="alert"
    >
      {{ error }}
    </p>
  </article>
</template>
