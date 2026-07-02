// REST endpoints aligned with EasiFlux-SDK v0.3 DEFAULT_ENDPOINTS
pub const SERVER_TIME: &str = "/futures/public/v1/market/time";
pub const TICKER: &str = "/futures/public/v1/market/tickers";
pub const KLINE: &str = "/futures/public/v1/market/kline";
pub const DEPTH: &str = "/futures/public/v1/market/order-book";
pub const PUBLIC_TRADES: &str = "/futures/public/v1/market/trades";
pub const FUNDING_RATE_HISTORY: &str = "/futures/public/v1/market/funding-rate-history";
pub const MARK_PRICE_KLINE: &str = "/futures/public/v1/market/mark-price-kline";
pub const INSTRUMENTS: &str = "/futures/public/v1/instruments";
pub const RISK_LIMIT: &str = "/futures/public/v1/position-risk-limit";
pub const MARKET_CLOSE_TIME: &str = "/futures/public/v1/market/market-close-time";
pub const FIAT_RATE: &str = "/asset-api/fiat/public/v1/rate";

pub const CREATE_ORDER: &str = "/futures/private/v1/create-order";
pub const CANCEL_ORDER: &str = "/futures/private/v1/cancel-order";
pub const CANCEL_ALL_ORDERS: &str = "/futures/private/v1/cancel-all-orders";
pub const REPLACE_ORDER: &str = "/futures/private/v1/replace-order";
pub const OPEN_ORDERS: &str = "/futures/private/v1/trade/activity-orders";
pub const ORDERS: &str = "/futures/private/v1/trade/orders";
pub const TRADE_FILLS: &str = "/futures/private/v1/trade/fills";
pub const BALANCES: &str = "/futures/private/v1/account/balance";
pub const POSITIONS: &str = "/futures/private/v1/position/list";
pub const FEE_RATE: &str = "/futures/private/v1/account/fee-rate";
pub const SET_LEVERAGE: &str = "/futures/private/v1/position/set-leverage";
pub const ADD_MARGIN: &str = "/futures/private/v1/position/add-margin";
pub const CLOSE_ALL_POSITIONS: &str = "/futures/private/v1/position/close-all";
pub const CLOSED_PNL: &str = "/futures/private/v1/position/closed-pnl";
pub const CREATE_TPSL: &str = "/futures/private/v1/position/create-tpsl";
pub const REPLACE_TPSL: &str = "/futures/private/v1/position/replace-tpsl";
pub const SWITCH_MARGIN_MODE: &str = "/futures/private/v1/position/switch-margin-mode";
pub const SWITCH_SEPARATE_POSITION_MODE: &str = "/futures/private/v1/position/switch-separate-mode";

pub const FUNDING_BALANCES: &str = "/asset-api/account/private/v1/get-funding-account-balance";
pub const FUNDING_TRANSFER: &str = "/asset-api/account/private/v1/user-account-transfer";
pub const USER_ID: &str = "/asset-api/account/private/v1/userid";
pub const TRANSFER_HISTORY: &str = "/asset-api/account/private/v1/user-transfer-rercord/page";

pub const WS_PUBLIC: &str = "wss://ws.easicoin.io/contract/public/v1";
pub const WS_PRIVATE: &str = "wss://ws.easicoin.io/contract/private/v1";
