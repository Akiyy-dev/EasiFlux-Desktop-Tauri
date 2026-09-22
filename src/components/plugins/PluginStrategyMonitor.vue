<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { PluginStrategyExecutionIntent } from '../../types/plugin'
import type { StrategyRunReason, StrategyRunView } from '../../types/pluginStrategy'
import {
  controlPluginStrategy,
  getPluginStrategyAccess,
  listPluginStrategies,
  pluginStrategyErrorMessage,
  reconcilePluginStrategy,
  startPluginStrategy,
  stopAllPluginStrategies,
} from '../../services/pluginStrategyService'

const props = withDefaults(defineProps<{
  strategyIntents?: PluginStrategyExecutionIntent[]
  pollIntervalMs?: number
  deferInitialLoad?: boolean
}>(), {
  strategyIntents: () => [],
  pollIntervalMs: 5000,
  deferInitialLoad: false,
})

let disposed = false
let generation = 0
let pollTimer: ReturnType<typeof globalThis.setInterval> | null = null
const runs = ref<StrategyRunView[]>([])
const loading = ref(false)
const mutating = ref(new Set<string>())
const resumeAcknowledgements = ref(new Set<string>())
const error = ref<string | null>(null)

const reasonCopy: Record<StrategyRunReason, string> = {
  plugin_strategy_paused: '用户已暂停',
  plugin_strategy_stopped: '用户已停止',
  plugin_strategy_restarted: '应用重启后等待重新授权',
  plugin_strategy_expired: '运行期限已到',
  plugin_strategy_limit_reached: '动作或数量额度已用尽',
  plugin_strategy_stale: '插件、账户或授权上下文已变化',
  plugin_strategy_data_unavailable: '权威数据暂不可用',
  plugin_strategy_compute_failed: '策略计算失败',
  plugin_strategy_invalid_output: '策略输出无效',
  plugin_strategy_recovery_required: '存在未决动作，需要核对',
  plugin_strategy_storage_unavailable: '策略持久化不可用',
  plugin_strategy_ack_failed: '订单回执确认未完成',
  plugin_strategy_unavailable: '策略运行服务不可用',
}

const hasRuns = computed(() => runs.value.length > 0)

function nextGeneration(): number {
  generation += 1
  return generation
}

function replaceRun(run: StrategyRunView): void {
  const index = runs.value.findIndex((candidate) => candidate.runId === run.runId)
  if (index === -1) runs.value = [run, ...runs.value]
  else runs.value = runs.value.map((candidate, candidateIndex) => candidateIndex === index ? run : candidate)
}

async function refresh(preserveError = false, duringMutation = false): Promise<void> {
  if (loading.value || disposed || (!duringMutation && mutating.value.size > 0)) return
  const owner = nextGeneration()
  loading.value = true
  if (!preserveError) error.value = null
  try {
    const result = await listPluginStrategies()
    if (disposed || owner !== generation) return
    runs.value = result.runs
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = pluginStrategyErrorMessage(caught)
  } finally {
    if (!disposed && owner === generation) loading.value = false
  }
}

async function control(run: StrategyRunView, action: 'pause' | 'stop'): Promise<void> {
  if (mutating.value.size > 0) return
  const owner = nextGeneration()
  mutating.value = new Set(mutating.value).add(run.runId)
  error.value = null
  try {
    const updated = await controlPluginStrategy(run.runId, action)
    if (disposed || owner !== generation) return
    replaceRun(updated)
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = `${pluginStrategyErrorMessage(caught)} 当前状态未假定为已${action === 'pause' ? '暂停' : '停止'}，正在重新读取权威列表。`
    await refresh(true, true)
  } finally {
    const next = new Set(mutating.value)
    next.delete(run.runId)
    mutating.value = next
  }
}

async function stopAll(): Promise<void> {
  if (mutating.value.size > 0) return
  const owner = nextGeneration()
  mutating.value = new Set(mutating.value).add('*')
  error.value = null
  try {
    const updated = await stopAllPluginStrategies()
    if (disposed || owner !== generation) return
    runs.value = updated.runs
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = `${pluginStrategyErrorMessage(caught)} 未假定任何运行已经停止，正在重新读取权威列表。`
    await refresh(true, true)
  } finally {
    const next = new Set(mutating.value)
    next.delete('*')
    mutating.value = next
  }
}

async function reconcile(run: StrategyRunView): Promise<void> {
  if (run.status !== 'recoveryRequired' || mutating.value.size > 0) return
  const owner = nextGeneration()
  mutating.value = new Set(mutating.value).add(run.runId)
  error.value = null
  try {
    const updated = await reconcilePluginStrategy(run.runId)
    if (disposed || owner !== generation) return
    replaceRun(updated)
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = pluginStrategyErrorMessage(caught)
    await refresh(true, true)
  } finally {
    const next = new Set(mutating.value)
    next.delete(run.runId)
    mutating.value = next
  }
}

function findIntent(run: StrategyRunView): PluginStrategyExecutionIntent | null {
  return props.strategyIntents.find((intent) => (
    intent.pluginId === run.pluginId && intent.contributionId === run.contributionId
  )) ?? null
}

function canResume(run: StrategyRunView): boolean {
  return ['paused', 'stopped', 'faulted'].includes(run.status)
    && run.reason !== 'plugin_strategy_expired'
    && run.reason !== 'plugin_strategy_limit_reached'
    && BigInt(run.expiresAtMs) > BigInt(Date.now())
    && run.actionsSubmitted < run.policy.maxActions
    && findIntent(run) !== null
}

function resumeAcknowledged(runId: string): boolean {
  return resumeAcknowledgements.value.has(runId)
}

function setResumeAcknowledgement(runId: string, checked: boolean): void {
  const next = new Set(resumeAcknowledgements.value)
  if (checked) next.add(runId)
  else next.delete(runId)
  resumeAcknowledgements.value = next
}

async function resume(run: StrategyRunView): Promise<void> {
  const intent = findIntent(run)
  if (!intent || !resumeAcknowledged(run.runId) || mutating.value.size > 0) return
  const requestId = globalThis.crypto?.randomUUID?.()
  if (!requestId) {
    error.value = '无法创建安全的恢复请求标识。'
    return
  }
  mutating.value = new Set(mutating.value).add(run.runId)
  const owner = nextGeneration()
  error.value = null
  try {
    // Pause/stop invalidates every earlier ticket. Resume always obtains a new
    // one after this explicit click and reuses the durable policy and counters.
    const access = await getPluginStrategyAccess(intent)
    if (disposed || owner !== generation) return
    const resumed = await startPluginStrategy(intent, access, {
      requestId,
      resumeRunId: run.runId,
      symbol: run.symbol,
      inputJson: run.inputJson,
      capabilities: [...run.capabilities],
      policy: { ...run.policy },
      acknowledgeAutomaticTrading: true,
    })
    if (disposed || owner !== generation) return
    replaceRun(resumed)
  } catch (caught) {
    if (disposed || owner !== generation) return
    error.value = `${pluginStrategyErrorMessage(caught)} 不会使用旧凭据或自动重试，正在按 requestId 重新读取。`
    await refresh(true, true)
  } finally {
    const acknowledgements = new Set(resumeAcknowledgements.value)
    acknowledgements.delete(run.runId)
    resumeAcknowledgements.value = acknowledgements
    const next = new Set(mutating.value)
    next.delete(run.runId)
    mutating.value = next
  }
}

function remainingLabel(expiresAtMs: string): string {
  const value = BigInt(expiresAtMs)
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) return `到期时间 ${expiresAtMs} ms`
  const remaining = Math.max(0, Math.ceil((Number(value) - Date.now()) / 1000))
  return `剩余约 ${remaining} 秒（${new Date(Number(value)).toLocaleString()} 到期）`
}

onMounted(() => {
  if (props.deferInitialLoad) globalThis.queueMicrotask(() => { void refresh() })
  else void refresh()
  pollTimer = globalThis.setInterval(
    () => { void refresh() },
    Math.max(1000, props.pollIntervalMs),
  )
})
watch(
  () => props.strategyIntents,
  () => {
    nextGeneration()
    resumeAcknowledgements.value = new Set()
  },
  { deep: true },
)
onBeforeUnmount(() => {
  disposed = true
  nextGeneration()
  if (pollTimer !== null) globalThis.clearInterval(pollTimer)
})
</script>

<template>
  <section class="plugin-strategy-monitor ef-card" data-testid="plugin-strategy-monitor">
    <header>
      <div>
        <h2>自动策略监控</h2>
        <p>此监控独立于启动面板、插件卡片和账户连接；关闭面板、停用或移除插件都不会隐藏已有运行。</p>
      </div>
      <div class="plugin-strategy-monitor__actions">
        <button class="ef-btn ef-btn-secondary ef-btn-sm" type="button" :disabled="loading || mutating.size > 0" @click="refresh()">
          重新读取
        </button>
        <button
          class="ef-btn ef-btn-danger ef-btn-sm"
          data-testid="strategy-stop-all"
          type="button"
          :disabled="mutating.size > 0"
          @click="stopAll"
        >
          紧急停止全部策略
        </button>
      </div>
    </header>

    <p>应用退出、电脑休眠或数据过旧时策略不会继续运行；重新连接不会自动恢复交易权限。</p>
    <p class="plugin-strategy-monitor__warning">
      停止不会自动撤销交易所订单、平仓或清算；它只撤销后续策略准入。
    </p>
    <p v-if="loading && !hasRuns" role="status">
      正在读取原生策略运行…
    </p>
    <p v-if="error" data-testid="strategy-monitor-error" role="alert">
      {{ error }}
    </p>
    <p v-if="!loading && !hasRuns">
      当前没有持久化策略运行记录。
    </p>

    <article
      v-for="run in runs"
      :key="run.runId"
      class="plugin-strategy-monitor__run"
      :data-run-id="run.runId"
    >
      <header>
        <h3>{{ run.pluginId }} / {{ run.contributionId }}</h3>
        <strong>{{ run.status }}</strong>
      </header>
      <dl>
        <div><dt>账户 / 环境</dt><dd>{{ run.account.accountId }} / {{ run.account.environment }}</dd></div>
        <div><dt>交易对</dt><dd>{{ run.symbol }}</dd></div>
        <div><dt>动作预算</dt><dd>{{ run.actionsSubmitted }} / {{ run.policy.maxActions }}</dd></div>
        <div><dt>累计提交数量</dt><dd>{{ run.totalSubmittedQty }} / {{ run.policy.maxTotalQty }}（交易所数量单位）</dd></div>
        <div><dt>单笔数量上限</dt><dd>{{ run.policy.maxOrderQty }}（不是美元金额或持仓/盈亏上限）</dd></div>
        <div><dt>到期</dt><dd>{{ remainingLabel(run.expiresAtMs) }}</dd></div>
        <div><dt>序列</dt><dd>{{ run.sequence }}</dd></div>
        <div>
          <dt>最后消息</dt><dd class="plugin-strategy-monitor__plain">
            {{ run.lastMessage || '无' }}
          </dd>
        </div>
      </dl>
      <p v-if="run.reason">
        原因：{{ reasonCopy[run.reason] }}
      </p>
      <section v-if="run.lastReceipt">
        <h4>最后回执</h4>
        <p>
          {{ run.lastReceipt.kind }} / {{ run.lastReceipt.status }}；提交标识
          {{ run.lastReceipt.submissionId ?? '无' }}；订单 ID {{ run.lastReceipt.orderId ?? '无' }}。
          accepted 不代表成交、盈利或撤单终态，filled 必须从权威订单状态另行确认。
        </p>
        <p v-if="run.lastReceipt.errorCode">
          {{ run.lastReceipt.errorCode === 'plugin_strategy_rejected'
            ? '交易所明确拒绝了本次动作。'
            : '动作结果未知，禁止自动重试；请使用核对操作。' }}
        </p>
      </section>
      <div class="plugin-strategy-monitor__actions">
        <button
          v-if="run.status === 'running'"
          class="ef-btn ef-btn-secondary ef-btn-sm"
          data-testid="strategy-pause"
          type="button"
          :disabled="mutating.size > 0"
          @click="control(run, 'pause')"
        >
          暂停
        </button>
        <button
          v-if="!['stopped', 'completed'].includes(run.status)"
          class="ef-btn ef-btn-danger ef-btn-sm"
          data-testid="strategy-stop"
          type="button"
          :disabled="mutating.size > 0"
          @click="control(run, 'stop')"
        >
          停止
        </button>
        <button
          v-if="run.status === 'recoveryRequired'"
          class="ef-btn ef-btn-secondary ef-btn-sm"
          data-testid="strategy-reconcile"
          type="button"
          :disabled="mutating.size > 0"
          @click="reconcile(run)"
        >
          仅核对未决结果
        </button>
      </div>
      <div v-if="canResume(run)" class="plugin-strategy-monitor__resume">
        <label>
          <input
            data-testid="strategy-resume-ack"
            type="checkbox"
            :checked="resumeAcknowledged(run.runId)"
            @change="setResumeAcknowledgement(run.runId, ($event.target as HTMLInputElement).checked)"
          >
          我明确同意使用不变的策略、权限、政策和剩余额度恢复真实自动交易。
        </label>
        <button
          class="ef-btn ef-btn-danger ef-btn-sm"
          data-testid="strategy-resume"
          type="button"
          :disabled="!resumeAcknowledged(run.runId) || mutating.size > 0"
          @click="resume(run)"
        >
          获取新凭据并恢复
        </button>
      </div>
    </article>
  </section>
</template>

<style scoped>
.plugin-strategy-monitor { display: grid; gap: 1rem; margin: 1rem 0; padding: 1rem; }
.plugin-strategy-monitor > header,
.plugin-strategy-monitor__run > header { display: flex; justify-content: space-between; gap: 1rem; align-items: start; }
.plugin-strategy-monitor__run { display: grid; gap: 0.75rem; border: 1px solid var(--border); border-radius: 0.65rem; padding: 1rem; }
.plugin-strategy-monitor__run dl { display: grid; gap: 0.4rem; margin: 0; }
.plugin-strategy-monitor__run dl > div { display: grid; grid-template-columns: minmax(8rem, 0.35fr) minmax(0, 1fr); gap: 0.75rem; }
.plugin-strategy-monitor__run dd { margin: 0; overflow-wrap: anywhere; }
.plugin-strategy-monitor__actions { display: flex; flex-wrap: wrap; gap: 0.55rem; }
.plugin-strategy-monitor__warning { border-left: 3px solid var(--danger); padding-left: 0.75rem; }
.plugin-strategy-monitor__plain { white-space: pre-wrap; unicode-bidi: plaintext; }
.plugin-strategy-monitor__resume { display: grid; gap: 0.55rem; }
</style>
