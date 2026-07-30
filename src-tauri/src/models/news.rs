use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsMessageDto {
    pub delivery_id: String,
    pub created_at: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsPage {
    pub items: Vec<NewsMessageDto>,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_delivery_id: Option<String>,
    pub unread_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsUnreadSnapshot {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_delivery_id: Option<String>,
    pub unread_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NewsStatusKind {
    DeploymentMisconfigured,
    NotConfigured,
    CredentialStoreUnavailable,
    InitialSync,
    Live,
    Retrying,
    CredentialInvalid,
    ContractError,
    StorageError,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsStatusSnapshot {
    pub kind: NewsStatusKind,
    pub initial_sync_complete: bool,
    pub synced_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_delivery_id: Option<String>,
    pub unread_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsMessagesCommittedEvent {
    pub inserted_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_delivery_id: Option<String>,
    pub unread_count: u32,
    pub initial_sync_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedNewsMessage {
    pub delivery_id: i64,
    pub created_at_ms: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedNewsPage {
    pub items: Vec<ValidatedNewsMessage>,
    pub next_cursor: i64,
    pub has_more: bool,
}

#[cfg(test)]
#[path = "news_tests.rs"]
mod tests;
