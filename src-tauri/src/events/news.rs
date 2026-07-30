use serde::Serialize;

use crate::services::news::ports::NewsEventError;

pub const NEWS_MESSAGES_COMMITTED_EVENT: &str = "news://messages-committed";
pub const NEWS_STATUS_CHANGED_EVENT: &str = "news://status-changed";

pub(crate) fn emit_news_payload<T, E, F>(
    emit: F,
    event_name: &str,
    payload: &T,
) -> Result<(), NewsEventError>
where
    T: Serialize + ?Sized,
    F: FnOnce(&str, &T) -> Result<(), E>,
{
    emit(event_name, payload).map_err(|_| NewsEventError)
}

#[cfg(test)]
#[path = "news_tests.rs"]
mod tests;
