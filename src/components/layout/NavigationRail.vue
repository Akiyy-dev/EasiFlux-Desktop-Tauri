<script setup lang="ts">
export type NavKey =
  | 'home'
  | 'trading'
  | 'charts'
  | 'news'
  | 'account'
  | 'plugins'
  | 'settings'

const props = defineProps<{
  active: NavKey
}>()

const emit = defineEmits<{
  select: [key: NavKey]
  openSettings: []
}>()

const items: Array<{ key: Exclude<NavKey, 'settings'>; label: string; icon: string }> = [
  { key: 'home', label: '首页', icon: '⌂' },
  { key: 'trading', label: '交易', icon: '⇄' },
  { key: 'charts', label: '图表', icon: '▦' },
  { key: 'news', label: '新闻', icon: '📰' },
  { key: 'account', label: '账户', icon: '👤' },
  { key: 'plugins', label: '插件', icon: '⧉' },
]
</script>

<template>
  <nav class="rail panel" aria-label="一级导航">
    <div class="rail-top">
      <button
        v-for="item in items"
        :key="item.key"
        class="rail-btn"
        :class="{ active: props.active === item.key }"
        type="button"
        :title="item.label"
        @click="emit('select', item.key)"
      >
        <span class="rail-icon" aria-hidden="true">{{ item.icon }}</span>
      </button>
    </div>
    <div class="rail-bottom">
      <button
        class="rail-btn"
        :class="{ active: props.active === 'settings' }"
        type="button"
        title="设置"
        @click="emit('openSettings')"
      >
        <span class="rail-icon" aria-hidden="true">⚙</span>
      </button>
    </div>
  </nav>
</template>

<style scoped>
.rail {
  width: 56px;
  height: 100%;
  display: flex;
  flex-direction: column;
  padding: 6px;
  gap: 6px;
}

.rail-top {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.rail-bottom {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.rail-btn {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-secondary);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  transition:
    background var(--ef-duration-fast) var(--ef-ease-out),
    border-color var(--ef-duration-fast) var(--ef-ease-out),
    color var(--ef-duration-fast) var(--ef-ease-out);
}

.rail-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.rail-btn.active {
  background: var(--bg-tertiary);
  color: var(--text-primary);
  border-color: var(--border-color);
}

.rail-icon {
  font-size: 16px;
  line-height: 1;
}
</style>
