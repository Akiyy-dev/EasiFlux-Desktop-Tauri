use std::sync::{Arc, Mutex};

use super::{
    get_news_status_from, list_news_messages_from, mark_news_seen_from, parse_delivery_id,
    parse_limit, recheck_news_credentials_from, retry_news_sync_from,
};
use crate::models::news::{
    NewsMessageDto, NewsMessagesCommittedEvent, NewsPage, NewsStatusKind, NewsStatusSnapshot,
    NewsUnreadSnapshot, ValidatedNewsPage,
};
use crate::services::news::ports::{NewsEventError, NewsEventSink, NewsRepository};
use crate::services::NewsService;
use crate::storage::{NewsCommitOutcome, NewsDatabaseState, NewsStorageError};

#[derive(Default)]
struct CommandRepository {
    calls: Mutex<Vec<&'static str>>,
}

impl NewsRepository for CommandRepository {
    fn prepare_source(&self, _fingerprint: &str) -> Result<(), NewsStorageError> {
        unreachable!()
    }

    fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        unreachable!()
    }

    fn commit_page(
        &self,
        _page: &ValidatedNewsPage,
        _received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        unreachable!()
    }

    fn list_messages(
        &self,
        before_delivery_id: Option<i64>,
        limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        assert_eq!(before_delivery_id, Some(9));
        assert_eq!(limit, 50);
        self.calls.lock().unwrap().push("list");
        Ok(NewsPage {
            items: vec![NewsMessageDto {
                delivery_id: "8".into(),
                created_at: "2026-07-30T00:00:00.000Z".into(),
                text: "cached".into(),
            }],
            has_more: false,
            latest_delivery_id: Some("8".into()),
            unread_count: 1,
        })
    }

    fn mark_seen(&self, through_delivery_id: i64) -> Result<NewsUnreadSnapshot, NewsStorageError> {
        assert_eq!(through_delivery_id, 8);
        self.calls.lock().unwrap().push("mark");
        Ok(NewsUnreadSnapshot {
            latest_delivery_id: Some("8".into()),
            unread_count: 0,
        })
    }
}

struct NullEvents;

impl NewsEventSink for NullEvents {
    fn emit_messages_committed(
        &self,
        _event: &NewsMessagesCommittedEvent,
    ) -> Result<(), NewsEventError> {
        Ok(())
    }

    fn emit_status_changed(&self, _status: &NewsStatusSnapshot) -> Result<(), NewsEventError> {
        Ok(())
    }
}

#[test]
fn decimal_id_parser_accepts_only_positive_ascii_signed_i64() {
    assert_eq!(parse_delivery_id("1").unwrap(), 1);
    assert_eq!(parse_delivery_id("9223372036854775807").unwrap(), i64::MAX);
    for invalid in [
        "",
        "0",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1.0",
        "1x",
        "１２",
        "9223372036854775808",
    ] {
        assert!(parse_delivery_id(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn limit_defaults_to_50_and_accepts_only_one_through_50() {
    assert_eq!(parse_limit(None).unwrap(), 50);
    assert_eq!(parse_limit(Some(1)).unwrap(), 1);
    assert_eq!(parse_limit(Some(50)).unwrap(), 50);
    assert!(parse_limit(Some(-1)).is_err());
    assert!(parse_limit(Some(0)).is_err());
    assert!(parse_limit(Some(51)).is_err());
}

#[tokio::test]
async fn validation_and_repository_errors_serialize_as_stable_redacted_chinese_messages() {
    let invalid = list_news_messages_from(
        &NewsService::deployment_misconfigured(Arc::new(NullEvents)),
        Some("private/raw/path".into()),
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        serde_json::to_value(invalid).unwrap(),
        serde_json::json!("配置错误: 新闻请求参数无效")
    );

    let unavailable = list_news_messages_from(
        &NewsService::deployment_misconfigured(Arc::new(NullEvents)),
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        serde_json::to_value(unavailable).unwrap(),
        serde_json::json!("存储错误: 新闻缓存暂不可用")
    );
}

#[tokio::test]
async fn five_command_helpers_delegate_only_to_the_app_wide_news_service() {
    let repository = Arc::new(CommandRepository::default());
    let service = NewsService::deployment_misconfigured_with_repository(
        repository.clone(),
        Arc::new(NullEvents),
    );

    assert_eq!(
        get_news_status_from(&service).kind,
        NewsStatusKind::DeploymentMisconfigured
    );
    assert_eq!(
        list_news_messages_from(&service, Some("9".into()), None)
            .await
            .unwrap()
            .items[0]
            .text,
        "cached"
    );
    assert_eq!(
        mark_news_seen_from(&service, "8".into())
            .await
            .unwrap()
            .unread_count,
        0
    );
    assert_eq!(
        recheck_news_credentials_from(&service).kind,
        NewsStatusKind::DeploymentMisconfigured
    );
    assert_eq!(
        retry_news_sync_from(&service).kind,
        NewsStatusKind::DeploymentMisconfigured
    );
    assert_eq!(*repository.calls.lock().unwrap(), ["list", "mark"]);
}
