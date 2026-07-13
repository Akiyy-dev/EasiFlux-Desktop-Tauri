<script setup lang="ts">
import { computed } from 'vue'
import { storeToRefs } from 'pinia'
import AppCard from '../ui/AppCard.vue'
import DashboardMetricTile from './DashboardMetricTile.vue'
import { useAccountStore } from '../../stores/account'
import { usePositionStore } from '../../stores/position'
import { formatMetric, pnlToneClass, sumUnrealisedPnl } from '../../utils/dashboardAssets'

const accountStore = useAccountStore()
const positionStore = usePositionStore()

const { summary, dailyPnl, dailyPnlLoading, dailyPnlError } = storeToRefs(accountStore)
const { positions } = storeToRefs(positionStore)

const usdtBalance = computed(() =>
  summary.value?.balances.find((balance) => balance.asset === 'USDT'),
)

const marginBalance = computed(() => usdtBalance.value?.frozen ?? summary.value?.balances[0]?.frozen)
const unrealisedPnl = computed(() => sumUnrealisedPnl(positions.value))
const unrealisedClass = computed(() => pnlToneClass(unrealisedPnl.value))
const todayPnl = computed(() => {
  if (dailyPnlLoading.value && dailyPnl.value.data == null) return '加载中'
  if (dailyPnlError.value) return '获取失败'
  return `${dailyPnl.value.data?.value ?? '0.0000'} USDT`
})
const todayPnlClass = computed(() => pnlToneClass(dailyPnl.value.data?.value ?? '0'))
</script>

<template>
  <AppCard title="资产概览">
    <div class="asset-grid">
      <DashboardMetricTile
        label="总资产"
        :value="formatMetric(summary?.totalEquity, 'USDT')"
      />
      <DashboardMetricTile
        label="今日盈亏"
        :value="todayPnl"
        :value-class="todayPnlClass"
      />
      <DashboardMetricTile
        label="未实现盈亏"
        :value="`${unrealisedPnl} USDT`"
        :value-class="unrealisedClass"
      />
      <DashboardMetricTile label="持仓数量" :value="String(positions.length)" />
      <DashboardMetricTile
        label="可用保证金"
        :value="formatMetric(usdtBalance?.available, 'USDT')"
      />
      <DashboardMetricTile
        label="保证金"
        :value="formatMetric(marginBalance, 'USDT')"
      />
    </div>
  </AppCard>
</template>

<style scoped>
.asset-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(148px, 1fr));
  gap: 8px;
}
</style>
