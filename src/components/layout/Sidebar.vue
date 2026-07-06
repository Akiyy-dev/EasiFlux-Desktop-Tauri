<script setup lang="ts">
import type { NavKey } from './NavigationRail.vue'

const props = defineProps<{
  active: NavKey
  collapsed: boolean
}>()

const emit = defineEmits<{
  toggleCollapsed: []
}>()

type SidebarSection = {
  title: string
  items: Array<{ key: string; label: string }>
}

const sectionsByNav: Partial<Record<NavKey, SidebarSection[]>> = {
  home: [
    {
      title: 'EasiFlux',
      items: [
        { key: 'welcome', label: '欢迎页' },
        { key: 'updates', label: '最近更新' },
      ],
    },
  ],
  plugins: [
    {
      title: '插件',
      items: [
        { key: 'installed', label: '已安装插件' },
        { key: 'market', label: '插件市场' },
        { key: 'manage', label: '插件管理' },
      ],
    },
  ],
  account: [
    {
      title: '账户',
      items: [
        { key: 'api', label: 'API 管理' },
        { key: 'assets', label: '资产总览' },
        { key: 'risk', label: '风险控制' },
      ],
    },
  ],
}

const sections = sectionsByNav[props.active] ?? []
</script>

<template>
  <aside
    v-if="sections.length > 0"
    class="sidebar panel"
    :class="{ collapsed: props.collapsed }"
    aria-label="二级导航"
  >
    <div class="sidebar-head">
      <div v-if="!props.collapsed" class="sidebar-title">导航</div>
      <button
        class="collapse-btn"
        type="button"
        :title="props.collapsed ? '展开' : '折叠'"
        @click="emit('toggleCollapsed')"
      >
        {{ props.collapsed ? '»' : '«' }}
      </button>
    </div>

    <div class="sidebar-body">
      <section v-for="section in sections" :key="section.title" class="section">
        <div v-if="!props.collapsed" class="section-title">{{ section.title }}</div>
        <button
          v-for="item in section.items"
          :key="item.key"
          class="item-btn"
          type="button"
          :title="item.label"
        >
          <span v-if="props.collapsed" class="dot" aria-hidden="true" />
          <span v-else class="label">{{ item.label }}</span>
        </button>
      </section>
    </div>
  </aside>
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

.sidebar-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border-color);
}

.sidebar-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.collapse-btn {
  background: transparent;
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
  border-radius: 8px;
  padding: 4px 8px;
  cursor: pointer;
}

.collapse-btn:hover {
  border-color: var(--accent-blue);
  color: var(--text-primary);
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
  color: var(--text-secondary);
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
  color: var(--text-secondary);
  cursor: pointer;
  transition:
    background var(--ef-duration-fast) var(--ef-ease-out),
    color var(--ef-duration-fast) var(--ef-ease-out);
}

.item-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--border-color);
}

.label {
  font-size: 13px;
}
</style>
