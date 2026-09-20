<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import { cancelPluginCompute } from '../../services/pluginService'
import {
  confirmPluginWorkflow,
  getPluginWorkflowAccess,
  isPluginWorkflowConfirmationNotSubmitted,
  parsePluginWorkflowInput,
  parsePluginWorkflowSymbol,
  pluginWorkflowErrorMessage,
  runPluginWorkflow,
  setPluginWorkflowGrants,
} from '../../services/pluginWorkflowService'
import { useAccountProfilesStore } from '../../stores/accountProfiles'
import { useConnectionStore } from '../../stores/connection'
import type { PluginWorkflowExecutionIntent } from '../../types/plugin'
import type {
  PluginWorkflowAccess,
  PluginWorkflowCapability,
  PluginWorkflowResult,
  PluginWorkflowTradeReceipt,
} from '../../types/pluginWorkflow'

const props = defineProps<{ intent: PluginWorkflowExecutionIntent }>()
const emit = defineEmits<{ close: [] }>()

const capabilityCopy: Record<PluginWorkflowCapability, { title: string; description: string }> = {
  'account.read': { title: '账户会话', description: '允许识别当前账户、环境和会话；非空授权必须包含此项。' },
  'balances.read': { title: '余额', description: '读取当前账户的资产余额快照。' },
  'positions.read': { title: '持仓', description: '读取所选交易对的当前持仓快照。' },
  'orders.read': { title: '未成交委托', description: '读取所选交易对的未成交委托；取消委托也需要此项。' },
  'market.read': { title: '市场报价', description: '读取所选交易对的最新、买一、卖一和标记价。' },
  'trade.place': { title: '下单提案', description: '允许插件准备真实订单提案；仍需单独确认才会提交。' },
  'trade.cancel': { title: '撤单提案', description: '允许插件准备真实撤单提案；仍需单独确认才会提交。' },
}

const accountProfiles = useAccountProfilesStore()
const connection = useConnectionStore()
const instanceId = useId()
const titleId = `${instanceId}-workflow-title`
const symbolId = `${instanceId}-workflow-symbol`
const inputId = `${instanceId}-workflow-input`
const inputHelpId = `${instanceId}-workflow-input-help`

type Status = 'loading' | 'ready' | 'granting' | 'running' | 'confirming' | 'error' | 'invalidated'

let ownerSequence = 0
let disposed = false
let activeRequestId: string | null = null
const status = ref<Status>('loading')
const access = ref<PluginWorkflowAccess | null>(null)
const selectedCapabilities = ref<PluginWorkflowCapability[]>([])
const symbolText = ref('BTCUSDT')
const inputText = ref(props.intent.defaultInput)
const result = ref<PluginWorkflowResult | null>(null)
const receipt = ref<PluginWorkflowTradeReceipt | null>(null)
const confirmationUnknown = ref(false)
const confirmationNotSubmitted = ref(false)
const error = ref<string | null>(null)

const busy = computed(() => ['loading', 'granting', 'running', 'confirming'].includes(status.value))
const canRun = computed(() => (
  status.value === 'ready'
  && access.value?.grantedCapabilities.includes('account.read') === true
))
const canSaveGrants = computed(() => status.value === 'ready' && access.value !== null)
const canRevoke = computed(() => (
  status.value === 'ready' && (access.value?.grantedCapabilities.length ?? 0) > 0
))

function cancelActiveRequest(): void {
  const requestId = activeRequestId
  activeRequestId = null
  if (requestId) void cancelPluginCompute(requestId).catch(() => undefined)
}

function clearExecution(cancelRunning = false): void {
  ownerSequence += 1
  if (cancelRunning) cancelActiveRequest()
  result.value = null
  receipt.value = null
  confirmationUnknown.value = false
  confirmationNotSubmitted.value = false
  error.value = null
  if (!busy.value && access.value) status.value = 'ready'
}

function accessOwner(): number {
  return ++ownerSequence
}

async function loadAccess(): Promise<void> {
  clearExecution(true)
  const owner = accessOwner()
  status.value = 'loading'
  access.value = null
  selectedCapabilities.value = []
  try {
    const loaded = await getPluginWorkflowAccess(props.intent)
    if (disposed || owner !== ownerSequence) return
    access.value = loaded
    selectedCapabilities.value = [...loaded.grantedCapabilities]
    status.value = 'ready'
  } catch (caught) {
    if (disposed || owner !== ownerSequence) return
    error.value = pluginWorkflowErrorMessage(caught)
    status.value = 'error'
  }
}

function capabilityChanged(capability: PluginWorkflowCapability, checked: boolean): void {
  const next = new Set(selectedCapabilities.value)
  if (checked) {
    next.add(capability)
    if (capability !== 'account.read') next.add('account.read')
  } else {
    next.delete(capability)
    if (capability === 'account.read') next.clear()
  }
  selectedCapabilities.value = props.intent.requestedCapabilities.filter((item) => next.has(item))
  clearExecution()
}

async function saveGrants(capabilities = selectedCapabilities.value): Promise<void> {
  const current = access.value
  if (!current || busy.value) return
  const owner = accessOwner()
  status.value = 'granting'
  result.value = null
  receipt.value = null
  confirmationUnknown.value = false
  confirmationNotSubmitted.value = false
  error.value = null
  try {
    const updated = await setPluginWorkflowGrants(props.intent, current, capabilities)
    if (disposed || owner !== ownerSequence) return
    access.value = updated
    selectedCapabilities.value = [...updated.grantedCapabilities]
    status.value = 'ready'
  } catch (caught) {
    if (disposed || owner !== ownerSequence) return
    error.value = pluginWorkflowErrorMessage(caught)
    status.value = 'error'
  }
}

function nextRequestId(): string | null {
  const randomUUID = globalThis.crypto?.randomUUID
  return typeof randomUUID === 'function'
    ? `workflow-${randomUUID.call(globalThis.crypto)}`
    : null
}

async function run(): Promise<void> {
  const current = access.value
  if (!current || !canRun.value) return
  const parsedSymbol = parsePluginWorkflowSymbol(symbolText.value)
  if (!parsedSymbol.ok) {
    error.value = parsedSymbol.error
    return
  }
  const parsedInput = parsePluginWorkflowInput(inputText.value)
  if (!parsedInput.ok) {
    error.value = parsedInput.error
    return
  }
  const requestId = nextRequestId()
  if (!requestId) {
    error.value = '无法创建安全的工作流请求标识，请重试。'
    return
  }
  const owner = accessOwner()
  activeRequestId = requestId
  status.value = 'running'
  result.value = null
  receipt.value = null
  confirmationUnknown.value = false
  confirmationNotSubmitted.value = false
  error.value = null
  try {
    const completed = await runPluginWorkflow(props.intent, current, {
      requestId, symbol: parsedSymbol.symbol, inputJson: parsedInput.inputJson,
    })
    if (disposed || owner !== ownerSequence) return
    result.value = completed
    status.value = 'ready'
  } catch (caught) {
    if (disposed || owner !== ownerSequence) return
    error.value = pluginWorkflowErrorMessage(caught)
    status.value = 'error'
  } finally {
    if (activeRequestId === requestId) activeRequestId = null
  }
}

async function confirmTrade(): Promise<void> {
  const prepared = result.value
  if (!prepared?.confirmation || prepared.output.kind === 'display'
    || busy.value || receipt.value || confirmationUnknown.value || confirmationNotSubmitted.value) {
    return
  }
  const owner = accessOwner()
  status.value = 'confirming'
  error.value = null
  try {
    const confirmed = await confirmPluginWorkflow(
      prepared.confirmation,
      prepared.output,
      prepared.account,
    )
    if (disposed || owner !== ownerSequence) return
    receipt.value = confirmed
    status.value = 'ready'
  } catch (caught) {
    if (disposed || owner !== ownerSequence) return
    // Both outcomes consume the local proposal. Only audited native codes are
    // safe to label pre-dispatch; every other failure requires reconciliation.
    confirmationNotSubmitted.value = isPluginWorkflowConfirmationNotSubmitted(caught)
    confirmationUnknown.value = !confirmationNotSubmitted.value
    error.value = null
    status.value = 'ready'
  }
}

function timestampLabel(raw: string): string {
  const value = BigInt(raw)
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) return `${raw} ms`
  return new Date(Number(value)).toLocaleString()
}

function invalidateContext(): void {
  if (disposed) return
  clearExecution(true)
  access.value = null
  selectedCapabilities.value = []
  error.value = '账户会话或连接已变化，原授权和结果已失效。请关闭后重新打开。'
  status.value = 'invalidated'
}

watch(inputText, () => clearExecution(), { flush: 'sync' })
watch(symbolText, () => clearExecution(), { flush: 'sync' })
watch(
  [() => accountProfiles.activeAccountId, () => accountProfiles.sessionEpoch, () => connection.status],
  invalidateContext,
  { flush: 'sync' },
)

onMounted(() => { void loadAccess() })
onBeforeUnmount(() => {
  disposed = true
  ownerSequence += 1
  cancelActiveRequest()
})
</script>

<template>
  <section
    class="plugin-workflow-dialog ef-card"
    data-testid="plugin-workflow-dialog"
    role="region"
    :aria-labelledby="titleId"
    :aria-busy="busy || undefined"
  >
    <header>
      <p>需明确授权的本地 WebAssembly 账户工作流</p>
      <h3 :id="titleId">
        {{ props.intent.title }}
      </h3>
      <p>{{ props.intent.pluginName }}（{{ props.intent.pluginId }}） / {{ props.intent.contributionId }}</p>
    </header>

    <p>
      安装、导入或启用插件都不会授予账户权限。授权仅在当前会话有效；账户、会话、凭据或插件内容变化后需重新授权。
    </p>

    <p v-if="status === 'loading'" data-testid="workflow-access-loading" role="status">
      正在读取权限状态…
    </p>
    <template v-if="access">
      <dl class="plugin-workflow-dialog__metadata" data-testid="workflow-account">
        <div><dt>账户</dt><dd><bdi>{{ access.account.accountId }}</bdi></dd></div>
        <div><dt>环境</dt><dd><bdi>{{ access.account.environment }}</bdi></dd></div>
        <div><dt>会话代次</dt><dd><bdi>{{ access.account.sessionEpoch }}</bdi></dd></div>
        <div><dt>授权版本</dt><dd><bdi>{{ access.grantRevision }}</bdi></dd></div>
      </dl>

      <fieldset :disabled="busy" class="plugin-workflow-dialog__capabilities">
        <legend>选择本会话授权</legend>
        <label v-for="capability in access.requestedCapabilities" :key="capability">
          <input
            data-testid="workflow-capability"
            type="checkbox"
            :value="capability"
            :checked="selectedCapabilities.includes(capability)"
            @change="capabilityChanged(capability, ($event.target as HTMLInputElement).checked)"
          >
          <span><strong>{{ capabilityCopy[capability].title }}</strong> · {{ capability }}</span>
          <small>{{ capabilityCopy[capability].description }}</small>
        </label>
      </fieldset>
      <div class="plugin-workflow-dialog__actions">
        <button
          class="ef-btn ef-btn-secondary"
          data-testid="workflow-save-grants"
          type="button"
          :disabled="!canSaveGrants"
          @click="saveGrants()"
        >
          保存选定授权
        </button>
        <button
          class="ef-btn ef-btn-secondary"
          data-testid="workflow-revoke"
          type="button"
          :disabled="!canRevoke"
          @click="saveGrants([])"
        >
          撤销全部授权
        </button>
      </div>
      <p>
        撤销会阻止后续读取和提案，无需再读取私有快照；但无法撤回已传送给交易系统的订单或撤单请求。
      </p>

      <form @submit.prevent>
        <label :for="symbolId">
          <span>交易对</span>
          <input
            :id="symbolId"
            v-model="symbolText"
            data-testid="workflow-symbol"
            type="text"
            maxlength="32"
            autocomplete="off"
            :disabled="busy"
          >
        </label>
        <label :for="inputId">
          <span>插件输入（JSON 对象）</span>
          <textarea
            :id="inputId"
            v-model="inputText"
            data-testid="workflow-input"
            rows="5"
            :aria-describedby="inputHelpId"
            :disabled="busy"
          />
        </label>
        <p :id="inputHelpId">
          最多 4096 个 UTF-8 字节。插件默认示例：<code>{{ props.intent.defaultInput }}</code>
        </p>
        <button
          class="ef-btn ef-btn-primary"
          data-testid="workflow-run"
          type="button"
          :disabled="!canRun"
          @click="run"
        >
          读取已授权快照并运行插件
        </button>
      </form>
    </template>

    <p v-if="error" data-testid="workflow-error" role="alert">
      {{ error }}
    </p>

    <section v-if="result" class="plugin-workflow-dialog__result" data-testid="workflow-snapshot">
      <header>
        <h4>已授权账户快照</h4>
        <p>
          完成于 {{ timestampLabel(result.snapshot.capturedAtMs) }}。快照不会自动刷新；请重新运行以获取新数据。
          各区段是分别获取，不代表交易所的原子快照。
        </p>
      </header>
      <section>
        <h5>余额</h5>
        <p v-if="result.snapshot.balances === null">
          未授权，未读取余额。
        </p>
        <template v-else>
          <p v-if="result.snapshot.balances.partial" data-testid="workflow-partial-warning">
            此列表可能不完整；当前接口不能证明已遍历全部分页。
          </p>
          <p>获取于 {{ timestampLabel(result.snapshot.balances.fetchedAtMs) }}。</p>
          <ul>
            <li v-for="balance in result.snapshot.balances.items" :key="balance.asset">
              <bdi>{{ balance.asset }}</bdi>：可用 {{ balance.available }}，冻结 {{ balance.frozen }}，总额 {{ balance.total }}
            </li>
          </ul>
        </template>
      </section>
      <section data-testid="workflow-positions">
        <h5>持仓</h5>
        <p v-if="result.snapshot.positions === null">
          未授权，未读取持仓。
        </p>
        <template v-else>
          <p v-if="result.snapshot.positions.partial" data-testid="workflow-partial-warning">
            此列表可能不完整。
          </p>
          <p>获取于 {{ timestampLabel(result.snapshot.positions.fetchedAtMs) }}。</p>
          <ul>
            <li v-for="position in result.snapshot.positions.items" :key="`${position.symbol}:${position.positionIdx}`">
              {{ position.symbol }} {{ position.side }}，数量 {{ position.size }}，入场价 {{ position.entryPrice }}，未实现盈亏 {{ position.unrealisedPnl }}
            </li>
          </ul>
        </template>
      </section>
      <section data-testid="workflow-orders">
        <h5>未成交委托</h5>
        <p v-if="result.snapshot.orders === null">
          未授权，未读取委托。
        </p>
        <template v-else>
          <p v-if="result.snapshot.orders.partial" data-testid="workflow-partial-warning">
            此列表可能不完整。
          </p>
          <p>获取于 {{ timestampLabel(result.snapshot.orders.fetchedAtMs) }}。</p>
          <ul>
            <li v-for="order in result.snapshot.orders.items" :key="order.orderId">
              {{ order.symbol }} {{ order.side }} {{ order.orderType }}，数量 {{ order.qty }}，价格 {{ order.price }}，状态 {{ order.status }}，订单 ID {{ order.orderId }}
            </li>
          </ul>
        </template>
      </section>
      <section>
        <h5>市场报价</h5>
        <p v-if="result.snapshot.market === null">
          未授权，未读取报价。
        </p>
        <p v-else>
          {{ result.snapshot.market.ticker.symbol }}：最新 {{ result.snapshot.market.ticker.lastPrice }}，买一 {{ result.snapshot.market.ticker.bidPrice }}，卖一 {{ result.snapshot.market.ticker.askPrice }}，标记 {{ result.snapshot.market.ticker.markPrice }}；获取于 {{ timestampLabel(result.snapshot.market.fetchedAtMs) }}。
        </p>
      </section>
      <section v-if="result.output.kind === 'display'">
        <h5>插件结果</h5>
        <p class="plugin-workflow-dialog__plain-text">
          {{ result.output.text }}
        </p>
      </section>
    </section>

    <section
      v-if="result?.confirmation && result.output.kind !== 'display' && !receipt && !confirmationUnknown && !confirmationNotSubmitted"
      class="plugin-workflow-dialog__confirmation"
      data-testid="workflow-confirmation"
      aria-label="真实交易确认"
    >
      <h4>{{ result.output.kind === 'placeOrder' ? '确认提交真实订单' : '确认提交真实撤单' }}</h4>
      <p>运行插件不会自动交易。只有下方的单独确认才会提交此不可编辑的提案。</p>
      <dl>
        <div><dt>插件</dt><dd>{{ props.intent.pluginName }}（{{ props.intent.pluginId }}）</dd></div>
        <div><dt>账户</dt><dd>{{ result.account.accountId }} / {{ result.account.environment }}</dd></div>
        <div><dt>到期</dt><dd>{{ timestampLabel(result.confirmation.expiresAtMs) }}</dd></div>
        <div><dt>提交标识</dt><dd>{{ result.confirmation.submissionId ?? '不适用（撤单）' }}</dd></div>
        <template v-if="result.output.kind === 'placeOrder'">
          <div><dt>交易对</dt><dd>{{ result.output.order.symbol }}</dd></div>
          <div><dt>方向 / 类型</dt><dd>{{ result.output.order.side }} / {{ result.output.order.orderType }}</dd></div>
          <div><dt>数量</dt><dd>{{ result.output.order.qty }}</dd></div>
          <div><dt>价格</dt><dd>{{ result.output.order.price ?? '市价' }}</dd></div>
          <div><dt>有效方式</dt><dd>{{ result.output.order.timeInForce }}</dd></div>
          <div><dt>持仓索引</dt><dd>{{ result.output.order.positionIdx }}</dd></div>
          <div><dt>只减仓</dt><dd>{{ result.output.order.reduceOnly ? '是' : '否' }}</dd></div>
        </template>
        <template v-else>
          <div><dt>交易对</dt><dd>{{ result.output.order.symbol }}</dd></div>
          <div><dt>取消订单 ID</dt><dd>{{ result.output.order.orderId }}</dd></div>
        </template>
      </dl>
      <button
        class="ef-btn ef-btn-danger"
        data-testid="workflow-confirm"
        type="button"
        :disabled="status === 'confirming'"
        @click="confirmTrade"
      >
        {{ result.output.kind === 'placeOrder' ? '确认提交真实订单' : '确认提交真实撤单' }}
      </button>
    </section>

    <section
      v-if="receipt || confirmationUnknown || confirmationNotSubmitted"
      class="plugin-workflow-dialog__receipt"
      aria-live="polite"
    >
      <p v-if="confirmationNotSubmitted" data-testid="workflow-confirmation-not-submitted">
        交易确认已过期、失效或上下文已变化，本次请求未提交。此提案已作废且不能重试；请重新运行工作流生成新提案。
      </p>
      <p v-else-if="receipt?.status === 'accepted'" data-testid="workflow-receipt-accepted">
        {{ receipt.action === 'cancelOrder'
          ? '撤单请求已受理；这不代表已确认终态取消，请在订单中心确认最终状态。'
          : '真实订单请求已被交易系统接受。' }}
      </p>
      <p v-else-if="receipt?.status === 'rejected'" data-testid="workflow-receipt-rejected">
        交易请求已被拒绝，本次确认不能重试。请重新运行工作流生成新提案。
      </p>
      <p v-else data-testid="workflow-receipt-unknown">
        交易结果未知，请前往交易页的订单中心或恢复界面核对；不要重新提交相同操作。
      </p>
    </section>

    <footer>
      <button class="ef-btn ef-btn-secondary" data-testid="workflow-close" type="button" @click="emit('close')">
        关闭工作流
      </button>
    </footer>
  </section>
</template>

<style scoped>
.plugin-workflow-dialog {
  display: grid;
  gap: 1rem;
  margin-top: 1rem;
  padding: 1rem;
}

.plugin-workflow-dialog form,
.plugin-workflow-dialog label,
.plugin-workflow-dialog__result,
.plugin-workflow-dialog__confirmation,
.plugin-workflow-dialog__receipt {
  display: grid;
  gap: 0.55rem;
}

.plugin-workflow-dialog__metadata,
.plugin-workflow-dialog__confirmation dl {
  display: grid;
  gap: 0.45rem;
  margin: 0;
}

.plugin-workflow-dialog__metadata > div,
.plugin-workflow-dialog__confirmation dl > div {
  display: grid;
  grid-template-columns: minmax(7rem, 0.35fr) minmax(0, 1fr);
  gap: 0.75rem;
}

.plugin-workflow-dialog dd { margin: 0; overflow-wrap: anywhere; }
.plugin-workflow-dialog__capabilities { display: grid; gap: 0.65rem; }
.plugin-workflow-dialog__capabilities label { grid-template-columns: auto 1fr; }
.plugin-workflow-dialog__capabilities small { grid-column: 2; color: var(--muted-foreground); }
.plugin-workflow-dialog__actions,
.plugin-workflow-dialog footer { display: flex; flex-wrap: wrap; gap: 0.65rem; }
.plugin-workflow-dialog input[type='text'],
.plugin-workflow-dialog textarea {
  width: 100%; border: 1px solid var(--border); border-radius: 0.5rem;
  background: var(--surface); color: var(--foreground); padding: 0.65rem 0.75rem; font: inherit;
}
.plugin-workflow-dialog__confirmation { border: 2px solid var(--danger); border-radius: 0.65rem; padding: 1rem; }
.plugin-workflow-dialog__plain-text { white-space: pre-wrap; unicode-bidi: plaintext; }
</style>
