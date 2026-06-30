import { onUnmounted } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

const pendingListeners: Promise<void>[] = []

export function whenTauriListenersReady(): Promise<void> {
  return Promise.all(pendingListeners).then(() => undefined)
}

export function useTauriEvent<T>(
  event: string,
  handler: (payload: T) => void,
): void {
  let unlisten: UnlistenFn | undefined

  pendingListeners.push(
    listen<T>(event, (e) => {
      handler(e.payload)
    }).then((fn) => {
      unlisten = fn
    }),
  )

  onUnmounted(() => {
    unlisten?.()
  })
}
