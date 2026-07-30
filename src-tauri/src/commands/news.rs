use tauri::State;

use crate::error::{AppError, AppResult};
use crate::models::news::{NewsPage, NewsStatusSnapshot, NewsUnreadSnapshot};
use crate::services::news::NewsServiceError;
use crate::services::NewsService;
use crate::state::AppState;

const INVALID_NEWS_REQUEST: &str = "新闻请求参数无效";
const NEWS_STORAGE_UNAVAILABLE: &str = "新闻缓存暂不可用";
const DEFAULT_NEWS_LIMIT: usize = 50;
const MAX_NEWS_LIMIT: i64 = 50;

pub(crate) fn parse_delivery_id(input: &str) -> AppResult<i64> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_request());
    }
    input
        .parse::<i64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid_request)
}

pub(crate) fn parse_limit(limit: Option<i64>) -> AppResult<usize> {
    match limit {
        None => Ok(DEFAULT_NEWS_LIMIT),
        Some(limit @ 1..=MAX_NEWS_LIMIT) => Ok(limit as usize),
        Some(_) => Err(invalid_request()),
    }
}

fn invalid_request() -> AppError {
    AppError::Config(INVALID_NEWS_REQUEST.into())
}

fn map_news_error(_error: NewsServiceError) -> AppError {
    AppError::Storage(NEWS_STORAGE_UNAVAILABLE.into())
}

pub(crate) fn get_news_status_from(service: &NewsService) -> NewsStatusSnapshot {
    service.status()
}

pub(crate) async fn list_news_messages_from(
    service: &NewsService,
    before_delivery_id: Option<String>,
    limit: Option<i64>,
) -> AppResult<NewsPage> {
    let before_id = before_delivery_id
        .as_deref()
        .map(parse_delivery_id)
        .transpose()?;
    service
        .list_messages(before_id, parse_limit(limit)?)
        .await
        .map_err(map_news_error)
}

pub(crate) async fn mark_news_seen_from(
    service: &NewsService,
    through_delivery_id: String,
) -> AppResult<NewsUnreadSnapshot> {
    service
        .mark_seen(parse_delivery_id(&through_delivery_id)?)
        .await
        .map_err(map_news_error)
}

pub(crate) fn recheck_news_credentials_from(service: &NewsService) -> NewsStatusSnapshot {
    service.recheck_credentials()
}

pub(crate) fn retry_news_sync_from(service: &NewsService) -> NewsStatusSnapshot {
    service.retry_sync()
}

#[tauri::command]
pub fn get_news_status(state: State<'_, AppState>) -> NewsStatusSnapshot {
    get_news_status_from(&state.news)
}

#[tauri::command]
pub async fn list_news_messages(
    state: State<'_, AppState>,
    before_delivery_id: Option<String>,
    limit: Option<i64>,
) -> AppResult<NewsPage> {
    list_news_messages_from(&state.news, before_delivery_id, limit).await
}

#[tauri::command]
pub async fn mark_news_seen(
    state: State<'_, AppState>,
    through_delivery_id: String,
) -> AppResult<NewsUnreadSnapshot> {
    mark_news_seen_from(&state.news, through_delivery_id).await
}

#[tauri::command]
pub fn recheck_news_credentials(state: State<'_, AppState>) -> NewsStatusSnapshot {
    recheck_news_credentials_from(&state.news)
}

#[tauri::command]
pub fn retry_news_sync(state: State<'_, AppState>) -> NewsStatusSnapshot {
    retry_news_sync_from(&state.news)
}

#[cfg(test)]
#[path = "news_tests.rs"]
mod tests;
