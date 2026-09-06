<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import PluginCard from './PluginCard.vue'
import { usePluginStore, type PluginStatusFilter } from '../../stores/plugin'
import type { PluginSection } from '../../types/navigation'

const props = defineProps<{
  section: PluginSection
}>()

const store = usePluginStore()
const title = ref<{ focus: () => void } | null>(null)

const sectionCopy: Record<PluginSection, { title: string; description: string }> = {
  installed: {
    title: '已安装插件',
    description: '查看随应用提供的可信插件，并管理它们的启用状态。',
  },
  market: {
    title: '插件市场',
    description: '浏览当前受信任的内置目录。阶段 0 不提供在线安装。',
  },
  manage: {
    title: '插件管理',
    description: '汇总插件状态，并检查请求权限与实际授予权限。',
  },
}

const activeCopy = computed(() => sectionCopy[props.section])
const initialLoading = computed(() => (
  store.availability === null
  && (store.loadStatus === 'idle' || store.loadStatus === 'loading')
))
const initialError = computed(() => (
  store.availability === null && store.loadStatus === 'error'
))
const refreshing = computed(() => (
  store.availability !== null && store.loadStatus === 'loading'
))
const refreshError = computed(() => (
  store.availability !== null && store.loadError !== null
))
const builtInPlugins = computed(() => (
  store.catalog.filter((plugin) => plugin.source === 'builtIn')
))
const enabledCount = computed(() => (
  store.catalog.filter((plugin) => plugin.status === 'enabled').length
))
const disabledCount = computed(() => (
  store.catalog.filter((plugin) => plugin.status === 'disabled').length
))
const blockedCount = computed(() => (
  store.catalog.filter((plugin) => plugin.status === 'blocked').length
))
const availabilityMessage = computed(() => {
  if (store.availabilityReasonCode === 'catalogInvalid') {
    return '插件目录校验失败，插件启停已暂停。'
  }
  return '插件状态子系统暂时不可用，插件启停已暂停。'
})

function eventValue(event: unknown): string | null {
  const value = (event as { target?: { value?: unknown } }).target?.value
  return typeof value === 'string' ? value : null
}

function updateQuery(event: unknown): void {
  const value = eventValue(event)
  if (value !== null) store.setQuery(value)
}

function updateStatusFilter(event: unknown): void {
  const value = eventValue(event)
  if (value === 'all' || value === 'enabled' || value === 'disabled' || value === 'blocked') {
    store.setStatusFilter(value satisfies PluginStatusFilter)
  }
}

function retry(): void {
  void store.retry()
}

function togglePlugin(id: string, enabled: boolean): void {
  void store.setEnabled(id, enabled)
}

onMounted(() => {
  void store.load()
  void nextTick(() => title.value?.focus())
})
</script>

<template>
  <section class="plugin-marketplace-page" :data-section="props.section">
    <header class="plugin-marketplace-page__header">
      <p class="plugin-marketplace-page__eyebrow">
        插件系统 · 安全基础版
      </p>
      <h1 ref="title" tabindex="-1">
        {{ activeCopy.title }}
      </h1>
      <p>{{ activeCopy.description }}</p>
    </header>

    <div
      v-if="initialLoading"
      class="plugin-marketplace-page__notice"
      role="status"
      aria-live="polite"
    >
      正在加载插件目录…
    </div>

    <div
      v-else-if="initialError"
      class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
      data-testid="plugin-load-error"
      role="alert"
    >
      <p>{{ store.loadError }}</p>
      <button class="ef-btn ef-btn-secondary ef-btn-sm" type="button" @click="retry">
        重试
      </button>
    </div>

    <template v-else>
      <div
        v-if="refreshing"
        class="plugin-marketplace-page__notice"
        role="status"
        aria-live="polite"
      >
        正在刷新插件目录…
      </div>

      <div
        v-if="refreshError"
        class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
        data-testid="plugin-refresh-error"
        role="alert"
      >
        <p>{{ store.loadError }}</p>
        <button class="ef-btn ef-btn-secondary ef-btn-sm" type="button" @click="retry">
          重试
        </button>
      </div>

      <p
        v-if="store.availability === 'unavailable'"
        class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
        data-testid="plugin-availability-alert"
        role="alert"
      >
        {{ availabilityMessage }}
      </p>

      <section v-if="props.section === 'installed'" class="plugin-marketplace-page__section">
        <div class="plugin-marketplace-page__controls">
          <label for="plugin-search">
            <span>搜索已安装插件</span>
            <input
              id="plugin-search"
              type="search"
              :value="store.query"
              placeholder="名称、ID、发布者或描述"
              @input="updateQuery"
            >
          </label>
          <label for="plugin-status-filter">
            <span>按状态筛选</span>
            <select
              id="plugin-status-filter"
              :value="store.statusFilter"
              @change="updateStatusFilter"
            >
              <option value="all">全部状态</option>
              <option value="enabled">已启用</option>
              <option value="disabled">已停用</option>
              <option value="blocked">已阻止</option>
            </select>
          </label>
        </div>

        <p
          v-if="store.catalog.length === 0"
          class="plugin-marketplace-page__empty"
          data-testid="plugin-catalog-empty"
        >
          当前没有已安装插件。内置目录为空是正常状态。
        </p>
        <p
          v-else-if="store.visiblePlugins.length === 0"
          class="plugin-marketplace-page__empty"
          data-testid="plugin-no-match"
        >
          没有符合当前条件的插件，请调整搜索或状态筛选。
        </p>
        <div v-else class="plugin-marketplace-page__grid">
          <PluginCard
            v-for="plugin in store.visiblePlugins"
            :key="plugin.manifest.id"
            :plugin="plugin"
            :pending="store.pendingIds.has(plugin.manifest.id)"
            :error="store.actionErrors[plugin.manifest.id] ?? null"
            @toggle="togglePlugin"
          />
        </div>
      </section>

      <section v-else-if="props.section === 'market'" class="plugin-marketplace-page__section">
        <p v-if="builtInPlugins.length === 0" class="plugin-marketplace-page__empty">
          可信插件目录当前为空。在线市场与安装流程将在后续阶段提供。
        </p>
        <div v-else class="plugin-marketplace-page__grid">
          <article
            v-for="plugin in builtInPlugins"
            :key="plugin.manifest.id"
            class="plugin-market-entry ef-card"
          >
            <header>
              <h2>{{ plugin.manifest.name }}</h2>
              <span>v{{ plugin.manifest.version }}</span>
            </header>
            <p>{{ plugin.manifest.description }}</p>
            <dl>
              <div>
                <dt>发布者</dt>
                <dd>{{ plugin.manifest.publisher }}（{{ plugin.manifest.publisherId }}）</dd>
              </div>
              <div>
                <dt>来源</dt>
                <dd>内置 · 随应用提供</dd>
              </div>
            </dl>
            <p class="plugin-market-entry__note">
              已随 EasiFlux 提供，无需下载或安装。
            </p>
          </article>
        </div>
      </section>

      <section v-else class="plugin-marketplace-page__section">
        <div class="plugin-management-summary" aria-label="插件状态汇总">
          <div data-testid="enabled-count">
            <strong>{{ enabledCount }}</strong>
            <span>已启用</span>
          </div>
          <div data-testid="disabled-count">
            <strong>{{ disabledCount }}</strong>
            <span>已停用</span>
          </div>
          <div data-testid="blocked-count">
            <strong>{{ blockedCount }}</strong>
            <span>已阻止</span>
          </div>
        </div>

        <p v-if="store.catalog.length === 0" class="plugin-marketplace-page__empty">
          当前没有可管理的插件。
        </p>
        <div v-else class="plugin-management-list">
          <article
            v-for="plugin in store.catalog"
            :key="plugin.manifest.id"
            class="plugin-management-item ef-card"
            data-testid="plugin-management-item"
          >
            <header>
              <h2>{{ plugin.manifest.name }}</h2>
              <span>{{ plugin.manifest.id }}</span>
            </header>
            <p>当前状态：{{ plugin.status === 'enabled' ? '已启用' : plugin.status === 'disabled' ? '已停用' : '已阻止' }}</p>
            <p data-testid="management-requested-capabilities">
              <strong>请求权限：</strong>无需额外权限
            </p>
            <p data-testid="management-granted-capabilities">
              <strong>已授予权限：</strong>无需额外权限
            </p>
          </article>
        </div>
      </section>
    </template>
  </section>
</template>

<style src="./PluginMarketplacePage.css"></style>
