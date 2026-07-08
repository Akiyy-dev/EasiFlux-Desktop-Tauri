import { tauriInvoke } from '../composables/useTauriCommand'
import { useAsyncState } from '../composables/useAsyncState'
import type { PrivatePanelsSnapshot } from '../types/models'
import { normalizeOrders } from '../utils/order'
import { normalizePositions } from '../utils/position'
import { useOrderStore } from './order'
import { usePositionStore } from './position'

const privatePanelsState = useAsyncState<PrivatePanelsSnapshot>((value) => value.positions.length === 0)

export async function refreshPrivatePanels(symbol?: string): Promise<PrivatePanelsSnapshot> {
  const snapshot = await privatePanelsState.run(() =>
    tauriInvoke<PrivatePanelsSnapshot>('refresh_private_panels', {
      symbol: symbol ?? null,
    }),
  )
  const orderStore = useOrderStore()
  const positionStore = usePositionStore()
  orderStore.setOpenOrders(normalizeOrders(snapshot.openOrders))
  orderStore.setOrderHistory(normalizeOrders(snapshot.orderHistory))
  positionStore.setPositions(normalizePositions(snapshot.positions))
  return snapshot
}

export function usePrivatePanelsState() {
  return privatePanelsState
}
