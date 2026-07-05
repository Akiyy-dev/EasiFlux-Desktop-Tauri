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
  wsPublicUrl: string
  wsPrivateUrl: string
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
  positionIdx?: number
}

export interface PlaceOrderRequest {
  symbol: string
  side: string
  orderType: string
  qty: string
  positionIdx?: number
  price?: string
  timeInForce?: string
  orderLinkId?: string
  reduceOnly?: boolean
}

export interface CancelOrderRequest {
  symbol: string
  orderId?: string
  orderLinkId?: string
}

export interface CancelAllOrdersRequest {
  symbol?: string
  coin?: string
  orderFilter?: string
}

export interface ReplaceOrderRequest {
  symbol: string
  orderId?: string
  orderLinkId?: string
  price?: string
  qty?: string
}

export interface SetLeverageRequest {
  symbol: string
  buyLeverage?: number
  sellLeverage?: number
}

export interface AddMarginRequest {
  symbol: string
  positionIdx: number
  margin: string
}

export interface CloseAllPositionsRequest {
  symbol?: string
  coin?: string
  positionIdx?: number
}

export interface CreateTpslRequest {
  symbol: string
  positionIdx: number
  tpSlMode: string
  takeProfit?: string
  stopLoss?: string
}

export interface ReplaceTpslRequest {
  symbol: string
  orderId: string
  takeProfit?: string
  stopLoss?: string
}

export interface SwitchMarginModeRequest {
  symbol: string
  marginMode: string
}

export interface SwitchSeparatePositionModeRequest {
  coin: string
  positionMode: string
}

export interface TransferRequest {
  amount: string
  coin: string
  fromWallet: string
  toWallet: string
}

export interface TradeStats {
  totalOrders: number
  filledOrders: number
  cancelledOrders: number
  totalVolume: string
  realizedPnl: string
  unrealisedPnl: string
  winRatePct: string
  winCount: number
  lossCount: number
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

export interface ProbeEndpointResult {
  endpoint: string
  success: boolean
  code?: string
  dataType: string
  dataKeys: string[]
  envelopeHint: string
  rawCount: number
  parsedCount: number
  firstItemKeys: string[]
  error?: string
}

export interface ProbePrivateEndpointsResult {
  balancesOk: boolean
  balanceCount: number
  endpoints: ProbeEndpointResult[]
}
