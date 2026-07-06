<script setup lang="ts">
import OrderPanel from '../trading/OrderPanel.vue'
</script>

<template>
  <aside class="right-panel panel">
    <div class="panel-title">交易</div>
    <OrderPanel />
  </aside>
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
