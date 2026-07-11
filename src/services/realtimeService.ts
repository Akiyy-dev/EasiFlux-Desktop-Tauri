let lastConnection = 'disconnected'
let lastWs = 'disconnected'

export function onConnectionStatusChanged(next: string): void {
  lastConnection = next
}

export function onWebsocketStatusChanged(next: string): void {
  lastWs = next
}

export function getLastRealtimeStatuses(): { connection: string; ws: string } {
  return { connection: lastConnection, ws: lastWs }
}
