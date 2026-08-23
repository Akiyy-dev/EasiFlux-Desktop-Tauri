use std::sync::{Arc, Mutex};

use crate::error::{AppError, AppResult};
use crate::models::notification::{
    NotificationAction, NotificationCategory, NotificationContent, NotificationInput,
    NotificationKind, NotificationRecord, NotificationScalar, NotificationScope,
    NotificationSeverity,
};
use crate::services::notification::{NotificationEmitter, NotificationService};
use crate::storage::notification_store::{NotificationFileV1, NotificationPersistence};

#[derive(Default)]
pub(super) struct FakePersistence {
    saves: Mutex<Vec<NotificationFileV1>>,
    fail: Mutex<bool>,
}

impl FakePersistence {
    pub(super) fn fail_next(&self) {
        *self.fail.lock().unwrap() = true;
    }

    pub(super) fn saves(&self) -> Vec<NotificationFileV1> {
        self.saves.lock().unwrap().clone()
    }
}

impl NotificationPersistence for FakePersistence {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()> {
        let mut fail = self.fail.lock().unwrap();
        if *fail {
            *fail = false;
            return Err(AppError::Storage("NOTIFICATION_STORAGE_UNAVAILABLE".into()));
        }
        self.saves.lock().unwrap().push(file.clone());
        Ok(())
    }
}

pub(super) struct Harness {
    pub service: Arc<NotificationService>,
    pub persistence: Arc<FakePersistence>,
    pub events: Arc<Mutex<Vec<crate::models::notification::NotificationChangedEvent>>>,
}

pub(super) fn harness(file: NotificationFileV1) -> Harness {
    let persistence = Arc::new(FakePersistence::default());
    let events = Arc::new(Mutex::new(Vec::new()));
    let event_sink = Arc::clone(&events);
    let emitter: NotificationEmitter = Arc::new(move |event| {
        event_sink.lock().unwrap().push(event.clone());
        Ok(())
    });
    let service = Arc::new(NotificationService::from_snapshot(
        file,
        persistence.clone(),
        emitter,
        1_700_000_000_000,
    ));
    Harness {
        service,
        persistence,
        events,
    }
}

pub(super) fn input(
    scope: NotificationScope,
    source_event_id: &str,
    dedupe_key: &str,
) -> NotificationInput {
    NotificationInput {
        scope,
        category: NotificationCategory::Trading,
        kind: NotificationKind::OrderFilled,
        severity: NotificationSeverity::Success,
        content: NotificationContent::new(
            "order.filled",
            [("orderId", NotificationScalar::String("order-1".into()))],
            "订单已成交",
            "订单已完全成交，请前往交易页查看。",
        )
        .unwrap(),
        entity: None,
        action: Some(NotificationAction::OpenTrading {
            order_id: Some("order-1".into()),
        }),
        source_event_id: Some(source_event_id.into()),
        dedupe_key: dedupe_key.into(),
        session_epoch: Some(7),
    }
}

pub(super) fn record(
    number: u128,
    scope: NotificationScope,
    source_event_id: &str,
    dedupe_key: &str,
    created_at_ms: u64,
) -> NotificationRecord {
    let input = input(scope.clone(), source_event_id, dedupe_key);
    NotificationRecord {
        id: format!("00000000-0000-4000-8000-{number:012x}"),
        scope,
        category: input.category,
        kind: input.kind,
        severity: input.severity,
        content: input.content,
        entity: input.entity,
        action: input.action,
        source_event_id: input.source_event_id,
        dedupe_key: input.dedupe_key,
        occurrence_count: 1,
        created_at_ms,
        updated_at_ms: created_at_ms,
        read_at_ms: None,
    }
}
