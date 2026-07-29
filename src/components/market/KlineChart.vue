<script setup lang="ts">
import { ref } from 'vue'
import '@klinecharts/pro/dist/klinecharts-pro.css'
import { useKlineChartWorkspace } from '../../composables/useKlineChartWorkspace'

const props = withDefaults(defineProps<{
  mode?: 'trading' | 'workspace'
  active?: boolean
}>(), {
  mode: 'trading',
  active: true,
})

const chartContainer = ref<globalThis.HTMLDivElement | null>(null)

useKlineChartWorkspace(chartContainer, props)
</script>

<template>
  <div class="chart" :class="`chart--${props.mode}`">
    <div ref="chartContainer" class="chart-view" />
  </div>
</template>

<style scoped>
.chart {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: clamp(320px, 42vh, 620px);
}

.chart--workspace {
  min-height: 0;
}

.chart-view {
  flex: 1;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  border-radius: var(--ef-radius-md);
}

.chart-view :deep(.klinecharts-pro) {
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
}
</style>
