use super::{message, page, page_for_ids, temporary_database};
use crate::storage::news_database::{NewsDatabase, NewsStorageErrorKind};

#[test]
fn partial_rows_are_hidden_but_progress_is_retained() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[2, 9], true), 10_000)
        .expect("partial page");

    let state = database.state_snapshot().expect("state");
    assert_eq!(state.cursor, 9);
    assert_eq!(state.synced_count, 2);
    assert_eq!(state.latest_delivery_id, Some(9));
    assert_eq!(state.unread_count, 0);
    let listed = database.list_messages(None, 50).expect("hidden list");
    assert!(listed.items.is_empty());
    assert!(!listed.has_more);
    assert_eq!(listed.latest_delivery_id, Some("9".to_owned()));
    assert_eq!(listed.unread_count, 0);
}

#[test]
fn unread_counts_rows_not_id_distance_and_caps_at_one_hundred() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[1], false), 2_000)
        .expect("baseline");

    let first_live: Vec<_> = (1..=100)
        .map(|index| message(index * 10 + 1, index * 1_000, "live"))
        .collect();
    database
        .commit_page(&page(first_live, 1_001, true), 3_000)
        .expect("first live page");
    assert_eq!(
        database
            .state_snapshot()
            .expect("capped state")
            .unread_count,
        100
    );
    database
        .commit_page(&page_for_ids(&[2_001], false), 4_000)
        .expect("extra live page");
    assert_eq!(
        database
            .state_snapshot()
            .expect("still capped")
            .unread_count,
        100
    );
}

#[test]
fn pagination_handles_forty_nine_fifty_and_fifty_one_rows() {
    for (count, expected_len, expected_more) in
        [(49_i64, 49_usize, false), (50, 50, false), (51, 50, true)]
    {
        let database = NewsDatabase::open_in_memory_for_test().expect("database");
        database.prepare_source("source-a").expect("source");
        let ids: Vec<i64> = (1..=count).collect();
        database
            .commit_page(&page_for_ids(&ids, false), 60_000)
            .expect("completed page");

        let listed = database.list_messages(None, 50).expect("list page");
        assert_eq!(listed.items.len(), expected_len);
        assert_eq!(listed.has_more, expected_more);
        assert_eq!(
            listed.items.first().map(|item| item.delivery_id.as_str()),
            Some(count.to_string()).as_deref()
        );
    }
}

#[test]
fn older_page_uses_a_strict_before_boundary_and_global_latest() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[1, 3, 5], false), 6_000)
        .expect("baseline");

    let page = database.list_messages(Some(3), 50).expect("older page");
    assert_eq!(
        page.items
            .iter()
            .map(|item| item.delivery_id.as_str())
            .collect::<Vec<_>>(),
        vec!["1"]
    );
    assert!(!page.has_more);
    assert_eq!(page.latest_delivery_id, Some("5".to_owned()));
}

#[test]
fn list_converts_milliseconds_to_rfc3339_utc_and_returns_newest_first() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(
            &page(
                vec![
                    message(1, 0, "epoch"),
                    message(2, 1_704_067_200_123, "new year"),
                ],
                2,
                false,
            ),
            2_000,
        )
        .expect("baseline");

    let listed = database.list_messages(None, 50).expect("list");
    assert_eq!(listed.items[0].delivery_id, "2");
    assert_eq!(listed.items[0].created_at, "2024-01-01T00:00:00.123Z");
    assert_eq!(listed.items[0].text, "new year");
    assert_eq!(listed.items[1].created_at, "1970-01-01T00:00:00.000Z");
}

#[test]
fn list_rejects_limits_outside_one_through_fifty() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");

    for limit in [0, 51, usize::MAX] {
        let error = database
            .list_messages(None, limit)
            .expect_err("invalid limit");
        assert_eq!(error.kind(), NewsStorageErrorKind::InvalidInput);
    }
}

#[test]
fn mark_seen_is_monotonic_clamped_and_returns_decimal_latest_id() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[1, 5], false), 6_000)
        .expect("baseline");
    database
        .commit_page(&page_for_ids(&[10, 20], false), 21_000)
        .expect("live page");

    let first = database.mark_seen(10).expect("mark ten");
    assert_eq!(first.latest_delivery_id, Some("20".to_owned()));
    assert_eq!(first.unread_count, 1);
    assert_eq!(
        database
            .state_snapshot()
            .expect("first state")
            .last_seen_delivery_id,
        10
    );
    assert_eq!(database.mark_seen(9).expect("no decrease").unread_count, 1);
    assert_eq!(
        database
            .state_snapshot()
            .expect("second state")
            .last_seen_delivery_id,
        10
    );
    assert_eq!(database.mark_seen(999).expect("clamp").unread_count, 0);
    assert_eq!(
        database
            .state_snapshot()
            .expect("clamped state")
            .last_seen_delivery_id,
        20
    );
}

#[test]
fn mark_seen_during_initial_sync_never_exposes_unread_or_rows() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[2, 4], true), 5_000)
        .expect("partial page");

    let snapshot = database.mark_seen(2).expect("mark partial seen");
    assert_eq!(snapshot.latest_delivery_id, Some("4".to_owned()));
    assert_eq!(snapshot.unread_count, 0);
    assert!(database
        .list_messages(None, 50)
        .expect("hidden list")
        .items
        .is_empty());
}

#[test]
fn state_messages_and_seen_position_persist_across_restart() {
    let (_directory, path) = temporary_database();
    {
        let database = NewsDatabase::open(&path).expect("database");
        database.prepare_source("source-a").expect("source");
        database
            .commit_page(&page_for_ids(&[1, 3], false), 4_000)
            .expect("baseline");
        database
            .commit_page(&page_for_ids(&[8, 13], false), 14_000)
            .expect("live page");
        database.mark_seen(8).expect("seen eight");
    }

    let database = NewsDatabase::open(&path).expect("reopen");
    database.prepare_source("source-a").expect("same source");
    let state = database.state_snapshot().expect("persisted state");
    assert_eq!(state.cursor, 13);
    assert_eq!(state.last_seen_delivery_id, 8);
    assert_eq!(state.synced_count, 4);
    assert_eq!(state.latest_delivery_id, Some(13));
    assert_eq!(state.unread_count, 1);
    let listed = database.list_messages(None, 50).expect("persisted list");
    assert_eq!(listed.items.len(), 4);
    assert_eq!(listed.items[0].delivery_id, "13");
}
