import { tauriInvoke } from '../composables/useTauriCommand'
import { useAsyncState } from '../composables/useAsyncState'
import type { PrivatePanelsSnapshot } from '../types/models'
import { normalizeOrders } from '../utils/order'
import { normalizePositions } from '../utils/position'
import { useOrderStore } from './order'
import { usePositionStore } from './position'

const privatePanelsState = useAsyncState<PrivatePanelsSnapshot>((value) => value.positions.length === 0)

export function applyPrivatePanelsSnapshot(snapshot: PrivatePanelsSnapshot): void {
  privatePanelsState.setData(snapshot)
}

export async function refreshPrivatePanels(): Promise<PrivatePanelsSnapshot> {
  return privatePanelsState.run(async () => {
    await tauriInvoke('scheduler_run_task', { task: 'privatePanels', force: true })
    const data = privatePanelsState.state.value.data
    if (data) {
      return data
    }
    const orderStore = useOrderStore()
    const positionStore = usePositionStore()
    return {
      openOrders: orderStore.openOrders,
      orderHistory: orderStore.orderHistory,
      positions: positionStore.positions,
    }
  })
}

export function usePrivatePanelsState() {
  return privatePanelsState
}

export function replacePrivatePanels(snapshot: PrivatePanelsSnapshot): void {
  applyPrivatePanelsSnapshot(snapshot)
  const orderStore = useOrderStore()
  const positionStore = usePositionStore()
  orderStore.setOpenOrders(normalizeOrders(snapshot.openOrders))
  orderStore.setOrderHistory(normalizeOrders(snapshot.orderHistory))
  positionStore.setPositions(normalizePositions(snapshot.positions))
}

export function clearPrivatePanels(): void {
  privatePanelsState.reset()
}
