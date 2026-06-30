export type ThemeMode = 'dark' | 'light'

export type ConnectionStatus =
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'error'

export interface AppConfig {
  activeSymbol: string
  activeAccountId: string
  watchlistSymbols: string[]
  theme: ThemeMode
  klineInterval: string
  useWebsocket: boolean
  tickerPollInterval: number
  windowWidth: number
  windowHeight: number
  accounts: string[]
  riskEnabled: boolean
  riskMaxOrderQty: string
  riskMaxPriceDeviationPct: string
  riskMaxDailyOrders: number
}

export interface ApiCredential {
  apiKey: string
  apiSecret: string
  baseUrl: string
  label: string
}

export interface SaveCredentialRequest {
  accountId: string
  apiKey: string
  apiSecret: string
  baseUrl: string
  label: string
}

export interface Ticker {
  symbol: string
  lastPrice: string
  bidPrice: string
  askPrice: string
  volume24h: string
  change24hPct: string
}

export interface DepthLevel {
  price: string
  qty: string
}

export interface Depth {
  symbol: string
  bids: DepthLevel[]
  asks: DepthLevel[]
}

export interface Kline {
  symbol: string
  interval: string
  openTime: number
  open: string
  high: string
  low: string
  close: string
  volume: string
}

export interface Balance {
  asset: string
  available: string
  frozen: string
  total: string
}

export interface AccountSummary {
  accountId: string
  balances: Balance[]
  totalEquity: string
}

export type OrderStatus =
  | 'New'
  | 'PartiallyFilled'
  | 'Filled'
  | 'Cancelled'
  | 'Rejected'
  | 'Unknown'

export interface Order {
  orderId: string
  symbol: string
  side: string
  orderType: string
  price: string
  qty: string
  status: OrderStatus
  orderLinkId?: string
  filledQty: string
  avgPrice: string
}

export interface Position {
  symbol: string
  side: string
  size: string
  entryPrice: string
  leverage: string
  unrealisedPnl: string
}

export interface PlaceOrderRequest {
  symbol: string
  side: string
  orderType: string
  qty: string
  price?: string
}

export interface CancelOrderRequest {
  symbol: string
  orderId: string
}

export interface TradeStats {
  totalOrders: number
  filledOrders: number
  cancelledOrders: number
  totalVolume: string
  realizedPnl: string
  winRatePct: string
}

export interface LogEntry {
  level: string
  message: string
  timestamp: number
}

export interface PingResponse {
  message: string
  version: string
}
