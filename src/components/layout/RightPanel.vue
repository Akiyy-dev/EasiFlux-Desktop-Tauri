<script setup lang="ts">
import AppCard from '../ui/AppCard.vue'
import OrderPanel from '../trading/OrderPanel.vue'
</script>

<template>
  <AppCard title="交易" flush class="right-panel">
    <OrderPanel />
  </AppCard>
</template>

<style scoped>
.right-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: auto;
}
</style>
