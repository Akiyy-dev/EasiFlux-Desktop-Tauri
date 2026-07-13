import { onMounted, ref, toValue, watch, type MaybeRef } from 'vue'
import { storeToRefs } from 'pinia'
import { useConnectionStore } from '../stores/connection'
import { refreshSyncTask } from '../services/dataSyncService'
import { reportError } from '../services/errorService'

export function useOrderCenterRefresh(active: MaybeRef<boolean>) {
  const connectionStore = useConnectionStore()
  const { connected } = storeToRefs(connectionStore)
  const loading = ref(false)

  async function refresh(): Promise<void> {
    if (!connectionStore.connected) {
      return
    }
    loading.value = true
    try {
      await refreshSyncTask('privatePanels')
    } catch (error) {
      reportError(error)
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
