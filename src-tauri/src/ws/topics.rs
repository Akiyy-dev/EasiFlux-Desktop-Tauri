//! Official WebSocket topic builders (SDK v0.3).

pub const TOPIC_TICKER_PREFIX: &str = "tickers-100.";
pub const TOPIC_DEPTH_PREFIX: &str = "ob_snap_shot.";
pub const TOPIC_TRADES_PREFIX: &str = "trades-100.";
pub const TOPIC_CANDLE_PREFIX: &str = "candle.";

pub const TOPIC_POSITION: &str = "contract.position";
pub const TOPIC_ORDER: &str = "contract.order";
pub const TOPIC_EXECUTION: &str = "contract.execution";
pub const TOPIC_WALLET: &str = "contract.wallet";

pub const PRIVATE_TOPICS: &[&str] = &[
    TOPIC_POSITION,
    TOPIC_ORDER,
    TOPIC_EXECUTION,
    TOPIC_WALLET,
];

pub fn topic_ticker(symbol: &str) -> String {
    format!("{TOPIC_TICKER_PREFIX}{symbol}")
}

pub fn topic_depth(symbol: &str, tick: &str) -> String {
    format!("{TOPIC_DEPTH_PREFIX}{symbol}.{tick}")
}

pub fn topic_candle(symbol: &str, interval: &str) -> String {
    format!("{TOPIC_CANDLE_PREFIX}{interval}.{symbol}")
}

pub fn topic_trades(symbol: &str) -> String {
    format!("{TOPIC_TRADES_PREFIX}{symbol}")
}

/// Map topic prefix to internal event channel name.
pub fn event_name_for_topic(topic: &str) -> &'static str {
    if topic.starts_with(TOPIC_TICKER_PREFIX) {
        return "ticker";
    }
    if topic.starts_with(TOPIC_DEPTH_PREFIX) {
        return "depth";
    }
    if topic.starts_with(TOPIC_TRADES_PREFIX) {
        return "trades";
    }
    if topic.starts_with(TOPIC_CANDLE_PREFIX) {
        return "candle";
    }
    match topic {
        TOPIC_POSITION => "position",
        TOPIC_ORDER => "order",
        TOPIC_EXECUTION => "execution",
        TOPIC_WALLET => "balance",
        _ => {
            if topic.contains("ticker") {
                "ticker"
            } else if topic.contains("depth") || topic.starts_with("ob_snap") {
                "depth"
            } else if topic.contains("order") {
                "order"
            } else if topic.contains("position") {
                "position"
            } else if topic.contains("wallet") || topic.contains("balance") {
                "balance"
            } else {
                "unknown"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_ticker_format() {
        assert_eq!(topic_ticker("BTCUSDT"), "tickers-100.BTCUSDT");
    }

    #[test]
    fn topic_depth_format() {
        assert_eq!(topic_depth("BTCUSDT", "1"), "ob_snap_shot.BTCUSDT.1");
    }
}
