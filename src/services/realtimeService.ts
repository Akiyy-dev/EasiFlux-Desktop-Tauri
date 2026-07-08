import { refreshAllSyncTasks } from './dataSyncService'

let lastConnection = 'disconnected'
let lastWs = 'disconnected'
let syncInFlight = false

async function syncAfterRecover(): Promise<void> {
  if (syncInFlight) {
    return
  }
  syncInFlight = true
  try {
    await refreshAllSyncTasks()
  } finally {
    syncInFlight = false
  }
}

export function onConnectionStatusChanged(next: string): void {
  const recovered = lastConnection !== 'connected' && next === 'connected'
  lastConnection = next
  if (recovered) {
    void syncAfterRecover()
  }
}

export function onWebsocketStatusChanged(next: string): void {
  const recovered = lastWs !== 'connected' && next === 'connected'
  lastWs = next
  if (recovered) {
    void syncAfterRecover()
  }
}
