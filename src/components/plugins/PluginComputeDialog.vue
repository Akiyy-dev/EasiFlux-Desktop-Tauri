<script setup lang="ts">
import { computed, onBeforeUnmount, ref, useId } from 'vue'
import {
  cancelPluginCompute,
  executePluginCompute,
  parsePluginComputeInput,
  parsePluginComputeParameter,
  pluginErrorMessage,
} from '../../services/pluginService'
import type {
  PluginComputeExecutionIntent,
  PluginComputeResult,
} from '../../types/plugin'

const props = defineProps<{
  intent: PluginComputeExecutionIntent
}>()

const emit = defineEmits<{
  close: []
}>()

const instanceId = useId()
const titleId = `${instanceId}-plugin-compute-title`
const inputId = `${instanceId}-plugin-compute-input`
const parameterId = `${instanceId}-plugin-compute-parameter`
const parameterRangeId = `${instanceId}-plugin-compute-parameter-range`

type ComputeStatus = 'idle' | 'running' | 'cancelling' | 'completed' | 'error'

let ownerSequence = 0
let activeRequest: { owner: number; requestId: string; cancelRequested: boolean } | null = null
let disposed = false
let closeWhenSettled = false

const inputText = ref('')
const parameterText = ref(String(props.intent.parameter.default))
const status = ref<ComputeStatus>('idle')
const error = ref<string | null>(null)
const result = ref<PluginComputeResult | null>(null)
const busy = computed(() => status.value === 'running' || status.value === 'cancelling')

const statusLabel = computed(() => ({
  idle: '等待输入',
  running: '正在运行',
  cancelling: '正在取消',
  completed: '已完成',
  error: '错误',
})[status.value])

function nextRequestId(): string | null {
  const randomUUID = globalThis.crypto?.randomUUID
  return typeof randomUUID === 'function'
    ? `compute-${randomUUID.call(globalThis.crypto)}`
    : null
}

async function cancelActive(): Promise<void> {
  const active = activeRequest
  if (!active) return
  active.cancelRequested = true
  status.value = 'cancelling'
  try {
    await cancelPluginCompute(active.requestId)
  } catch {
    // The execute request remains authoritative and keeps the form busy until it settles.
  }
}

async function run(): Promise<void> {
  if (busy.value) return
  const parsedInput = parsePluginComputeInput(inputText.value)
  if (!parsedInput.ok) {
    result.value = null
    error.value = parsedInput.error
    status.value = 'error'
    return
  }
  const parsedParameter = parsePluginComputeParameter(parameterText.value, props.intent.parameter)
  if (!parsedParameter.ok) {
    result.value = null
    error.value = parsedParameter.error
    status.value = 'error'
    return
  }

  const owner = ++ownerSequence
  const requestId = nextRequestId()
  if (requestId === null) {
    result.value = null
    error.value = '无法创建安全的计算请求标识，请重试。'
    status.value = 'error'
    return
  }
  activeRequest = { owner, requestId, cancelRequested: false }
  closeWhenSettled = false
  status.value = 'running'
  error.value = null
  result.value = null

  try {
    const computed = await executePluginCompute({
      requestId,
      pluginId: props.intent.pluginId,
      contributionId: props.intent.contributionId,
      expectedCatalogGeneration: props.intent.expectedCatalogGeneration,
      expectedRevision: props.intent.expectedRevision,
      values: parsedInput.values,
      parameter: parsedParameter.value,
    })
    if (disposed || activeRequest?.owner !== owner) return
    if (activeRequest.cancelRequested) {
      error.value = pluginErrorMessage({
        code: 'plugin_compute_cancelled',
        message: 'cancelled',
      })
      status.value = 'error'
      return
    }
    result.value = computed
    status.value = 'completed'
  } catch (caught) {
    if (disposed || activeRequest?.owner !== owner) return
    error.value = activeRequest.cancelRequested
      ? pluginErrorMessage({ code: 'plugin_compute_cancelled', message: 'cancelled' })
      : pluginErrorMessage(caught)
    status.value = 'error'
  } finally {
    if (!disposed && activeRequest?.owner === owner) {
      activeRequest = null
      if (closeWhenSettled) emit('close')
    }
  }
}

function requestClose(): void {
  if (!activeRequest) {
    emit('close')
    return
  }
  closeWhenSettled = true
  void cancelActive()
}

onBeforeUnmount(() => {
  disposed = true
  ownerSequence++
  if (activeRequest) void cancelPluginCompute(activeRequest.requestId).catch(() => undefined)
  activeRequest = null
})
</script>

<template>
  <section
    class="plugin-compute-dialog ef-card"
    data-testid="plugin-compute-dialog"
    role="region"
    :aria-labelledby="titleId"
    :aria-busy="busy || undefined"
  >
    <header>
      <p>本地 WebAssembly 计算</p>
      <h3 :id="titleId">
        {{ props.intent.title }}
      </h3>
    </header>
    <p>
      只有点击“运行”才会执行此插件代码。输入和结果仅保存在当前内存中，不会写入插件或应用数据。
    </p>
    <form @submit.prevent>
      <label :for="inputId">
        <span>数值序列</span>
        <textarea
          :id="inputId"
          v-model="inputText"
          data-testid="plugin-compute-input"
          rows="5"
          :disabled="busy"
          placeholder="例如：1, 2, 3, 4, 5"
        />
      </label>
      <label :for="parameterId">
        <span>{{ props.intent.parameter.label }}</span>
        <input
          :id="parameterId"
          v-model="parameterText"
          data-testid="plugin-compute-parameter"
          type="text"
          inputmode="decimal"
          :disabled="busy"
          :aria-describedby="parameterRangeId"
        >
      </label>
      <p :id="parameterRangeId">
        允许范围：{{ props.intent.parameter.min }} 至 {{ props.intent.parameter.max }}
      </p>

      <p data-testid="plugin-compute-status" role="status" aria-live="polite">
        状态：{{ statusLabel }}
      </p>
      <p
        v-if="error"
        data-testid="plugin-compute-error"
        role="alert"
      >
        {{ error }}
      </p>
      <section v-if="result" aria-label="计算结果">
        <p data-testid="plugin-compute-result">
          结果：{{ result.value }}
        </p>
        <p data-testid="plugin-compute-provenance">
          来源：{{ props.intent.pluginName }}（{{ result.pluginId }}） / {{ result.contributionId }}；
          输入 {{ result.inputCount }} 个；{{ props.intent.parameter.label }} {{ result.parameter }}。
        </p>
      </section>

      <footer>
        <button
          class="ef-btn ef-btn-primary"
          data-testid="plugin-compute-run"
          type="button"
          :disabled="busy"
          @click="run"
        >
          运行
        </button>
        <button
          class="ef-btn ef-btn-secondary"
          data-testid="plugin-compute-cancel"
          type="button"
          :disabled="!busy || status === 'cancelling'"
          @click="cancelActive"
        >
          取消运行
        </button>
        <button
          class="ef-btn ef-btn-secondary"
          data-testid="plugin-compute-close"
          type="button"
          @click="requestClose"
        >
          关闭
        </button>
      </footer>
    </form>
  </section>
</template>

<style scoped>
.plugin-compute-dialog {
  display: grid;
  gap: 0.9rem;
  margin-top: 1rem;
  padding: 1rem;
}

.plugin-compute-dialog form,
.plugin-compute-dialog label {
  display: grid;
  gap: 0.45rem;
}

.plugin-compute-dialog textarea,
.plugin-compute-dialog input {
  width: 100%;
  border: 1px solid var(--border);
  border-radius: 0.5rem;
  background: var(--surface);
  color: var(--foreground);
  padding: 0.65rem 0.75rem;
  font: inherit;
}

.plugin-compute-dialog footer {
  display: flex;
  flex-wrap: wrap;
  gap: 0.65rem;
}
</style>
