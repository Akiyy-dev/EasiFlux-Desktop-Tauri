export interface PendingOrderSubmission {
  orderLinkId: string
  symbol: string
  side: string
  orderType: string
  qty: string
  price?: string | null
  reduceOnly?: boolean | null
  createdAtMs: number
}
