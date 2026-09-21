<script setup lang="ts">
import { computed } from 'vue'
import type { PluginCatalogItem } from '../../types/plugin'
import {
  pluginCapabilityLabels,
  pluginManagementLabel,
  pluginStatusLabel,
} from './pluginPresentation'

interface RemovalOpener {
  readonly isConnected: boolean
  focus: () => void
}

interface RemovalClickEvent {
  readonly currentTarget: unknown
}

const props = withDefaults(defineProps<{
  plugin: PluginCatalogItem
  pending: boolean
  error: string | null
  removalDisabled?: boolean
}>(), { removalDisabled: false })

const emit = defineEmits<{
  toggle: [id: string, enabled: boolean]
  remove: [id: string, opener: RemovalOpener]
}>()

const reasonLabels = {
  stateUnavailable: '插件状态暂时不可用，请稍后重试。',
  catalogInvalid: '插件目录校验失败，暂时无法启用。',
} as const

const statusLabel = computed(() => pluginStatusLabel(props.plugin.status))
const hasComputeCommand = computed(() => (
  props.plugin.manifest.schemaVersion >= 4
  && props.plugin.manifest.contributions.some(
    (command) => command.actionId === 'sandbox.computeSeries',
  )
))
const hasWorkflowCommand = computed(() => (
  props.plugin.manifest.schemaVersion === 5
  && props.plugin.manifest.contributions.some(
    (command) => command.actionId === 'sandbox.accountWorkflow',
  )
))
const requestedCapabilityLabel = computed(() => (
  props.plugin.manifest.schemaVersion === 5
    ? props.plugin.manifest.requestedCapabilities.map(
      (capability) => pluginCapabilityLabels[capability],
    ).join('、')
    : '无需额外权限'
))
const statusId = computed(() => `plugin-card-${props.plugin.manifest.id}-status`)
const errorId = computed(() => `plugin-card-${props.plugin.manifest.id}-error`)
const describedBy = computed(() => (
  props.error ? `${statusId.value} ${errorId.value}` : statusId.value
))
const disabled = computed(() => (
  props.pending
  || !props.plugin.canToggle
  || props.plugin.status === 'blocked'
  || props.plugin.toggleBlockReasonCode === 'removalPending'
))
const reasonLabel = computed(() => {
  if (props.plugin.toggleBlockReasonCode === 'removalPending') {
    return pluginManagementLabel('removalPending')
  }
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

function isRemovalOpener(value: unknown): value is RemovalOpener {
  if (typeof value !== 'object' || value === null) return false
  const candidate = value as { isConnected?: unknown; focus?: unknown }
  return candidate.isConnected === true && typeof candidate.focus === 'function'
}

function requestRemoval(event: RemovalClickEvent): void {
  const opener = event.currentTarget
  if (
    props.removalDisabled
    || props.plugin.management !== 'managed'
    || props.plugin.status !== 'disabled'
    || props.plugin.canRemove !== true
    || !isRemovalOpener(opener)
  ) return
  emit('remove', props.plugin.manifest.id, opener)
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
          @click.prevent="requestToggle"
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
        <dt>来源与管理</dt>
        <dd>{{ pluginManagementLabel(plugin.management) }}</dd>
      </div>
    </dl>

    <p v-if="plugin.source === 'localDeclarative'" class="plugin-card__local-note">
      {{ hasWorkflowCommand
        ? '只有明确打开账户工作流并在会话内选择授权后，插件才能读取对应数据或准备提案；每次真实交易仍需单独确认'
        : hasComputeCommand
          ? '只有明确点击运行才会在 WebAssembly 沙箱中执行本地计算；输入和结果仅保存在内存中'
          : plugin.manifest.schemaVersion !== 1
            ? plugin.manifest.schemaVersion >= 3
              ? '启用后可显示信息或请求宿主打开白名单页面，不会运行插件代码'
              : '启用后提供只读命令，不会运行插件代码'
            : '启用仅记录宿主偏好，不会运行插件代码' }}
    </p>

    <slot name="commands" />

    <div class="plugin-card__permissions">
      <p data-testid="requested-capabilities">
        <strong>请求权限：</strong>{{ requestedCapabilityLabel }}
      </p>
      <p data-testid="granted-capabilities">
        <strong>已授予权限：</strong>{{ hasWorkflowCommand
          ? '目录不显示会话授权；启用不会授权，请在账户工作流中查看和选择'
          : '无需额外权限' }}
      </p>
    </div>

    <p
      :id="statusId"
      class="plugin-card__status"
      data-testid="plugin-status"
    >
      状态：{{ statusLabel }}<span v-if="reasonLabel">；{{ reasonLabel }}</span>
    </p>
    <div
      v-if="plugin.management === 'managed'"
      class="plugin-card__removal"
      data-testid="plugin-removal-area"
    >
      <p v-if="plugin.status === 'enabled'">
        如需移除，请先停用此插件。
      </p>
      <button
        v-else-if="plugin.status === 'disabled' && plugin.canRemove"
        class="ef-btn ef-btn-danger ef-btn-sm"
        data-testid="plugin-remove-button"
        type="button"
        :disabled="props.removalDisabled"
        @click="requestRemoval"
      >
        移除本地包
      </button>
      <p v-else>
        此受管包当前不可移除。
      </p>
    </div>
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
