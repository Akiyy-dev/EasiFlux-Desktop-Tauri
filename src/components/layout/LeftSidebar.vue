<script setup lang="ts">
import MarketList from '../market/MarketList.vue'
</script>

<template>
  <aside class="left-sidebar panel">
    <div class="panel-title">市场</div>
    <MarketList />
  </aside>
</template>

<style scoped>
.left-sidebar {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
</style>
