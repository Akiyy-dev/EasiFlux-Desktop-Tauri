<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { PluginStrategyExecutionIntent } from '../../types/plugin'
import type {
  PluginStrategyCapability,
  StrategyAccess,
  StrategyPolicy,
  StrategyRunView,
} from '../../types/pluginStrategy'
import {
  getPluginStrategyAccess,
  listPluginStrategies,
  parsePluginStrategyPolicy,
  parsePluginStrategySymbol,
  pluginStrategyErrorMessage,
  startPluginStrategy,
} from '../../services/pluginStrategyService'
import { pluginCapabilityLabels } from './pluginPresentation'

const props = defineProps<{ intent: PluginStrategyExecutionIntent }>()
const emit = defineEmits<{ close: [] }>()

type Status = 'loading' | 'ready' | 'starting' | 'started' | 'error' | 'uncertain'

let generation = 0
let disposed = false
const status = ref<Status>('loading')
const access = ref<StrategyAccess | null>(null)
const selectedCapabilities = ref<PluginStrategyCapability[]>([])
const symbol = ref('BTCUSDT')
const inputJson = ref(props.intent.defaultInput)
const policy = ref<StrategyPolicy>({
  intervalMs: 5000,
  maxOrderQty: '0.001',
  maxTotalQty: '0.01',
  maxActions: 20,
  maxRunSeconds: 3600,
  reduceOnly: false,
})
const acknowledged = ref(false)
const startedRun = ref<StrategyRunView | null>(null)
const error = ref<string | null>(null)

const policyValid = computed(() => {
  try {
    parsePluginStrategyPolicy(policy.value)
    return true
  } catch {
    return false
  }
})

const inputValid = computed(() => {
  try {
    const value: unknown = JSON.parse(inputJson.value)
    return typeof value === 'object' && value !== null && !Array.isArray(value)
      && new globalThis.TextEncoder().encode(inputJson.value).byteLength <= 4096
  } catch {
    return false
  }
})

const symbolValid = computed(() => {
  try {
    parsePluginStrategySymbol(symbol.value)
    return true
  } catch {
    return false
  }
})

const canStart = computed(() => (
  status.value === 'ready'
  && access.value !== null
  && selectedCapabilities.value.includes('account.read')
  && selectedCapabilities.value.includes('strategy.run')
  && acknowledged.value
  && policyValid.value
  && inputValid.value
  && symbolValid.value
))

function nextGeneration(): number {
  generation += 1
  return generation
}

async function loadAccess(): Promise<void> {
  const owner = nextGeneration()
  status.value = 'loading'
  access.value = null
  selectedCapabilities.value = []
  acknowledged.value = false
  startedRun.value = null
  error.value = null
  try {
    const result = await getPluginStrategyAccess(props.intent)
    if (disposed || owner !== generation) return
    access.value = result
    status.value = 'ready'
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = pluginStrategyErrorMessage(caught)
    status.value = 'error'
  }
}

function capabilityChanged(capability: PluginStrategyCapability, checked: boolean): void {
  if (status.value !== 'ready') return
  const selected = new Set(selectedCapabilities.value)
  if (checked) selected.add(capability)
  else selected.delete(capability)
  selectedCapabilities.value = props.intent.requestedCapabilities.filter((item) => selected.has(item))
}

function createRequestId(): string | null {
  return typeof globalThis.crypto?.randomUUID === 'function'
    ? globalThis.crypto.randomUUID()
    : null
}

async function start(): Promise<void> {
  const currentAccess = access.value
  if (!currentAccess || !canStart.value) return
  const requestId = createRequestId()
  if (!requestId) {
    error.value = '无法创建安全的策略请求标识，请重新打开后重试。'
    return
  }
  const owner = nextGeneration()
  status.value = 'starting'
  error.value = null
  try {
    const result = await startPluginStrategy(props.intent, currentAccess, {
      requestId,
      resumeRunId: null,
      symbol: parsePluginStrategySymbol(symbol.value),
      inputJson: inputJson.value,
      capabilities: [...selectedCapabilities.value],
      policy: { ...policy.value },
      acknowledgeAutomaticTrading: true,
    })
    if (disposed || owner !== generation) return
    startedRun.value = result
    status.value = 'started'
  } catch (caught) {
    if (disposed || owner !== generation) return
    status.value = 'uncertain'
    error.value = pluginStrategyErrorMessage(caught)
    try {
      const result = await listPluginStrategies()
      if (disposed || owner !== generation) return
      startedRun.value = result.runs.find((run) => run.requestId === requestId) ?? null
    } catch {
      // Keep the original uncertain result visible. A later explicit reload is safe.
    }
  }
}

onMounted(() => { void loadAccess() })
onBeforeUnmount(() => {
  disposed = true
  nextGeneration()
})
</script>

<template>
  <section class="plugin-strategy-dialog ef-card" data-testid="strategy-dialog" role="region">
    <header>
      <p>原生监督的自动交易策略</p>
      <h3>{{ props.intent.title }}</h3>
      <p>{{ props.intent.pluginName }}（{{ props.intent.pluginId }}）/ {{ props.intent.contributionId }}</p>
    </header>

    <p class="plugin-strategy-dialog__warning">
      导入、启用、打开或刷新页面都不会启动策略。启动后，原生宿主可在下列硬限制内自动下单或撤单，不再逐单确认。
    </p>
    <p v-if="status === 'loading'" role="status">
      正在获取 60 秒内有效的一次性启动凭据…
    </p>

    <template v-if="access">
      <dl>
        <div><dt>账户</dt><dd>{{ access.account.accountId }}</dd></div>
        <div><dt>环境</dt><dd>{{ access.account.environment }}</dd></div>
        <div><dt>启动凭据到期</dt><dd>{{ access.expiresAtMs }} ms</dd></div>
      </dl>

      <fieldset :disabled="status !== 'ready'">
        <legend>本次运行权限（初始全部未选）</legend>
        <label v-for="capability in access.requestedCapabilities" :key="capability">
          <input
            data-testid="strategy-capability"
            type="checkbox"
            :value="capability"
            :checked="selectedCapabilities.includes(capability)"
            @change="capabilityChanged(capability, ($event.target as HTMLInputElement).checked)"
          >
          <span>{{ pluginCapabilityLabels[capability] }} · {{ capability }}</span>
        </label>
      </fieldset>

      <form @submit.prevent>
        <label>交易对 <input v-model="symbol" data-testid="strategy-symbol" maxlength="32"></label>
        <label>策略输入（JSON 对象）
          <textarea v-model="inputJson" data-testid="strategy-input" rows="4" />
        </label>
        <label>回调间隔（毫秒）
          <input v-model.number="policy.intervalMs" data-testid="strategy-interval" type="number" min="5000" max="60000">
        </label>
        <label>单笔最大数量（交易所数量单位）
          <input v-model="policy.maxOrderQty" data-testid="strategy-max-order-qty" inputmode="decimal">
        </label>
        <label>累计提交数量上限（交易所数量单位）
          <input v-model="policy.maxTotalQty" data-testid="strategy-max-total-qty" inputmode="decimal">
        </label>
        <label>最大动作数
          <input v-model.number="policy.maxActions" data-testid="strategy-max-actions" type="number" min="1" max="1000">
        </label>
        <label>最长运行秒数
          <input v-model.number="policy.maxRunSeconds" data-testid="strategy-max-seconds" type="number" min="60" max="86400">
        </label>
        <label><input v-model="policy.reduceOnly" data-testid="strategy-reduce-only" type="checkbox"> 强制所有下单只减仓</label>
        <p>累计数量包含被拒绝或结果未知的提交，撤单不会退还额度；它不是美元金额、持仓、盈亏或损失上限。</p>
        <label class="plugin-strategy-dialog__consent">
          <input v-model="acknowledged" data-testid="strategy-acknowledgement" type="checkbox" :disabled="status !== 'ready'">
          我明确同意此运行在以上权限和限制内进行真实自动交易，不再逐单确认。
        </label>
        <button
          class="ef-btn ef-btn-danger"
          data-start-strategy
          type="button"
          :disabled="!canStart"
          @click="start"
        >
          启动真实自动交易策略
        </button>
      </form>
    </template>

    <p v-if="startedRun" data-testid="strategy-started" role="status">
      运行 {{ startedRun.runId }} 当前状态：{{ startedRun.status }}。关闭此面板不会停止原生宿主中的运行。
    </p>
    <div v-if="status === 'uncertain'" data-testid="strategy-start-uncertain" role="alert">
      <p>启动调用结果不确定，不会自动重试或重复提交。已按 requestId 重新读取运行列表。</p>
      <p v-if="startedRun">
        列表中找到了对应运行：{{ startedRun.status }}。
      </p>
      <button type="button" class="ef-btn ef-btn-secondary" @click="loadAccess">
        明确重新获取凭据（将清空选择和同意）
      </button>
    </div>
    <p v-else-if="error" role="alert">
      {{ error }}
    </p>

    <footer>
      <button class="ef-btn ef-btn-secondary" type="button" @click="emit('close')">
        关闭启动面板
      </button>
    </footer>
  </section>
</template>

<style scoped>
.plugin-strategy-dialog { display: grid; gap: 1rem; margin-top: 1rem; padding: 1rem; }
.plugin-strategy-dialog form,
.plugin-strategy-dialog fieldset,
.plugin-strategy-dialog label { display: grid; gap: 0.45rem; }
.plugin-strategy-dialog fieldset label,
.plugin-strategy-dialog__consent { grid-template-columns: auto 1fr; align-items: start; }
.plugin-strategy-dialog dl { display: grid; gap: 0.35rem; margin: 0; }
.plugin-strategy-dialog dl > div { display: grid; grid-template-columns: 9rem 1fr; gap: 0.75rem; }
.plugin-strategy-dialog dd { margin: 0; overflow-wrap: anywhere; }
.plugin-strategy-dialog__warning,
.plugin-strategy-dialog__consent { border: 1px solid var(--danger); border-radius: 0.5rem; padding: 0.75rem; }
.plugin-strategy-dialog input:not([type='checkbox']),
.plugin-strategy-dialog textarea { width: 100%; box-sizing: border-box; }
</style>
