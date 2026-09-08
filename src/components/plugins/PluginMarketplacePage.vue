<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import PluginCard from './PluginCard.vue'
import PluginImportDialog from './PluginImportDialog.vue'
import { pluginSourceLabel } from './pluginPresentation'
import { usePluginStore, type PluginStatusFilter } from '../../stores/plugin'
import type { LocalManifestImportCommitFailure } from '../../types/plugin'
import type { PluginSection } from '../../types/navigation'

const props = defineProps<{
  section: PluginSection
}>()

interface FocusControl {
  readonly isConnected: boolean
  focus: () => void
}

const store = usePluginStore()
const title = ref<{ focus: () => void } | null>(null)
const importTrigger = ref<FocusControl | null>(null)
const importDialogOpener = ref<FocusControl | null>(null)

const importFailureCopy: Record<LocalManifestImportCommitFailure, string> = {
  plugin_catalog_stale: '插件目录已更新，请刷新后重试。',
  plugin_catalog_invalid: '插件目录不可用，请稍后重试。',
  plugin_catalog_generation_exhausted: '插件目录版本已达到上限，请联系支持。',
  plugin_state_unavailable: '插件状态暂不可用，请重试。',
  plugin_state_persist_failed: '保存插件状态失败，请重试。',
  plugin_state_capacity_exceeded: '插件状态容量已达到上限，请联系支持。',
  plugin_revision_exhausted: '插件状态版本已达到上限，请联系支持。',
  plugin_import_id_conflict: '已存在相同插件 ID；当前不支持覆盖或更新。',
  plugin_import_discovery_unavailable: '请先修复本地插件发现问题，再导入清单。',
  plugin_import_capacity_exceeded: '本地插件数量或读取预算已达上限。',
  plugin_import_staging_capacity_exceeded: '导入暂存区需要人工检查和清理。',
  plugin_import_write_failed: '无法完成清单写入，请检查后重试。',
}

const sectionCopy: Record<PluginSection, { title: string; description: string }> = {
  installed: {
    title: '已安装插件',
    description: '查看内置插件与已发现的本地声明式包，并管理宿主启用偏好。',
  },
  market: {
    title: '插件市场',
    description: '浏览随应用提供的内置目录。当前版本不提供在线安装。',
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
const refreshing = computed(() => (
  store.availability !== null && store.loadStatus === 'loading'
))
const reloading = computed(() => store.reloadStatus === 'loading')
const importEntryDisabled = computed(() => store.importStatus !== 'idle')
const importedMessage = computed(() => (
  store.importStatus === 'result' && store.importResult?.status === 'imported'
    ? '清单已导入，默认停用。'
    : null
))
const importAlertMessage = computed(() => {
  if (store.importStatus !== 'result') return null
  if (store.importOutcomeUnknown) {
    return store.importError ?? '结果尚未确认，请重新扫描。'
  }
  if (store.importError) return store.importError
  const result = store.importResult
  if (result?.status === 'notImported') {
    if (result.disabledDecisionSaved) {
      return '导入未完成；该清单的停用偏好已保存，旧启用不会恢复'
    }
    return importFailureCopy[result.reasonCode]
  }
  if (result?.status === 'importedNotVisible') {
    return '清单已写入，但目录结果尚未确认，请重新扫描。'
  }
  return null
})
const importNeedsReload = computed(() => (
  store.importOutcomeUnknown || store.importResult?.status === 'importedNotVisible'
))
const reloadDisabled = computed(() => (
  initialLoading.value || store.loadStatus === 'loading' || reloading.value
))
// One transport failure surface: a reload failure takes precedence because only
// a successful reload clears both error domains. Validated health stays separate.
const transportError = computed(() => {
  if (reloadDisabled.value) return null
  if (store.reloadError) {
    return { message: store.reloadError, testId: 'plugin-reload-error' }
  }
  if (store.loadError) {
    return {
      message: store.loadError,
      testId: store.availability === null ? 'plugin-load-error' : 'plugin-refresh-error',
    }
  }
  return null
})
const recoveryLabel = computed(() => store.reloadError ? '重新扫描并重试' : '重试')
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
const localDiscoveryMessage = computed(() => store.availability === 'available'
  ? '本地插件发现暂时不可用，内置插件仍可使用。请重新扫描本地插件。'
  : '本地插件发现暂时不可用，请重新扫描本地插件。')

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

function recover(): void {
  if (reloadDisabled.value) return
  if (store.reloadError) void store.reload()
  else void store.retry()
}

function reload(): void {
  if (!reloadDisabled.value) void store.reload()
}

function prepareImport(): void {
  if (props.section === 'market' || importEntryDisabled.value) return
  importDialogOpener.value = importTrigger.value?.isConnected ? importTrigger.value : null
  void store.prepareImport()
}

function cancelImport(): void {
  void store.cancelImport()
}

function commitImport(): void {
  void store.commitImport()
}

function clearImportResult(): void {
  store.clearImportResult()
}

function reloadImportResult(): void {
  if (!reloadDisabled.value) void store.reload()
}

function togglePlugin(id: string, enabled: boolean): void {
  void store.setEnabled(id, enabled)
}

onMounted(() => {
  void store.load()
  void nextTick(() => title.value?.focus())
})

onBeforeUnmount(() => {
  store.releaseImportView()
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
      <p class="plugin-marketplace-page__description">
        {{ activeCopy.description }}
      </p>
      <div class="plugin-marketplace-page__actions">
        <button
          class="ef-btn ef-btn-secondary ef-btn-sm"
          type="button"
          :disabled="reloadDisabled"
          @click="reload"
        >
          重新扫描本地插件
        </button>
        <button
          v-if="props.section !== 'market'"
          ref="importTrigger"
          class="ef-btn ef-btn-primary ef-btn-sm"
          data-testid="plugin-import-button"
          type="button"
          :disabled="importEntryDisabled"
          @click="prepareImport"
        >
          导入本地清单
        </button>
      </div>
    </header>

    <PluginImportDialog
      v-if="store.importPreview && (
        store.importStatus === 'preview' || store.importStatus === 'committing'
      )"
      :preview="store.importPreview"
      :committing="store.importStatus === 'committing'"
      :stale="store.importPreviewStale"
      :opener="importDialogOpener"
      @confirm="commitImport"
      @cancel="cancelImport"
    />

    <div
      v-if="store.importStatus === 'choosing'"
      class="plugin-marketplace-page__notice"
      data-testid="plugin-import-choosing"
      role="status"
      aria-live="polite"
    >
      正在选择插件清单…
    </div>
    <div
      v-if="importedMessage"
      class="plugin-marketplace-page__notice plugin-marketplace-page__notice--success"
      data-testid="plugin-import-status"
      role="status"
      aria-live="polite"
    >
      <p>{{ importedMessage }}</p>
      <button
        class="ef-btn ef-btn-secondary ef-btn-sm"
        data-testid="plugin-import-dismiss"
        type="button"
        @click="clearImportResult"
      >
        关闭提示
      </button>
    </div>
    <div
      v-else-if="importAlertMessage"
      class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
      data-testid="plugin-import-alert"
      role="alert"
    >
      <p>{{ importAlertMessage }}</p>
      <div class="plugin-marketplace-page__notice-actions">
        <button
          v-if="importNeedsReload"
          class="ef-btn ef-btn-secondary ef-btn-sm"
          data-testid="plugin-import-result-reload"
          type="button"
          :disabled="reloadDisabled"
          @click="reloadImportResult"
        >
          重新扫描本地插件
        </button>
        <button
          class="ef-btn ef-btn-secondary ef-btn-sm"
          data-testid="plugin-import-dismiss"
          type="button"
          @click="clearImportResult"
        >
          关闭提示
        </button>
      </div>
    </div>

    <div
      v-if="reloading"
      class="plugin-marketplace-page__notice"
      data-testid="plugin-reload-status"
      role="status"
      aria-live="polite"
    >
      正在重新扫描本地插件…
    </div>
    <div
      v-if="transportError"
      class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
      :data-testid="transportError.testId"
      role="alert"
    >
      <p>{{ transportError.message }}</p>
      <button
        class="ef-btn ef-btn-secondary ef-btn-sm"
        type="button"
        :disabled="reloadDisabled"
        @click="recover"
      >
        {{ recoveryLabel }}
      </button>
    </div>

    <div
      v-if="initialLoading"
      class="plugin-marketplace-page__notice"
      role="status"
      aria-live="polite"
    >
      正在加载插件目录…
    </div>

    <template v-if="store.availability !== null">
      <div
        v-if="store.localDiscovery?.status === 'degraded'"
        class="plugin-marketplace-page__notice"
        data-testid="plugin-local-discovery-status"
        role="status"
        aria-live="polite"
      >
        本地插件扫描已完成，已拒绝 {{ store.localDiscovery.rejectedPackageCount }} 个包。
      </div>
      <div
        v-else-if="store.localDiscovery?.status === 'unavailable'"
        class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
        data-testid="plugin-local-discovery-alert"
        role="alert"
      >
        {{ localDiscoveryMessage }}
      </div>

      <div
        v-if="refreshing"
        class="plugin-marketplace-page__notice"
        role="status"
        aria-live="polite"
      >
        正在刷新插件目录…
      </div>

      <div
        v-if="store.availability === 'unavailable'"
        class="plugin-marketplace-page__notice plugin-marketplace-page__notice--error"
        data-testid="plugin-availability-alert"
        role="alert"
      >
        <p>{{ availabilityMessage }}</p>
        <button
          class="ef-btn ef-btn-secondary ef-btn-sm"
          type="button"
          :disabled="reloadDisabled"
          @click="recover"
        >
          {{ store.reloadError ? '重新扫描并重试' : '重试插件子系统' }}
        </button>
      </div>

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
          当前没有已安装插件，也没有发现可用的本地声明式包。
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
          内置插件目录当前为空。本地声明式包可在已安装插件与插件管理中查看。
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
                <dd>{{ pluginSourceLabel(plugin.source) }}</dd>
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
            <p>来源：{{ pluginSourceLabel(plugin.source) }}</p>
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
