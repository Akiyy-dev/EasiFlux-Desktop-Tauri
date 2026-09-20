<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, onUnmounted, ref, watch } from 'vue'
import type { PluginCommandContribution, ReadyLocalManifestImport } from '../../types/plugin'
import { pluginComputeModuleByteLength } from '../../services/pluginManifestDiff'
import { pluginPageLabel } from '../../services/pluginNavigation'
import PluginManifestComparison from './PluginManifestComparison.vue'

interface DialogControl {
  readonly open: boolean
  showModal: () => void
  close: () => void
}

interface FocusControl {
  readonly isConnected: boolean
  focus: () => void
}

const props = defineProps<{
  preview: ReadyLocalManifestImport
  committing: boolean
  stale: boolean
  opener?: FocusControl | null
}>()

const emit = defineEmits<{
  confirm: []
  cancel: []
}>()

const dialog = ref<DialogControl | null>(null)
const cancelButton = ref<FocusControl | null>(null)
const actionRequested = ref(false)
let returnFocusTarget: FocusControl | null = null
const isComparison = computed(() => props.preview.assessment.kind === 'existingId')
const hasComputeCommand = computed(() => (
  props.preview.manifest.schemaVersion === 4
  && props.preview.manifest.contributions.some(
    (command) => command.actionId === 'sandbox.computeSeries',
  )
))

function commandPreviewLabel(command: PluginCommandContribution): string {
  if (command.actionId === 'host.showInfo') return `显示信息：${command.title}`
  if (command.actionId === 'host.openPage') {
    return `打开页面：${pluginPageLabel(command.params.destination)}`
  }
  return `运行计算：${command.title}`
}

function requestCancel(event?: { preventDefault: () => void }): void {
  event?.preventDefault()
  if (props.committing || actionRequested.value) return
  actionRequested.value = true
  emit('cancel')
}

function requestConfirm(): void {
  if (isComparison.value || props.committing || props.stale || actionRequested.value) return
  actionRequested.value = true
  emit('confirm')
}

watch(() => props.preview.token, () => {
  actionRequested.value = false
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
  if (returnFocusTarget?.isConnected) returnFocusTarget.focus()
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
          本地插件清单
        </p>
        <h2 id="plugin-import-title">
          {{ isComparison ? '比较现有插件清单' : '确认导入此清单' }}
        </h2>
      </header>

      <p
        v-if="isComparison"
        id="plugin-import-warning"
        class="plugin-import-dialog__warning"
        data-testid="plugin-import-comparison-notice"
      >
        已发现相同插件 ID。此比较不会覆盖、更新或回滚现有插件，也不会写入插件目录；如需变更，请按文档手动处理。
      </p>
      <PluginManifestComparison
        v-if="props.preview.assessment.kind === 'existingId'"
        :current="props.preview.assessment.current"
        :incoming="props.preview.manifest"
        :version-relation="props.preview.assessment.versionRelation"
      />

      <dl v-if="!isComparison" class="plugin-import-dialog__metadata">
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

      <p v-if="!isComparison" id="plugin-import-warning" class="plugin-import-dialog__warning">
        目录中未发现相同插件 ID；确认后将作为新清单导入。发布者信息由清单作者填写，未经认证。
        <template v-if="hasComputeCommand">
          本次将复制包含可执行的本地 WebAssembly 代码的清单，但导入和启用不会执行代码或授予权限。导入后默认停用；只有明确点击运行才会在受限沙箱中计算，输入和结果仅保存在内存中。
        </template>
        <template v-else>
          本次只复制清单，不运行代码或授予权限。导入后默认停用，{{ props.preview.manifest.schemaVersion !== 1
            ? props.preview.manifest.schemaVersion >= 3
              ? '启用后可显示普通文本，或由宿主打开白名单页面。'
              : '启用后提供只读命令，仅由宿主显示普通文本。'
            : '启用仅记录宿主偏好。' }}
        </template>
      </p>
      <div
        v-if="!isComparison && props.preview.manifest.schemaVersion !== 1"
        class="plugin-import-dialog__hint"
        data-testid="plugin-import-commands"
      >
        <p>插件命令：</p>
        <ul>
          <li v-for="command in props.preview.manifest.contributions" :key="command.contributionId">
            <bdi>{{ commandPreviewLabel(command) }}</bdi>
            <dl
              v-if="command.actionId === 'sandbox.computeSeries'"
              data-testid="plugin-import-compute-details"
            >
              <div><dt>运行时</dt><dd><bdi>{{ command.params.runtime }}</bdi></dd></div>
              <div><dt>ABI</dt><dd><bdi>{{ command.params.abi }}</bdi></dd></div>
              <div>
                <dt>代码模块</dt>
                <dd><bdi>{{ pluginComputeModuleByteLength(command) }} 字节</bdi></dd>
              </div>
              <div><dt>参数名称</dt><dd><bdi>{{ command.params.parameter.label }}</bdi></dd></div>
              <div><dt>参数默认值</dt><dd><bdi>{{ command.params.parameter.default }}</bdi></dd></div>
              <div>
                <dt>参数范围</dt>
                <dd>
                  <bdi>{{ command.params.parameter.min }} 至 {{ command.params.parameter.max }}</bdi>
                </dd>
              </div>
            </dl>
          </li>
        </ul>
      </div>
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
          {{ isComparison ? '关闭' : '取消' }}
        </button>
        <button
          class="ef-btn ef-btn-primary"
          data-testid="plugin-import-confirm"
          type="button"
          :disabled="isComparison || props.committing || props.stale"
          @click="requestConfirm"
        >
          {{ isComparison ? '仅供比较' : '确认导入' }}
        </button>
      </footer>
    </div>
  </dialog>
</template>
