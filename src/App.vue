<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NConfigProvider, NMessageProvider, darkTheme } from 'naive-ui'
import AppShell from './components/layout/AppShell.vue'
import SettingsDialog from './components/settings/SettingsDialog.vue'
import { useTauriEvent } from './composables/useTauriEvent'
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useConnectionStore } from './stores/connection'
import { useMarketStore } from './stores/market'
import { useOrderStore } from './stores/order'
import { usePositionStore } from './stores/position'
import { useAccountStore } from './stores/account'
import { useLogStore } from './stores/log'
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

useTauriEvent<string>('app:ready', (version) => {
  appStore.markReady(version)
})

useTauriEvent<string>('connection:status', (status) => {
  connectionStore.setStatus(status)
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

useTauriEvent<string>('error:occurred', (message) => {
  logStore.setError(message)
})

useTauriEvent<LogEntry>('log:entry', (entry) => {
  logStore.addEntry(entry)
})

onMounted(async () => {
  await appStore.ping()
  await configStore.fetchConfig()
  if (configStore.config) {
    marketStore.activeSymbol = configStore.config.activeSymbol
    marketStore.klineInterval = configStore.config.klineInterval
  }
  const hasCreds = await configStore.hasCredentials(
    configStore.config?.activeAccountId ?? 'default',
  )
  if (hasCreds) {
    try {
      await connectionStore.connect(configStore.config?.useWebsocket ?? true)
    } catch {
      showSettings.value = true
    }
  } else {
    showSettings.value = true
  }
})
</script>

<template>
  <NConfigProvider :theme="darkTheme">
    <NMessageProvider>
      <AppShell :version="appStore.version" @open-settings="showSettings = true" />
      <SettingsDialog v-model:show="showSettings" />
    </NMessageProvider>
  </NConfigProvider>
</template>
