<script setup lang="ts">
import TitleBar from './TitleBar.vue'
import LeftSidebar from './LeftSidebar.vue'
import CenterPanel from './CenterPanel.vue'
import RightPanel from './RightPanel.vue'
import BottomPanel from './BottomPanel.vue'

defineProps<{
  version: string
}>()

const emit = defineEmits<{
  openSettings: []
}>()
</script>

<template>
  <div class="app-shell">
    <TitleBar :version="version" @open-settings="emit('openSettings')" />
    <div class="main-grid">
      <LeftSidebar class="left" />
      <CenterPanel class="center" />
      <RightPanel class="right" />
    </div>
    <BottomPanel />
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  gap: 8px;
  padding: 0 8px 8px;
}

.main-grid {
  display: grid;
  grid-template-columns: 200px 1fr 280px;
  gap: 8px;
  flex: 1;
  min-height: 0;
}

.left,
.center,
.right {
  min-height: 0;
}
</style>
