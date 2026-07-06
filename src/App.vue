<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NConfigProvider, NMessageProvider, darkTheme } from 'naive-ui'
import AppShell from './components/layout/AppShell.vue'
import SettingsDialog from './components/settings/SettingsDialog.vue'
import { naiveThemeOverrides } from './constants/naiveTheme'
import { useTauriEvent, whenTauriListenersReady } from './composables/useTauriEvent'
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useConnectionStore } from './stores/connection'
import { useMarketStore } from './stores/market'
import { useOrderStore } from './stores/order'
import { usePositionStore } from './stores/position'
import { useAccountStore } from './stores/account'
import { useLogStore } from './stores/log'
import { normalizeAccountId } from './utils/account'
import type { Balance, Depth, Kline, LogEntry, Order, Position, Ticker } from './types/models'

const appStore = useAppStore()
const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const marketStore = useMarketStore()
const orderStore = useOrderStore()
const positionStore = usePositionStore()
const accountStore = useAccountStore()
const logStore = useLogStore()

const showSettings = ref(false)

function reportError(context: string, error: unknown): void {
  const text =
    typeof error === 'string'
      ? error
      : error instanceof Error
        ? error.message
        : '未知错误'
  logStore.setError(`${context}: ${text}`)
}

useTauriEvent<string>('app:ready', (version) => {
  appStore.markReady(version)
})

useTauriEvent<string>('connection:status', (status) => {
  connectionStore.setStatus(status)
})

useTauriEvent<string>('websocket:status', (status) => {
  connectionStore.setWsStatus(status)
})

useTauriEvent<Ticker>('market:ticker', (ticker) => {
  marketStore.setTicker(ticker)
})

useTauriEvent<Depth>('market:depth', (depth) => {
  marketStore.setDepth(depth)
})

useTauriEvent<Kline[]>('market:kline', (klines) => {
  marketStore.setKlines(klines)
})

useTauriEvent<Order>('order:updated', (order) => {
  orderStore.upsertOrder(order)
})

useTauriEvent<Position>('position:updated', (position) => {
  positionStore.upsertPosition(position)
})

useTauriEvent<Balance>('balance:updated', (balance) => {
  accountStore.setBalance(balance)
})

useTauriEvent<string>('error:occurred', (msg) => {
  logStore.setError(msg)
})

useTauriEvent<LogEntry>('log:entry', (entry) => {
  logStore.addEntry(entry)
})

onMounted(async () => {
  await whenTauriListenersReady()

  try {
    await appStore.ping()
  } catch (error) {
    reportError('启动检查失败', error)
  }

  try {
    await configStore.fetchConfig()
  } catch (error) {
    reportError('加载配置失败', error)
    showSettings.value = true
    return
  }

  if (configStore.config) {
    marketStore.activeSymbol = configStore.config.activeSymbol
    marketStore.klineInterval = configStore.config.klineInterval
    void marketStore.loadInstruments(configStore.config.watchlistSymbols)
  }

  const hasCreds = await configStore.hasCredentials(
    normalizeAccountId(configStore.config?.activeAccountId),
  )
  if (hasCreds) {
    try {
      await connectionStore.connect(configStore.config?.useWebsocket ?? true)
    } catch (error) {
      reportError('自动连接失败', error)
      showSettings.value = true
    }
  } else {
    showSettings.value = true
  }
})
</script>

<template>
  <NConfigProvider :theme="darkTheme" :theme-overrides="naiveThemeOverrides">
    <NMessageProvider>
      <AppShell :version="appStore.version" @open-settings="showSettings = true" />
      <SettingsDialog v-model:show="showSettings" />
    </NMessageProvider>
  </NConfigProvider>
</template>
