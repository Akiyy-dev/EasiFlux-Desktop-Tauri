<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import AppButton from '../ui/AppButton.vue'
import AppCard from '../ui/AppCard.vue'
import { useRiskStore } from '../../stores/risk'
import type { RiskStatus, UpdateRiskConfigRequest } from '../../types/models'

const props = defineProps<{ active: boolean }>()
const store = useRiskStore()
const timezones = ['Asia/Shanghai', 'UTC', 'America/New_York', 'Europe/London']
const draft = reactive<UpdateRiskConfigRequest>({
  enabled: true,
  maxOrderQty: '100',
  maxPriceDeviationPct: '5',
  maxDailyOrders: 500,
  tradingDayTimezone: 'Asia/Shanghai',
})

function adopt(status: RiskStatus): void {
  draft.enabled = status.enabled
  draft.maxOrderQty = status.maxOrderQty
  draft.maxPriceDeviationPct = status.maxPriceDeviationPct
  draft.maxDailyOrders = status.maxDailyOrders
  draft.tradingDayTimezone = status.tradingDayTimezone
}

watch(() => store.status, (status) => status && adopt(status), { immediate: true })
watch(() => props.active, (active) => {
  if (active) void store.refresh().catch(() => undefined)
}, { immediate: true })

const ledgerLabel = computed(() => ({
  disabled: '已停用', ready: '正常', unavailable: '不可用',
}[store.status?.ledgerState ?? 'unavailable']))
const usage = computed(() => {
  const status = store.status
  if (!status || status.occupiedOrders === null || status.remainingOrders === null) return '--'
  return `${status.occupiedOrders} 已占用 · ${status.remainingOrders} 剩余`
})
const updatedAt = computed(() => store.status?.updatedAtMs
  ? new Date(store.status.updatedAtMs).toLocaleString()
  : '--')

async function save(): Promise<void> {
  await store.save({ ...draft }).catch(() => undefined)
}

async function refresh(): Promise<void> {
  await store.refresh().catch(() => undefined)
}
</script>

<template>
  <AppCard title="每日风控">
    <header class="risk-header">
      <div>
        <span class="eyebrow">应用全局额度</span>
        <strong data-testid="ledger-state" :class="`ledger-${store.status?.ledgerState ?? 'unavailable'}`">
          {{ ledgerLabel }}
        </strong>
      </div>
      <AppButton
        data-testid="refresh-risk"
        :disabled="store.saving"
        :loading="store.reading"
        @click="refresh"
      >
        刷新状态
      </AppButton>
    </header>

    <p v-if="store.status?.error" class="warning" role="alert">
      {{ store.status.error }}
    </p>
    <p v-else-if="store.readError" class="warning" role="alert">
      {{ store.readError }}
    </p>

    <dl class="ledger-strip">
      <div><dt>交易日</dt><dd>{{ store.status?.tradingDay ?? '--' }}</dd></div>
      <div>
        <dt>当日额度</dt>
        <dd data-testid="risk-usage">
          {{ usage }}
        </dd>
      </div>
      <div><dt>更新时间</dt><dd>{{ updatedAt }}</dd></div>
    </dl>

    <form class="risk-form" @submit.prevent="save">
      <label class="toggle-row">
        <span>启用下单风控检查</span>
        <input v-model="draft.enabled" type="checkbox">
      </label>
      <label>
        <span>最大单笔下单数量</span>
        <input v-model="draft.maxOrderQty" data-testid="max-order-qty" inputmode="decimal">
      </label>
      <label>
        <span>最大价格偏离（%）</span>
        <input v-model="draft.maxPriceDeviationPct" inputmode="decimal">
      </label>
      <label>
        <span>每日最大下单次数</span>
        <input v-model.number="draft.maxDailyOrders" type="number" min="1" step="1">
      </label>
      <label>
        <span>交易日时区</span>
        <select v-model="draft.tradingDayTimezone">
          <option v-for="timezone in timezones" :key="timezone" :value="timezone">{{ timezone }}</option>
        </select>
      </label>
      <p v-if="store.updateError" class="warning" role="alert">
        {{ store.updateError }}
      </p>
      <AppButton
        data-testid="save-risk"
        :disabled="store.reading"
        :loading="store.saving"
        @click="save"
      >
        保存更改
      </AppButton>
    </form>
  </AppCard>
</template>

<style scoped>
.risk-header { display: flex; align-items: center; justify-content: space-between; gap: var(--ef-space-3); }
.risk-header > div { display: grid; gap: 4px; }
.eyebrow, dt { color: var(--ef-color-text-secondary); font-size: 12px; letter-spacing: .06em; text-transform: uppercase; }
.ledger-ready { color: var(--ef-color-success); }
.ledger-disabled { color: var(--ef-color-text-secondary); }
.ledger-unavailable, .warning { color: var(--ef-color-danger); }
.ledger-strip { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--ef-space-2); margin: var(--ef-space-3) 0; }
.ledger-strip > div { border-left: 2px solid var(--ef-color-border); padding-left: var(--ef-space-2); }
dd { margin: 4px 0 0; font-variant-numeric: tabular-nums; }
.risk-form { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--ef-space-3); }
label { display: grid; gap: 6px; color: var(--ef-color-text-secondary); font-size: 13px; }
input, select { min-height: 36px; border: 1px solid var(--ef-color-border); border-radius: var(--ef-radius-sm); padding: 0 var(--ef-space-2); background: var(--ef-color-surface); color: var(--ef-color-text); }
input:focus-visible, select:focus-visible { outline: 2px solid var(--ef-color-ring); outline-offset: 2px; }
.toggle-row { grid-column: 1 / -1; display: flex; align-items: center; justify-content: space-between; }
.toggle-row input { min-height: auto; }
.warning { grid-column: 1 / -1; margin: 0; }
.risk-form > :last-child { justify-self: start; }
@media (max-width: 700px) { .ledger-strip, .risk-form { grid-template-columns: 1fr; } }
</style>
