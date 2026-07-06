import { onMounted, ref, toValue, watch, type MaybeRef } from 'vue'
import { storeToRefs } from 'pinia'
import { useConnectionStore } from '../stores/connection'
import { useLogStore } from '../stores/log'
import { refreshPrivatePanels } from '../stores/privatePanels'

export function useOrderCenterRefresh(active: MaybeRef<boolean>) {
  const connectionStore = useConnectionStore()
  const logStore = useLogStore()
  const { connected } = storeToRefs(connectionStore)
  const loading = ref(false)

  async function refresh(): Promise<void> {
    if (!connectionStore.connected) {
      return
    }
    loading.value = true
    try {
      await refreshPrivatePanels()
    } catch (error) {
      logStore.setError(error instanceof Error ? error.message : String(error))
    } finally {
      loading.value = false
    }
  }

  onMounted(() => {
    void refresh()
  })

  watch(connected, (isConnected) => {
    if (isConnected) {
      void refresh()
    }
  })

  watch(
    () => toValue(active),
    (isActive) => {
      if (isActive) {
        void refresh()
      }
    },
  )

  return { loading, refresh }
}
