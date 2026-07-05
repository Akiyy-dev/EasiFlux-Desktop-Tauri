import type { Order, OrderStatus } from '../types/models'

const TERMINAL_STATUSES = new Set<OrderStatus>(['Filled', 'Cancelled', 'Rejected'])

function readString(raw: Record<string, unknown>, ...keys: string[]): string {
  for (const key of keys) {
    const value = raw[key]
    if (value != null && value !== '') {
      return String(value)
    }
  }
  return ''
}

export function normalizeOrder(raw: Order | Record<string, unknown>): Order {
  const record = raw as Record<string, unknown>
  const orderLinkId = readString(record, 'orderLinkId', 'order_link_id')
  return {
    orderId: readString(record, 'orderId', 'order_id', 'id'),
    symbol: readString(record, 'symbol', 's'),
    side: readString(record, 'side'),
    orderType: readString(record, 'orderType', 'order_type', 'type'),
    price: readString(record, 'price') || '0',
    qty: readString(record, 'qty', 'quantity', 'size') || '0',
    status: (readString(record, 'status', 'orderStatus', 'order_status') ||
      'Unknown') as OrderStatus,
    orderLinkId: orderLinkId || undefined,
    filledQty:
      readString(record, 'filledQty', 'filled_qty', 'cumExecQty', 'cum_exec_qty') || '0',
    avgPrice: readString(record, 'avgPrice', 'avg_price') || '0',
  }
}

export function normalizeOrders(raw: unknown): Order[] {
  if (!Array.isArray(raw)) {
    return []
  }
  return raw
    .map((item) => normalizeOrder(item as Record<string, unknown>))
    .filter((order) => order.orderId.length > 0)
}

export function isTerminalOrderStatus(status: OrderStatus): boolean {
  return TERMINAL_STATUSES.has(status)
}
