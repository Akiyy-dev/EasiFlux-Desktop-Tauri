use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::{NewsFetchError, NewsFetchErrorKind, ValidatedNewsMessage, ValidatedNewsPage};

const MAX_TEXT_BYTES: usize = 256 * 1024;

pub(super) fn decode_page(
    body: &[u8],
    request_cursor: i64,
    requested_limit: usize,
) -> Result<ValidatedNewsPage, NewsFetchError> {
    let wrapper: WireResponse = serde_json::from_slice(body)
        .map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
    if !wrapper.ok {
        return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
    }
    let page = wrapper
        .data
        .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
    validate_page(page, request_cursor, requested_limit)
}

fn validate_page(
    page: WirePage,
    request_cursor: i64,
    requested_limit: usize,
) -> Result<ValidatedNewsPage, NewsFetchError> {
    if page.items.len() > requested_limit {
        return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
    }
    let next_cursor = json_i64(&page.next_cursor)?;
    if next_cursor < 0 {
        return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
    }

    let mut seen = HashSet::with_capacity(page.items.len());
    let mut items = Vec::with_capacity(page.items.len());
    for item in page.items {
        let delivery_id = json_i64(&item.delivery_id)?;
        if delivery_id <= 0 || delivery_id <= request_cursor || !seen.insert(delivery_id) {
            return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
        }
        let created_at = item
            .created_at
            .as_str()
            .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
        let created_at_ms = parse_created_at_ms(created_at)?;
        let text = item
            .text
            .as_str()
            .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
        if text.len() > MAX_TEXT_BYTES {
            return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
        }
        items.push(ValidatedNewsMessage {
            delivery_id,
            created_at_ms,
            text: if text.trim().is_empty() {
                String::new()
            } else {
                text.to_owned()
            },
        });
    }
    items.sort_unstable_by_key(|item| item.delivery_id);

    if items.is_empty() {
        if next_cursor != request_cursor || page.has_more {
            return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
        }
    } else {
        let maximum_id = items.last().expect("nonempty items").delivery_id;
        if next_cursor != maximum_id || next_cursor <= request_cursor {
            return Err(NewsFetchError::new(NewsFetchErrorKind::Contract));
        }
    }

    Ok(ValidatedNewsPage {
        items,
        next_cursor,
        has_more: page.has_more,
    })
}

fn json_i64(value: &Value) -> Result<i64, NewsFetchError> {
    value
        .as_i64()
        .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))
}

fn parse_created_at_ms(value: &str) -> Result<i64, NewsFetchError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| NewsFetchError::new(NewsFetchErrorKind::Contract))?;
    let utc = parsed.with_timezone(&Utc);
    utc.timestamp()
        .checked_mul(1_000)
        .and_then(|seconds| seconds.checked_add(i64::from(utc.timestamp_subsec_millis())))
        .ok_or_else(|| NewsFetchError::new(NewsFetchErrorKind::Contract))
}

#[derive(Deserialize)]
struct WireResponse {
    ok: bool,
    data: Option<WirePage>,
}

#[derive(Deserialize)]
struct WirePage {
    items: Vec<WireItem>,
    next_cursor: Value,
    has_more: bool,
}

#[derive(Deserialize)]
struct WireItem {
    delivery_id: Value,
    created_at: Value,
    text: Value,
}
