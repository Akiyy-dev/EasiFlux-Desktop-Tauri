use std::str::FromStr;

use rust_decimal::Decimal;

use crate::api::{ApiClient, PublicApi};

pub(super) async fn fresh_reference_price(api: &ApiClient, symbol: &str) -> Option<String> {
    // Use the current API source for each submission; UI caches have no source or age guarantee.
    let ticker = PublicApi::ticker(api, symbol).await.ok()?;
    if !ticker.symbol.eq_ignore_ascii_case(symbol)
        || Decimal::from_str(&ticker.last_price).ok()? <= Decimal::ZERO
    {
        return None;
    }
    Some(ticker.last_price)
}

pub(super) async fn fresh_protection_reference(
    api: &ApiClient,
    symbol: &str,
    trigger_by: &str,
) -> Option<String> {
    let ticker = PublicApi::ticker(api, symbol).await.ok()?;
    if !ticker.symbol.eq_ignore_ascii_case(symbol) {
        return None;
    }
    let selected = match trigger_by {
        "LastPrice" => &ticker.last_price,
        "MarkPrice" => &ticker.mark_price,
        _ => return None,
    };
    if Decimal::from_str(selected).ok()? <= Decimal::ZERO {
        return None;
    }
    Some(selected.clone())
}

#[cfg(test)]
mod tests;
