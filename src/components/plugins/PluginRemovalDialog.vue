<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import type { PluginCatalogItem } from '../../types/plugin'
import { pluginManagementLabel } from './pluginPresentation'

interface FocusControl {
  readonly isConnected: boolean
  focus: () => void
}

interface DialogControl {
  readonly open: boolean
  showModal: () => void
  close: () => void
}

interface DialogActionControl extends FocusControl {
  readonly disabled: boolean
}

interface DialogKeyEvent {
  readonly key: string
  readonly shiftKey: boolean
  preventDefault: () => void
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

const dialog = ref<DialogControl | null>(null)
const cancelButton = ref<DialogActionControl | null>(null)
const confirmButton = ref<DialogActionControl | null>(null)
const actionRequested = ref(false)
const acceptedSubmission = ref(props.submitting)
let returnFocusTarget: FocusControl | null = null

function requestCancel(event?: { preventDefault: () => void }): void {
  event?.preventDefault()
  if (acceptedSubmission.value || props.submitting || actionRequested.value) return
  actionRequested.value = true
  emit('cancel')
}

function requestConfirm(): void {
  if (acceptedSubmission.value || props.submitting || props.stale || actionRequested.value) return
  actionRequested.value = true
  emit('confirm')
}

function trapFocus(event: DialogKeyEvent): void {
  if (event.key !== 'Tab') return
  const controls = [cancelButton.value, confirmButton.value].filter(
    (control): control is DialogActionControl => (
      control !== null && control.isConnected && !control.disabled
    ),
  )
  const first = controls[0]
  const last = controls[controls.length - 1]
  if (!first || !last) {
    event.preventDefault()
    return
  }
  const active = globalThis.document.activeElement
  if (event.shiftKey && active === first) {
    event.preventDefault()
    last.focus()
  } else if (!event.shiftKey && active === last) {
    event.preventDefault()
    first.focus()
  } else if (!controls.some((control) => control === active)) {
    event.preventDefault()
    ;(event.shiftKey ? last : first).focus()
  }
}

watch(() => props.submitting, (submitting) => {
  if (submitting) acceptedSubmission.value = true
})

watch(() => props.stale, (stale) => {
  if (stale && !acceptedSubmission.value) actionRequested.value = false
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
  if (!acceptedSubmission.value && dialog.value?.open) dialog.value.close()
})

onUnmounted(() => {
  if (!acceptedSubmission.value && returnFocusTarget?.isConnected) returnFocusTarget.focus()
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
          ref="confirmButton"
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
