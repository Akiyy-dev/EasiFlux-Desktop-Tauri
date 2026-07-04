<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { tauriInvoke } from '../../composables/useTauriCommand'
import { useConnectionStore } from '../../stores/connection'
import type { TradeStats } from '../../types/models'

const props = defineProps<{
  active?: boolean
}>()

const connectionStore = useConnectionStore()
const { connected } = storeToRefs(connectionStore)

const stats = ref<TradeStats | null>(null)

async function loadStats(): Promise<void> {
  try {
    stats.value = await tauriInvoke<TradeStats>('get_trade_stats')
  } catch {
    stats.value = null
  }
}

onMounted(() => {
  void loadStats()
})

watch(connected, (isConnected) => {
  if (isConnected) {
    void loadStats()
  }
})

watch(
  () => props.active,
  (isActive) => {
    if (isActive) {
      void loadStats()
    }
  },
)
</script>

<template>
  <div class="analytics">
    <template v-if="stats">
      <div class="row"><span>总订单</span><span>{{ stats.totalOrders }}</span></div>
      <div class="row"><span>成交</span><span>{{ stats.filledOrders }}</span></div>
      <div class="row"><span>撤销</span><span>{{ stats.cancelledOrders }}</span></div>
      <div class="row"><span>成交量</span><span>{{ stats.totalVolume }}</span></div>
      <div class="row"><span>胜率</span><span>{{ stats.winRatePct }}%</span></div>
    </template>
    <div v-else class="empty">暂无统计数据</div>
  </div>
</template>

<style scoped>
.analytics {
  padding: 12px;
  font-size: 13px;
}

.row {
  display: flex;
  justify-content: space-between;
  padding: 6px 0;
  border-bottom: 1px solid var(--border-color);
}

.empty {
  color: var(--text-secondary);
}
</style>
