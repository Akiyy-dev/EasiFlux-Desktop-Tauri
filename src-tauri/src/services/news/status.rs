use crate::models::news::{NewsStatusKind, NewsStatusSnapshot};
use crate::storage::NewsDatabaseState;

pub(super) fn default_message(kind: NewsStatusKind) -> &'static str {
    match kind {
        NewsStatusKind::DeploymentMisconfigured => "新闻数据源未内置，请使用正确的安装包",
        NewsStatusKind::NotConfigured => "新闻服务未配置",
        NewsStatusKind::CredentialStoreUnavailable => "凭据存储暂不可用",
        NewsStatusKind::InitialSync => "正在同步历史新闻",
        NewsStatusKind::Live => "实时",
        NewsStatusKind::Retrying => "新闻服务暂时不可用，正在重试",
        NewsStatusKind::CredentialInvalid => "新闻服务凭据无效",
        NewsStatusKind::ContractError => "新闻服务协议异常",
        NewsStatusKind::StorageError => "新闻缓存暂不可用",
        NewsStatusKind::Stopped => "新闻服务已停止",
    }
}

pub(super) fn empty_snapshot(kind: NewsStatusKind) -> NewsStatusSnapshot {
    NewsStatusSnapshot {
        kind,
        initial_sync_complete: false,
        synced_count: 0,
        latest_delivery_id: None,
        unread_count: 0,
        retry_at: None,
        message: Some(default_message(kind).to_owned()),
    }
}

pub(super) fn from_state(
    kind: NewsStatusKind,
    state: &NewsDatabaseState,
    retry_at: Option<String>,
) -> NewsStatusSnapshot {
    NewsStatusSnapshot {
        kind,
        initial_sync_complete: state.initial_sync_complete,
        synced_count: state.synced_count,
        latest_delivery_id: state.latest_delivery_id.map(|id| id.to_string()),
        unread_count: state.unread_count.min(100),
        retry_at,
        message: Some(default_message(kind).to_owned()),
    }
}

pub(super) fn from_snapshot(
    kind: NewsStatusKind,
    snapshot: &NewsStatusSnapshot,
    retry_at: Option<String>,
) -> NewsStatusSnapshot {
    NewsStatusSnapshot {
        kind,
        initial_sync_complete: snapshot.initial_sync_complete,
        synced_count: snapshot.synced_count,
        latest_delivery_id: snapshot.latest_delivery_id.clone(),
        unread_count: snapshot.unread_count.min(100),
        retry_at,
        message: Some(default_message(kind).to_owned()),
    }
}
