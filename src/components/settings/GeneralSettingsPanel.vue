<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NForm, NFormItem, NInputNumber, NSwitch } from 'naive-ui'
import { useGeneralSettingsAutosave } from '../../composables/useGeneralSettingsAutosave'
import { reportError } from '../../services/errorService'
import { useConfigStore } from '../../stores/config'
import { useConnectionStore } from '../../stores/connection'
import { AppButton } from '../ui'

const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const initializing = ref(true)
const initializationError = ref<string | null>(null)
const initialized = ref(false)
const useWebsocket = ref(false)
const tickerPollInterval = ref<number | null>(null)
const intervalError = ref<string | null>(null)
const visibleSaveError = ref<string | null>(null)

const autosave = useGeneralSettingsAutosave(
  (draft) => configStore.updateGeneralSettings(draft),
  async () => {
    try {
      await configStore.fetchConfig()
    } catch (error) {
      reportError(error, '通用设置对账失败')
      throw error
    }
  },
)

const appliedWebsocketMode = ref<boolean | null>(null)
const reconnectCandidate = ref<boolean | null>(null)
const websocketEditPending = ref(false)
const pendingWebsocketMode = ref<boolean | null>(null)
const reconnectRequired = computed(() => (
  pendingWebsocketMode.value !== null
  && (!websocketEditPending.value || autosave.status.value === 'error')
))
const controlsDisabled = computed(() => initializing.value || !initialized.value)
const autosaveStatusText = computed(() => {
  if (autosave.status.value === 'saving') return '正在保存'
  if (autosave.status.value === 'saved') return '已保存'
  return null
})

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  return '通用设置加载失败'
}

async function initialize(): Promise<void> {
  if (initializing.value && initialized.value) return
  initializing.value = true
  initializationError.value = null
  try {
    const config = configStore.config ?? await configStore.fetchConfig()
    const settings = {
      useWebsocket: config.useWebsocket,
      tickerPollInterval: config.tickerPollInterval,
    }
    autosave.initialize(settings)
    useWebsocket.value = settings.useWebsocket
    tickerPollInterval.value = settings.tickerPollInterval
    intervalError.value = null
    visibleSaveError.value = null
    appliedWebsocketMode.value = settings.useWebsocket
    reconnectCandidate.value = null
    websocketEditPending.value = false
    pendingWebsocketMode.value = null
    initialized.value = true
  } catch (error) {
    initialized.value = false
    initializationError.value = errorText(error)
  } finally {
    initializing.value = false
  }
}

function updateWebsocket(value: boolean): void {
  if (!initialized.value) return
  useWebsocket.value = value
  websocketEditPending.value = true
  reconnectCandidate.value = connectionStore.connected ? value : null
  autosave.update({ useWebsocket: value })
}

function updateTickerPollInterval(value: number | null): void {
  if (!initialized.value) return
  tickerPollInterval.value = value
  if (
    typeof value !== 'number'
    || !Number.isFinite(value)
    || value < 1
    || value > 3600
  ) {
    intervalError.value = '行情轮询间隔必须是 1 到 3600 秒之间的有限数值'
    return
  }
  intervalError.value = null
  autosave.update({ tickerPollInterval: value })
}

async function retrySave(): Promise<void> {
  await autosave.retry()
}

async function reconnectNow(): Promise<void> {
  const mode = pendingWebsocketMode.value
  if (mode === null) return
  try {
    await connectionStore.reconnect(mode)
    appliedWebsocketMode.value = mode
    pendingWebsocketMode.value = null
  } catch {
    // The store owns reconnectError. Keep the committed pending mode retryable.
  }
}

watch(
  () => [autosave.status.value, autosave.error.value] as const,
  ([status, saveError]) => {
    if (status === 'error' && saveError) visibleSaveError.value = saveError
    if (status === 'saved') visibleSaveError.value = null
  },
  { flush: 'sync' },
)

watch(
  () => [
    autosave.status.value,
    autosave.lastSaved.value?.useWebsocket,
  ] as const,
  ([status, savedMode]) => {
    if (status !== 'saved' || !websocketEditPending.value) return
    const candidate = reconnectCandidate.value
    pendingWebsocketMode.value = connectionStore.connected
      && candidate !== null
      && savedMode === candidate
      && candidate !== appliedWebsocketMode.value
      ? candidate
      : null
    websocketEditPending.value = false
    reconnectCandidate.value = null
  },
  { flush: 'sync' },
)

watch(
  () => connectionStore.connected,
  (connected, wasConnected) => {
    if (connected && !wasConnected) {
      appliedWebsocketMode.value = autosave.lastSaved.value?.useWebsocket ?? null
      pendingWebsocketMode.value = null
      reconnectCandidate.value = websocketEditPending.value
        ? autosave.draft.value?.useWebsocket ?? null
        : null
      return
    }
    if (!connected && wasConnected && !connectionStore.reconnecting) {
      pendingWebsocketMode.value = null
      reconnectCandidate.value = null
    }
  },
)

onMounted(() => { void initialize() })
onBeforeUnmount(() => {
  void autosave.dispose().catch((error) => {
    reportError(error, '离开通用设置前保存失败')
  })
})
</script>

<template>
  <section class="general-settings-panel" aria-labelledby="general-settings-title">
    <header class="general-settings-header">
      <div>
        <h2 id="general-settings-title">
          通用设置
        </h2>
        <p>
          调整实时行情模式与轮询频率。更改会自动保存。
        </p>
      </div>
      <span
        v-if="autosaveStatusText"
        class="general-settings-status"
        role="status"
        aria-live="polite"
      >
        {{ autosaveStatusText }}
      </span>
    </header>

    <p v-if="initializing" class="general-settings-muted" role="status" aria-live="polite">
      正在加载通用设置
    </p>
    <div v-else-if="initializationError" class="general-settings-error-row">
      <p role="alert">
        {{ initializationError }}
      </p>
      <AppButton
        data-testid="general-settings-retry"
        aria-label="重试加载通用设置"
        @click="initialize"
      >
        重试加载
      </AppButton>
    </div>

    <NForm class="general-settings-form" label-placement="top">
      <NFormItem label="WebSocket 实时更新">
        <NSwitch
          :value="useWebsocket"
          :disabled="controlsDisabled"
          aria-label="WebSocket 实时更新"
          @update:value="updateWebsocket"
        />
      </NFormItem>
      <NFormItem label="行情轮询间隔（秒）">
        <NInputNumber
          :value="tickerPollInterval"
          :disabled="controlsDisabled"
          :min="1"
          :max="3600"
          :step="1"
          aria-label="行情轮询间隔（秒）"
          style="width: 100%"
          @update:value="updateTickerPollInterval"
        />
      </NFormItem>
    </NForm>

    <p v-if="intervalError" class="general-settings-error" role="alert">
      {{ intervalError }}
    </p>

    <div v-if="visibleSaveError" class="general-settings-error-row">
      <p data-testid="general-settings-save-error" role="alert">
        保存失败：{{ visibleSaveError }}
      </p>
      <AppButton
        data-testid="general-settings-save-retry"
        aria-label="重试保存通用设置"
        :loading="autosave.status.value === 'saving'"
        @click="retrySave"
      >
        重试保存
      </AppButton>
    </div>

    <div v-if="reconnectRequired" class="general-settings-reconnect">
      <p>
        WebSocket 模式已保存，重连后生效。
      </p>
      <AppButton
        data-testid="general-reconnect"
        aria-label="立即重连并应用 WebSocket 设置"
        :loading="connectionStore.reconnecting"
        @click="reconnectNow"
      >
        立即重连
      </AppButton>
    </div>

    <p
      v-if="connectionStore.reconnectError && pendingWebsocketMode !== null"
      class="general-settings-error"
      data-testid="general-reconnect-error"
      role="alert"
    >
      重连失败：{{ connectionStore.reconnectError }}
    </p>
  </section>
</template>

<style scoped src="./GeneralSettingsPanel.css"></style>
