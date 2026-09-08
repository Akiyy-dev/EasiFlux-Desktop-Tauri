<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import type { ReadyLocalManifestImport } from '../../types/plugin'

const props = defineProps<{
  preview: ReadyLocalManifestImport
  committing: boolean
  stale: boolean
}>()

const emit = defineEmits<{
  confirm: []
  cancel: []
}>()

interface DialogControl {
  readonly open: boolean
  showModal: () => void
  close: () => void
}

interface FocusControl {
  readonly isConnected: boolean
  focus: () => void
}

const dialog = ref<DialogControl | null>(null)
const cancelButton = ref<FocusControl | null>(null)
const actionRequested = ref(false)
let opener: FocusControl | null = null

function requestCancel(event?: { preventDefault: () => void }): void {
  event?.preventDefault()
  if (props.committing || actionRequested.value) return
  actionRequested.value = true
  emit('cancel')
}

function requestConfirm(): void {
  if (props.committing || props.stale || actionRequested.value) return
  actionRequested.value = true
  emit('confirm')
}

watch(() => props.preview.token, () => {
  actionRequested.value = false
})

onMounted(async () => {
  const active = globalThis.document.activeElement
  opener = active instanceof globalThis.HTMLElement ? active : null
  dialog.value?.showModal()
  await nextTick()
  cancelButton.value?.focus()
})

onBeforeUnmount(() => {
  if (dialog.value?.open) dialog.value.close()
})

onUnmounted(() => {
  if (opener?.isConnected) opener.focus()
})
</script>

<template>
  <dialog
    ref="dialog"
    class="plugin-import-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="plugin-import-title"
    aria-describedby="plugin-import-warning"
    :aria-busy="props.committing"
    @cancel="requestCancel"
  >
    <div class="plugin-import-dialog__surface">
      <header>
        <p class="plugin-import-dialog__eyebrow">
          本地声明式清单
        </p>
        <h2 id="plugin-import-title">
          确认导入此清单
        </h2>
      </header>

      <dl class="plugin-import-dialog__metadata">
        <div>
          <dt>名称</dt>
          <dd><bdi>{{ props.preview.manifest.name }}</bdi></dd>
        </div>
        <div>
          <dt>ID</dt>
          <dd><bdi>{{ props.preview.manifest.id }}</bdi></dd>
        </div>
        <div>
          <dt>版本</dt>
          <dd><bdi>{{ props.preview.manifest.version }}</bdi></dd>
        </div>
        <div>
          <dt>描述</dt>
          <dd><bdi>{{ props.preview.manifest.description }}</bdi></dd>
        </div>
        <div>
          <dt>发布者</dt>
          <dd><bdi>{{ props.preview.manifest.publisher }}</bdi></dd>
        </div>
        <div>
          <dt>发布者 ID</dt>
          <dd><bdi>{{ props.preview.manifest.publisherId }}</bdi></dd>
        </div>
      </dl>

      <p id="plugin-import-warning" class="plugin-import-dialog__warning">
        发布者信息由清单作者填写，未经认证。本次只复制元数据，不运行代码或授予权限。导入后默认停用，启用仅记录宿主偏好。
      </p>
      <p class="plugin-import-dialog__hint">
        此预览为一次性确认，预览将在五分钟后失效；到期后请重新选择清单。
      </p>
      <p
        v-if="props.stale"
        class="plugin-import-dialog__stale"
        data-testid="plugin-import-stale"
        role="alert"
      >
        插件目录已更新，此预览已失效。请取消并重新选择清单。
      </p>
      <p
        v-if="props.committing"
        class="plugin-import-dialog__progress"
        role="status"
        aria-live="polite"
      >
        正在导入清单…
      </p>

      <footer class="plugin-import-dialog__actions">
        <button
          ref="cancelButton"
          class="ef-btn ef-btn-secondary"
          data-testid="plugin-import-cancel"
          type="button"
          :disabled="props.committing"
          @click="requestCancel"
        >
          取消
        </button>
        <button
          class="ef-btn ef-btn-primary"
          data-testid="plugin-import-confirm"
          type="button"
          :disabled="props.committing || props.stale"
          @click="requestConfirm"
        >
          确认导入
        </button>
      </footer>
    </div>
  </dialog>
</template>
