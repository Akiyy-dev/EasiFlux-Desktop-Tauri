use rusqlite::params;

use super::{message, open_temporary, page, page_for_ids, raw_connection, temporary_database};
use crate::storage::news_database::{NewsDatabase, NewsStorageErrorKind};

#[test]
fn commit_page_inserts_ascending_and_advances_cursor() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");

    let outcome = database
        .commit_page(
            &page(
                vec![message(2, 2_000, "two"), message(4, 4_000, "four")],
                4,
                true,
            ),
            9_000,
        )
        .expect("commit page");

    assert_eq!(outcome.inserted_count, 2);
    assert_eq!(outcome.newest_delivery_id, Some(4));
    assert_eq!(outcome.unread_count, 0);
    assert!(!outcome.initial_sync_complete);
    let state = database.state_snapshot().expect("state");
    assert_eq!(state.cursor, 4);
    assert_eq!(state.synced_count, 2);
    assert_eq!(state.latest_delivery_id, Some(4));
    let connection = database.connection.lock().expect("database lock");
    let stored: Vec<(i64, i64)> = connection
        .prepare("SELECT delivery_id, received_at_ms FROM news_messages ORDER BY delivery_id")
        .expect("stored query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("stored rows")
        .collect::<Result<_, _>>()
        .expect("stored values");
    assert_eq!(stored, vec![(2, 9_000), (4, 9_000)]);
}

#[test]
fn duplicate_rows_are_ignored_while_cursor_still_advances() {
    let (_directory, path, database) = open_temporary();
    database.prepare_source("source-a").expect("source");
    let connection = raw_connection(&path);
    connection
        .execute(
            "INSERT INTO news_messages(delivery_id, created_at_ms, text, received_at_ms) \
             VALUES (?1, ?2, ?3, ?4)",
            params![5_i64, 5_000_i64, "existing", 6_000_i64],
        )
        .expect("seed duplicate");
    drop(connection);

    let outcome = database
        .commit_page(&page_for_ids(&[5], true), 9_000)
        .expect("idempotent page");

    assert_eq!(outcome.inserted_count, 0);
    assert_eq!(database.state_snapshot().expect("state").cursor, 5);
    let connection = database.connection.lock().expect("database lock");
    let stored: (String, i64) = connection
        .query_row(
            "SELECT text, received_at_ms FROM news_messages WHERE delivery_id = 5",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("preserved duplicate");
    assert_eq!(stored, ("existing".to_owned(), 6_000));
}

#[test]
fn invalid_page_cursor_contract_changes_nothing() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[1], true), 1_000)
        .expect("first page");
    let before = database.state_snapshot().expect("before state");

    for invalid in [
        page(vec![message(1, 1_000, "replay")], 1, true),
        page(
            vec![message(3, 3_000, "three"), message(2, 2_000, "two")],
            2,
            true,
        ),
        page(vec![message(2, 2_000, "two")], 3, true),
        page(Vec::new(), 1, true),
    ] {
        let error = database
            .commit_page(&invalid, 2_000)
            .expect_err("invalid page must fail");
        assert_eq!(error.kind(), NewsStorageErrorKind::InvalidInput);
        assert_eq!(database.state_snapshot().expect("unchanged state"), before);
    }
}

#[test]
fn sql_failure_rolls_back_all_page_inserts() {
    let (_directory, path, database) = open_temporary();
    database.prepare_source("source-a").expect("source");
    let connection = raw_connection(&path);
    connection
        .execute_batch(
            "CREATE TRIGGER fail_news_state_update BEFORE UPDATE ON news_sync_state \
             BEGIN SELECT RAISE(ABORT, 'forced'); END;",
        )
        .expect("force transaction failure");
    drop(connection);

    let error = database
        .commit_page(&page_for_ids(&[1, 2], true), 3_000)
        .expect_err("state update must fail");
    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    let connection = raw_connection(&path);
    let retained: i64 = connection
        .query_row("SELECT COUNT(*) FROM news_messages", [], |row| row.get(0))
        .expect("row count");
    assert_eq!(retained, 0);
}

#[test]
fn partial_sync_survives_reopen_and_resumes_from_persisted_cursor() {
    let (_directory, path) = temporary_database();
    {
        let database = NewsDatabase::open(&path).expect("database");
        database.prepare_source("source-a").expect("source");
        database
            .commit_page(&page_for_ids(&[2, 4], true), 5_000)
            .expect("partial page");
    }

    let database = NewsDatabase::open(&path).expect("reopen database");
    let partial = database.state_snapshot().expect("partial state");
    assert_eq!(partial.cursor, 4);
    assert_eq!(partial.synced_count, 2);
    assert!(!partial.initial_sync_complete);
    database
        .commit_page(&page_for_ids(&[7], false), 8_000)
        .expect("resume page");
    let completed = database.state_snapshot().expect("completed state");
    assert_eq!(completed.cursor, 7);
    assert_eq!(completed.synced_count, 3);
    assert_eq!(completed.last_seen_delivery_id, 7);
    assert!(completed.initial_sync_complete);
    assert_eq!(completed.unread_count, 0);
}

#[test]
fn first_multi_page_sync_establishes_seen_baseline_atomically() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");
    database
        .commit_page(&page_for_ids(&[1, 3], true), 4_000)
        .expect("first page");
    let outcome = database
        .commit_page(&page_for_ids(&[5, 8], false), 9_000)
        .expect("completion page");

    assert_eq!(outcome.inserted_count, 2);
    assert_eq!(outcome.newest_delivery_id, Some(8));
    assert_eq!(outcome.unread_count, 0);
    assert!(outcome.initial_sync_complete);
    let state = database.state_snapshot().expect("state");
    assert_eq!(state.last_seen_delivery_id, 8);
    assert_eq!(state.synced_count, 4);
}

#[test]
fn initial_empty_page_completes_at_zero() {
    let database = NewsDatabase::open_in_memory_for_test().expect("database");
    database.prepare_source("source-a").expect("source");

    let outcome = database
        .commit_page(&page(Vec::new(), 0, false), 1_000)
        .expect("empty completion");

    assert_eq!(outcome.inserted_count, 0);
    assert_eq!(outcome.newest_delivery_id, None);
    assert_eq!(outcome.unread_count, 0);
    assert!(outcome.initial_sync_complete);
    let state = database.state_snapshot().expect("state");
    assert_eq!(state.cursor, 0);
    assert_eq!(state.last_seen_delivery_id, 0);
    assert!(state.initial_sync_complete);
}
