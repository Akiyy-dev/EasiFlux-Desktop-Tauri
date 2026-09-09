<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import type { PluginCatalogItem } from '../../types/plugin'
import { pluginManagementLabel } from './pluginPresentation'

interface FocusControl {
  readonly isConnected: boolean
  focus: () => void
}

const props = defineProps<{
  plugin: PluginCatalogItem
  submitting: boolean
  stale: boolean
  opener?: FocusControl | null
}>()

const emit = defineEmits<{
  confirm: []
  cancel: []
}>()

const dialog = ref<HTMLDialogElement | null>(null)
const cancelButton = ref<HTMLButtonElement | null>(null)
const actionRequested = ref(false)
let returnFocusTarget: FocusControl | null = null
let submitted = false

function requestCancel(event?: { preventDefault: () => void }): void {
  event?.preventDefault()
  if (props.submitting || actionRequested.value) return
  actionRequested.value = true
  emit('cancel')
}

function requestConfirm(): void {
  if (props.submitting || props.stale || actionRequested.value) return
  actionRequested.value = true
  submitted = true
  emit('confirm')
}

function trapFocus(event: KeyboardEvent): void {
  if (event.key !== 'Tab') return
  const controls = [...(dialog.value?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])]
    .filter((control) => control.isConnected)
  if (controls.length === 0) {
    event.preventDefault()
    return
  }
  const first = controls[0]
  const last = controls[controls.length - 1]
  if (event.shiftKey && document.activeElement === first) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && document.activeElement === last) {
    event.preventDefault()
    first.focus()
  } else if (!controls.includes(document.activeElement as HTMLButtonElement)) {
    event.preventDefault()
    ;(event.shiftKey ? last : first).focus()
  }
}

watch(() => props.stale, (stale) => {
  if (stale && !props.submitting) {
    actionRequested.value = false
    submitted = false
  }
})

onMounted(async () => {
  const active = globalThis.document.activeElement
  returnFocusTarget = props.opener?.isConnected
    ? props.opener
    : active instanceof globalThis.HTMLElement ? active : null
  dialog.value?.showModal()
  await nextTick()
  cancelButton.value?.focus()
})

onBeforeUnmount(() => {
  if (dialog.value?.open) dialog.value.close()
})

onUnmounted(() => {
  if (!submitted && returnFocusTarget?.isConnected) returnFocusTarget.focus()
})
</script>

<template>
  <dialog
    ref="dialog"
    class="plugin-removal-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="plugin-removal-title"
    aria-describedby="plugin-removal-warning"
    :aria-busy="props.submitting"
    @cancel="requestCancel"
    @keydown="trapFocus"
  >
    <div class="plugin-removal-dialog__surface">
      <header>
        <p class="plugin-removal-dialog__eyebrow">
          受管本地声明式包
        </p>
        <h2 id="plugin-removal-title">
          确认移除此本地包
        </h2>
      </header>

      <dl class="plugin-removal-dialog__metadata">
        <div><dt>名称</dt><dd><bdi>{{ props.plugin.manifest.name }}</bdi></dd></div>
        <div><dt>ID</dt><dd><bdi>{{ props.plugin.manifest.id }}</bdi></dd></div>
        <div><dt>版本</dt><dd><bdi>{{ props.plugin.manifest.version }}</bdi></dd></div>
        <div><dt>发布者</dt><dd><bdi>{{ props.plugin.manifest.publisher }}</bdi></dd></div>
        <div><dt>来源与管理</dt><dd><bdi>{{ pluginManagementLabel(props.plugin.management) }}</bdi></dd></div>
      </dl>

      <p id="plugin-removal-warning" class="plugin-removal-dialog__warning">
        EasiFlux 管理的本地副本将被移除；最初选择的源文件不会被修改。此操作无法撤销，当前的停用偏好将保留。
      </p>
      <p
        v-if="props.stale"
        class="plugin-removal-dialog__stale"
        data-testid="plugin-removal-stale"
        role="alert"
      >
        插件目录或状态已更新，此确认已失效。请取消并重新开始。
      </p>
      <p
        v-if="props.submitting"
        class="plugin-removal-dialog__progress"
        data-testid="plugin-removal-progress"
        role="status"
        aria-live="polite"
      >
        正在移除本地包…
      </p>

      <footer class="plugin-removal-dialog__actions">
        <button
          ref="cancelButton"
          class="ef-btn ef-btn-secondary"
          data-testid="plugin-removal-cancel"
          type="button"
          :disabled="props.submitting"
          @click="requestCancel"
        >
          取消
        </button>
        <button
          class="ef-btn ef-btn-danger"
          data-testid="plugin-removal-confirm"
          type="button"
          :disabled="props.submitting || props.stale"
          @click="requestConfirm"
        >
          确认移除
        </button>
      </footer>
    </div>
  </dialog>
</template>
