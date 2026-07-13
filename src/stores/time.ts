import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  getTimeSnapshot,
  serverNowMs,
  startTimeService,
  stopTimeService,
  subscribeTimeSnapshot,
  syncTimeNow,
} from '../services/timeService'
import type { TimeSnapshot } from '../types/models'

export const useTimeStore = defineStore('time', () => {
  const snapshot = ref<TimeSnapshot>(getTimeSnapshot())
  const serverNow = ref(serverNowMs())
  let unsubscribe: (() => void) | null = null
  let tickTimer: ReturnType<typeof globalThis.setInterval> | null = null

  function start(): void {
    if (unsubscribe) {
      return
    }
    startTimeService()
    unsubscribe = subscribeTimeSnapshot((next) => {
      snapshot.value = next
      serverNow.value = serverNowMs()
    })
    tickTimer = globalThis.setInterval(() => {
      serverNow.value = serverNowMs()
    }, 1000)
  }

  function stop(): void {
    unsubscribe?.()
    unsubscribe = null
    if (tickTimer) {
      globalThis.clearInterval(tickTimer)
      tickTimer = null
    }
    stopTimeService()
  }

  const offsetMs = computed(() => snapshot.value.offsetMs)
  const syncStatus = computed(() => snapshot.value.syncStatus)
  const source = computed(() => snapshot.value.source)

  return {
    snapshot,
    serverNow,
    offsetMs,
    syncStatus,
    source,
    start,
    stop,
    syncNow: syncTimeNow,
  }
})
