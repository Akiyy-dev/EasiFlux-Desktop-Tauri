import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { LogEntry } from '../types/models'

const MAX_LOGS = 500

export const useLogStore = defineStore('log', () => {
  const entries = ref<LogEntry[]>([])
  const lastError = ref<string | null>(null)

  function addEntry(entry: LogEntry): void {
    entries.value.unshift(entry)
    if (entries.value.length > MAX_LOGS) {
      entries.value.length = MAX_LOGS
    }
  }

  function setError(message: string): void {
    lastError.value = message
    addEntry({ level: 'error', message, timestamp: Date.now() })
  }

  function clearError(): void {
    lastError.value = null
  }

  function clear(): void {
    entries.value = []
  }

  return { entries, lastError, addEntry, setError, clearError, clear }
})
