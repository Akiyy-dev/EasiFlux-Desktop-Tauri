<script setup lang="ts">
import { computed, watch } from 'vue'
import AppButton from '../ui/AppButton.vue'
import AppCard from '../ui/AppCard.vue'
import { useAccountStore } from '../../stores/account'
import { useConnectionStore } from '../../stores/connection'
import { usePositionStore } from '../../stores/position'
import { sumUnrealisedPnl } from '../../utils/dashboardAssets'

const props = withDefaults(defineProps<{ active?: boolean }>(), { active: true })

const accountStore = useAccountStore()
const connectionStore = useConnectionStore()
const positionStore = usePositionStore()
const refreshing = computed(() =>
  accountStore.loading
  || positionStore.loading
  || accountStore.dailyPnlLoading
  || accountStore.fundingLoading,
)

const positions = computed(() =>
  positionStore.positions.filter((position) => Number.parseFloat(position.size) !== 0),
)
const unrealisedPnl = computed(() => sumUnrealisedPnl(positions.value))

function formatUpdatedAt(value: number | null): string {
  return value ? new Date(value).toLocaleTimeString() : '--'
}

function runSection(task: () => Promise<void>): void {
  void task().catch(() => undefined)
}

function refresh(): void {
  if (!connectionStore.connected) return
  runSection(() => accountStore.refreshAccount(true))
  runSection(() => positionStore.refreshPositions())
  runSection(() => accountStore.refreshDailyPnl(true))
  runSection(() => accountStore.refreshFundingBalances())
}

watch(() => props.active, (active) => {
  if (active) void refresh()
}, { immediate: true })
</script>

<template>
  <AppCard title="资产概览">
    <header class="panel-header">
      <span>连接状态：{{ connectionStore.status }}</span>
      <AppButton data-testid="refresh-assets" :disabled="!connectionStore.connected" :loading="refreshing" @click="refresh">
        刷新
      </AppButton>
    </header>
    <p v-if="!connectionStore.connected" class="hint">
      连接账户后刷新
    </p>

    <section
      class="asset-section"
      data-testid="asset-contract"
      :data-state="accountStore.status"
    >
      <h3>合约账户 <small>{{ formatUpdatedAt(accountStore.updatedAt) }}</small></h3>
      <p v-if="accountStore.status === 'loading'">
        加载中…
      </p>
      <p v-else-if="accountStore.status === 'error'" role="alert">
        {{ accountStore.error }}
      </p>
      <p v-else-if="accountStore.status === 'empty'">
        暂无合约账户余额
      </p>
      <template v-else-if="accountStore.status === 'success'">
        <p>合约权益：{{ accountStore.summary?.totalEquity ?? '--' }}</p>
        <ul>
          <li v-for="balance in accountStore.balances" :key="balance.asset">
            {{ balance.asset }}：可用 {{ balance.available }} / 冻结 {{ balance.frozen }} / 总计 {{ balance.total }}
          </li>
        </ul>
      </template>
      <p v-else>
        尚未刷新
      </p>
    </section>

    <section
      class="asset-section"
      data-testid="asset-positions"
      :data-state="positionStore.status"
    >
      <h3>持仓 <small>{{ formatUpdatedAt(positionStore.updatedAt) }}</small></h3>
      <p v-if="positionStore.status === 'loading'">
        加载中…
      </p>
      <p v-else-if="positionStore.status === 'error'" role="alert">
        {{ positionStore.error }}
      </p>
      <p v-else-if="positionStore.status === 'empty'">
        暂无持仓
      </p>
      <template v-else-if="positionStore.status === 'success'">
        <p>未实现盈亏：{{ unrealisedPnl }}</p>
        <ul>
          <li v-for="position in positions" :key="`${position.symbol}:${position.positionIdx ?? 0}`">
            {{ position.symbol }} {{ position.side }} {{ position.size }}，未实现盈亏 {{ position.unrealisedPnl }}
          </li>
        </ul>
      </template>
      <p v-else>
        尚未刷新
      </p>
    </section>

    <section
      class="asset-section"
      data-testid="asset-daily-pnl"
      :data-state="accountStore.dailyPnlStatus"
    >
      <h3>当日已实现盈亏 <small>{{ formatUpdatedAt(accountStore.dailyPnlUpdatedAt) }}</small></h3>
      <p v-if="accountStore.dailyPnlStatus === 'loading'">
        加载中…
      </p>
      <p v-else-if="accountStore.dailyPnlStatus === 'error'" role="alert">
        {{ accountStore.dailyPnlError }}
      </p>
      <p v-else-if="accountStore.dailyPnlStatus === 'empty'">
        暂无当日已实现盈亏
      </p>
      <p v-else-if="accountStore.dailyPnlStatus === 'success'">
        {{ accountStore.dailyPnl.data?.value ?? '--' }}
      </p>
      <p v-else>
        尚未刷新
      </p>
    </section>

    <section
      class="asset-section"
      data-testid="asset-funding"
      :data-state="accountStore.fundingStatus"
    >
      <h3>资金账户 <small>{{ formatUpdatedAt(accountStore.fundingUpdatedAt) }}</small></h3>
      <p v-if="accountStore.fundingStatus === 'loading'">
        加载中…
      </p>
      <p v-else-if="accountStore.fundingStatus === 'error'" role="alert">
        {{ accountStore.fundingError }}
      </p>
      <p v-else-if="accountStore.fundingStatus === 'empty'">
        暂无资金账户余额
      </p>
      <ul v-else-if="accountStore.fundingStatus === 'success'">
        <li v-for="balance in accountStore.fundingBalances" :key="balance.asset">
          {{ balance.asset }}：可用 {{ balance.available }} / 冻结 {{ balance.frozen }} / 总计 {{ balance.total }}
        </li>
      </ul>
      <p v-else>
        尚未刷新
      </p>
    </section>
  </AppCard>
</template>

<style scoped>
.panel-header { display: flex; align-items: center; justify-content: space-between; gap: var(--ef-space-2); }
.hint, small { color: var(--ef-text-muted); }
.asset-section { border-top: 1px solid var(--ef-border); margin-top: var(--ef-space-3); padding-top: var(--ef-space-2); }
h3 { display: flex; justify-content: space-between; margin: 0; }
ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
</style>
