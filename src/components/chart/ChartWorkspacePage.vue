<script setup lang="ts">
import KlineChart from '../market/KlineChart.vue'

defineProps<{ active: boolean }>()
</script>

<template>
  <section class="chart-workspace-page" data-testid="chart-workspace-page">
    <KlineChart mode="workspace" :active="active" />
  </section>
</template>

<style scoped>
.chart-workspace-page {
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}
</style>
