import { useAccountStore } from '../stores/account'
import { useOrderStore } from '../stores/order'
import { usePositionStore } from '../stores/position'
import { clearPrivatePanels } from '../stores/privatePanels'

export function clearAccountBoundState(): void {
  useAccountStore().clearAccountData()
  useOrderStore().clearOrders()
  usePositionStore().clearPositions()
  clearPrivatePanels()
}
