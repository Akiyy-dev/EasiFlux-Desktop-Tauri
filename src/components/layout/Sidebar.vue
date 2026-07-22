<script setup lang="ts">
import { ChevronLeft, ChevronRight } from 'lucide-vue-next'
import { computed } from 'vue'
import AppCard from '../ui/AppCard.vue'
import AppIcon from '../ui/AppIcon.vue'
import type { SidebarSectionKey, SidebarTarget } from '../../types/navigation'
import { sectionsByNav } from './sidebarSections'

const props = defineProps<{
  target: SidebarTarget
  collapsed: boolean
}>()

const emit = defineEmits<{
  toggleCollapsed: []
  selectSection: [section: SidebarSectionKey]
}>()

const sections = computed(() => sectionsByNav[props.target.page] ?? [])
</script>

<template>
  <AppCard
    v-if="sections.length > 0"
    as="aside"
    class="sidebar ef-motion-sidebar"
    :class="{ collapsed: props.collapsed }"
    aria-label="二级导航"
    flush
  >
    <template #header>
      <div class="sidebar-head">
        <div v-if="!props.collapsed" class="sidebar-title">
          导航
        </div>
        <button
          class="collapse-btn ef-motion-hover ef-motion-press"
          type="button"
          :title="props.collapsed ? '展开' : '折叠'"
          @click="emit('toggleCollapsed')"
        >
          <AppIcon :icon="props.collapsed ? ChevronRight : ChevronLeft" :size="16" />
        </button>
      </div>
    </template>

    <div class="sidebar-body">
      <section v-for="section in sections" :key="section.title" class="section">
        <div v-if="!props.collapsed" class="section-title">
          {{ section.title }}
        </div>
        <button
          v-for="item in section.items"
          :key="item.key"
          class="item-btn ef-motion-hover"
          :class="{ active: props.target.section === item.key }"
          type="button"
          :title="item.label"
          @click="emit('selectSection', item.key)"
        >
          <span v-if="props.collapsed" class="dot" aria-hidden="true" />
          <span v-else class="label">{{ item.label }}</span>
        </button>
      </section>
    </div>
  </AppCard>
</template>

<style scoped>
.sidebar {
  width: 240px;
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.sidebar.collapsed {
  width: 64px;
}

.sidebar :deep(.ef-card-header) {
  padding: 8px 10px;
  text-transform: none;
  letter-spacing: normal;
}

.sidebar-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
}

.sidebar-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--muted-foreground);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.collapse-btn {
  background: transparent;
  border: 1px solid var(--border);
  color: var(--muted-foreground);
  border-radius: 8px;
  padding: 4px 8px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.collapse-btn:hover {
  border-color: var(--ring);
  color: var(--foreground);
}

.sidebar-body {
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.section-title {
  font-size: 11px;
  color: var(--muted-foreground);
  padding: 0 6px;
}

.item-btn {
  width: 100%;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--muted-foreground);
  cursor: pointer;
}

.item-btn:hover {
  background: var(--accent);
  color: var(--foreground);
}

.item-btn.active {
  background: var(--accent);
  border-color: var(--border);
  color: var(--foreground);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--border);
}

.label {
  font-size: 13px;
}
</style>
