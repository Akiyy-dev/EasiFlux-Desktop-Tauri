<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NConfigProvider, NMessageProvider, darkTheme } from 'naive-ui'
import AppShell from './components/layout/AppShell.vue'
import ErrorToastBridge from './components/common/ErrorToastBridge.vue'
import SettingsDialog from './components/settings/SettingsDialog.vue'
import { naiveThemeOverrides } from './constants/naiveTheme'
import { useTauriEvent, whenTauriListenersReady } from './composables/useTauriEvent'
import { useAccountSessionEvent } from './composables/useAccountSessionEvent'
import { useAppStore } from './stores/app'
import { useConfigStore } from './stores/config'
import { useConnectionStore } from './stores/connection'
import { useMarketStore } from './stores/market'
import { useOrderStore } from './stores/order'
import { usePositionStore } from './stores/position'
import { useAccountStore } from './stores/account'
import { useLogStore } from './stores/log'
import { useTimeStore } from './stores/time'
import { reportError as reportGlobalError } from './services/errorService'
import { onConnectionStatusChanged, onWebsocketStatusChanged } from './services/realtimeService'
import { onTimeUpdated } from './services/timeService'
import { applyPrivatePanelsSnapshot } from './stores/privatePanels'
import { normalizeAccountId } from './utils/account'
import { normalizeOrders } from './utils/order'
import { normalizePositions } from './utils/position'
import type {
  AccountSummary,
  Balance,
  DailyPnlSnapshot,
  Depth,
  EnvironmentStatus,
  Kline,
  LogEntry,
  Order,
  Position,
  PrivatePanelsSnapshot,
  Ticker,
  TimeSnapshot,
} from './types/models'

const appStore = useAppStore()
const configStore = useConfigStore()
const connectionStore = useConnectionStore()
const marketStore = useMarketStore()
const orderStore = useOrderStore()
const positionStore = usePositionStore()
const accountStore = useAccountStore()
const logStore = useLogStore()
const timeStore = useTimeStore()

const showSettings = ref(false)

function reportError(context: string, error: unknown): void {
  reportGlobalError(error, context)
}

useTauriEvent<string>('app:ready', (version) => {
  appStore.markReady(version)
})

useAccountSessionEvent<string>('connection:status', (status) => {
  connectionStore.setStatus(status)
  onConnectionStatusChanged(status)
})

useAccountSessionEvent<string>('websocket:status', (status) => {
  connectionStore.setWsStatus(status)
  onWebsocketStatusChanged(status)
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

useAccountSessionEvent<Order>('order:updated', (order) => {
  orderStore.upsertOrder(order)
})

useAccountSessionEvent<Position>('position:updated', (position) => {
  positionStore.upsertPosition(position)
})

useAccountSessionEvent<Balance>('balance:updated', (balance) => {
  accountStore.setBalance(balance)
})

useAccountSessionEvent<AccountSummary>('account:snapshot', (snapshot) => {
  accountStore.applySnapshot(snapshot)
})

useAccountSessionEvent<PrivatePanelsSnapshot>('private-panels:snapshot', (snapshot) => {
  applyPrivatePanelsSnapshot(snapshot)
  orderStore.setOpenOrders(normalizeOrders(snapshot.openOrders))
  orderStore.setOrderHistory(normalizeOrders(snapshot.orderHistory))
  positionStore.setPositions(normalizePositions(snapshot.positions))
})

useAccountSessionEvent<DailyPnlSnapshot>('daily-pnl:updated', (snapshot) => {
  accountStore.applyDailyPnlSnapshot(snapshot)
})

useAccountSessionEvent<EnvironmentStatus>('environment:updated', (status) => {
  appStore.applyEnvironment(status)
})

useTauriEvent<TimeSnapshot>('time:updated', (snapshot) => {
  onTimeUpdated(snapshot)
})

useTauriEvent<string>('error:occurred', (msg) => {
  reportGlobalError(msg)
})

useTauriEvent<LogEntry>('log:entry', (entry) => {
  logStore.addEntry(entry)
})

onMounted(async () => {
  await whenTauriListenersReady()
  timeStore.start()

  try {
    await appStore.initVersion()
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
    void appStore.refreshEnvironment(true).catch((error) => {
      reportError('环境检测失败', error)
    })
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
      <ErrorToastBridge />
      <AppShell @open-settings="showSettings = true" />
      <SettingsDialog v-model:show="showSettings" />
    </NMessageProvider>
  </NConfigProvider>
</template>
