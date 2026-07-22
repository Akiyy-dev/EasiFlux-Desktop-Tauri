<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppButton from '../ui/AppButton.vue'
import AppCard from '../ui/AppCard.vue'
import { useAccountStore } from '../../stores/account'
import { useConnectionStore } from '../../stores/connection'
import { usePositionStore } from '../../stores/position'
import { sumUnrealisedPnl } from '../../utils/dashboardAssets'

type AssetSection = 'contract' | 'positions' | 'dailyPnl' | 'funding'

const accountStore = useAccountStore()
const connectionStore = useConnectionStore()
const positionStore = usePositionStore()
const refreshing = ref(false)
const updatedAt = ref<Record<AssetSection, number | null>>({
  contract: null,
  positions: null,
  dailyPnl: null,
  funding: null,
})
const sectionErrors = ref<Record<AssetSection, string | null>>({
  contract: null,
  positions: null,
  dailyPnl: null,
  funding: null,
})

const positions = computed(() =>
  positionStore.positions.filter((position) => Number.parseFloat(position.size) !== 0),
)
const unrealisedPnl = computed(() => sumUnrealisedPnl(positions.value))

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function formatUpdatedAt(value: number | null): string {
  return value ? new Date(value).toLocaleTimeString() : '--'
}

async function refresh(): Promise<void> {
  if (!connectionStore.connected) return
  refreshing.value = true
  const tasks: Array<[AssetSection, () => Promise<void>]> = [
    ['contract', () => accountStore.refreshAccount(true)],
    ['positions', () => positionStore.refreshPositions()],
    ['dailyPnl', () => accountStore.refreshDailyPnl(true)],
    ['funding', () => accountStore.refreshFundingBalances()],
  ]
  const results = await Promise.allSettled(tasks.map(([, task]) => task()))
  results.forEach((result, index) => {
    const section = tasks[index][0]
    if (result.status === 'fulfilled') {
      updatedAt.value[section] = Date.now()
      sectionErrors.value[section] = null
    } else {
      sectionErrors.value[section] = errorMessage(result.reason)
    }
  })
  refreshing.value = false
}

onMounted(() => void refresh())
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

    <section class="asset-section">
      <h3>合约账户 <small>{{ formatUpdatedAt(updatedAt.contract) }}</small></h3>
      <p v-if="sectionErrors.contract" role="alert">
        {{ sectionErrors.contract }}
      </p>
      <p>合约权益：{{ accountStore.summary?.totalEquity ?? '--' }}</p>
      <ul>
        <li v-for="balance in accountStore.balances" :key="balance.asset">
          {{ balance.asset }}：可用 {{ balance.available }} / 冻结 {{ balance.frozen }} / 总计 {{ balance.total }}
        </li>
      </ul>
    </section>

    <section class="asset-section">
      <h3>持仓 <small>{{ formatUpdatedAt(updatedAt.positions) }}</small></h3>
      <p v-if="sectionErrors.positions" role="alert">
        {{ sectionErrors.positions }}
      </p>
      <p>未实现盈亏：{{ unrealisedPnl }}</p>
      <ul>
        <li v-for="position in positions" :key="`${position.symbol}:${position.positionIdx ?? 0}`">
          {{ position.symbol }} {{ position.side }} {{ position.size }}，未实现盈亏 {{ position.unrealisedPnl }}
        </li>
      </ul>
    </section>

    <section class="asset-section">
      <h3>当日已实现盈亏 <small>{{ formatUpdatedAt(updatedAt.dailyPnl) }}</small></h3>
      <p v-if="sectionErrors.dailyPnl" role="alert">
        {{ sectionErrors.dailyPnl }}
      </p>
      <p>{{ accountStore.dailyPnl.data?.value ?? '--' }}</p>
    </section>

    <section class="asset-section">
      <h3>资金账户 <small>{{ formatUpdatedAt(updatedAt.funding) }}</small></h3>
      <p v-if="accountStore.fundingError ?? sectionErrors.funding" role="alert">
        {{ accountStore.fundingError ?? sectionErrors.funding }}
      </p>
      <ul>
        <li v-for="balance in accountStore.fundingBalances" :key="balance.asset">
          {{ balance.asset }}：可用 {{ balance.available }} / 冻结 {{ balance.frozen }} / 总计 {{ balance.total }}
        </li>
      </ul>
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
