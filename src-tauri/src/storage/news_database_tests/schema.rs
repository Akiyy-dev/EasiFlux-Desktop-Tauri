use std::fs;

use rusqlite::Connection;

use super::{open_temporary, raw_connection, temporary_database};
use crate::storage::news_database::{NewsDatabase, NewsStorageErrorKind};

#[test]
fn open_creates_parent_and_exact_v1_schema_with_required_pragmas() {
    let (_directory, path, database) = open_temporary();

    assert!(path.is_file());
    let connection = database.connection.lock().expect("database lock");
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("user version"),
        1
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0))
            .expect("foreign keys"),
        1
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "busy_timeout", |row| row.get::<_, i64>(0))
            .expect("busy timeout"),
        5_000
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))
            .expect("journal mode"),
        "wal"
    );
    assert_eq!(
        connection
            .pragma_query_value(None, "synchronous", |row| row.get::<_, i64>(0))
            .expect("synchronous"),
        1
    );

    let objects: Vec<(String, String)> = connection
        .prepare(
            "SELECT type, name FROM sqlite_master \
             WHERE name IN ('news_messages', 'idx_news_messages_created_at', 'news_sync_state') \
             ORDER BY name",
        )
        .expect("schema query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("schema rows")
        .collect::<Result<_, _>>()
        .expect("schema values");
    assert_eq!(
        objects,
        vec![
            (
                "index".to_owned(),
                "idx_news_messages_created_at".to_owned()
            ),
            ("table".to_owned(), "news_messages".to_owned()),
            ("table".to_owned(), "news_sync_state".to_owned()),
        ]
    );

    assert!(connection
        .execute(
            "INSERT INTO news_messages(delivery_id, created_at_ms, text, received_at_ms) \
             VALUES (0, 0, '', 0)",
            [],
        )
        .is_err());
    let defaults: (i64, i64, i64, String) = connection
        .query_row(
            "SELECT cursor, last_seen_delivery_id, initial_sync_complete, source_fingerprint \
             FROM news_sync_state WHERE singleton_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("sync defaults");
    assert_eq!(defaults, (0, 0, 0, String::new()));
}

#[test]
fn fresh_state_is_empty_and_initially_hidden() {
    let database = NewsDatabase::open_in_memory_for_test().expect("in-memory database");

    assert_eq!(
        database.state_snapshot().expect("fresh state"),
        super::super::NewsDatabaseState {
            cursor: 0,
            last_seen_delivery_id: 0,
            initial_sync_complete: false,
            synced_count: 0,
            latest_delivery_id: None,
            unread_count: 0,
        }
    );
    assert!(database
        .list_messages(None, 50)
        .expect("fresh page")
        .items
        .is_empty());
}

#[test]
fn source_binding_accepts_empty_and_same_but_rejects_different_without_mutation() {
    let database = NewsDatabase::open_in_memory_for_test().expect("in-memory database");

    database.prepare_source("source-a").expect("first binding");
    database.prepare_source("source-a").expect("same binding");
    let error = database
        .prepare_source("source-b")
        .expect_err("mismatched source");
    assert_eq!(error.kind(), NewsStorageErrorKind::SourceMismatch);
    assert_eq!(error.to_string(), "news storage failed: source mismatch");
    assert!(!format!("{error:?}").contains("source-a"));
    assert!(!format!("{error:?}").contains("source-b"));

    let connection = database.connection.lock().expect("database lock");
    let fingerprint: String = connection
        .query_row(
            "SELECT source_fingerprint FROM news_sync_state WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .expect("fingerprint");
    assert_eq!(fingerprint, "source-a");
}

#[test]
fn migration_failure_preserves_existing_database() {
    let (_directory, path) = temporary_database();
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .execute("CREATE TABLE news_messages (legacy TEXT NOT NULL)", [])
        .expect("legacy schema");
    drop(connection);

    let error = NewsDatabase::open(&path).expect_err("migration must fail");
    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    assert!(path.is_file());
    let connection = raw_connection(&path);
    let sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'news_messages'",
            [],
            |row| row.get(0),
        )
        .expect("legacy table remains");
    assert!(sql.contains("legacy"));
}

#[test]
fn corrupt_database_is_not_deleted_or_repaired() {
    let (_directory, path) = temporary_database();
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    let original = b"not a sqlite database";
    fs::write(&path, original).expect("write corrupt database");

    let error = NewsDatabase::open(&path).expect_err("corrupt database must fail");
    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    assert_eq!(fs::read(&path).expect("read preserved file"), original);
}

#[test]
fn unsupported_future_schema_is_rejected_without_changing_version() {
    let (_directory, path) = temporary_database();
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .pragma_update(None, "user_version", 2)
        .expect("future version");
    drop(connection);

    let error = NewsDatabase::open(&path).expect_err("future schema must fail");
    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    let connection = raw_connection(&path);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .expect("preserved version"),
        2
    );
}

#[test]
fn incomplete_v1_schema_is_rejected_without_automatic_repair() {
    let (_directory, path) = temporary_database();
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .execute_batch(
            "CREATE TABLE news_messages (
               delivery_id INTEGER PRIMARY KEY CHECK (delivery_id > 0),
               created_at_ms INTEGER NOT NULL,
               text TEXT NOT NULL,
               received_at_ms INTEGER NOT NULL
             );
             CREATE TABLE news_sync_state (
               singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
               cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
               last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
               initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
               source_fingerprint TEXT NOT NULL DEFAULT ''
             );
             INSERT INTO news_sync_state(singleton_id) VALUES (1);
             PRAGMA user_version = 1;",
        )
        .expect("incomplete v1 schema");
    drop(connection);

    let error = NewsDatabase::open(&path).expect_err("missing index must fail");
    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    let connection = raw_connection(&path);
    let index_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' \
             AND name = 'idx_news_messages_created_at'",
            [],
            |row| row.get(0),
        )
        .expect("index count");
    assert_eq!(index_count, 0);
}

#[test]
fn same_name_tables_missing_v1_constraints_are_rejected_unchanged() {
    let (_directory, path) = temporary_database();
    let weak_messages = "CREATE TABLE news_messages (
        delivery_id INTEGER,
        created_at_ms INTEGER,
        text TEXT,
        received_at_ms INTEGER
    )";
    let exact_index = "CREATE INDEX idx_news_messages_created_at
        ON news_messages(created_at_ms DESC, delivery_id DESC)";
    let exact_state = "CREATE TABLE news_sync_state (
        singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
        cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
        last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
        initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
        source_fingerprint TEXT NOT NULL DEFAULT ''
    )";
    seed_v1_schema(&path, weak_messages, exact_index, exact_state, &[1]);
    let original = fs::read(&path).expect("original database bytes");

    let error = NewsDatabase::open(&path).expect_err("weak same-name table must fail");

    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    assert!(path.is_file());
    assert_eq!(fs::read(&path).expect("preserved database bytes"), original);
    assert_eq!(schema_sql(&path, "table", "news_messages"), weak_messages);
    assert_eq!(
        schema_sql(&path, "index", "idx_news_messages_created_at"),
        exact_index
    );
}

#[test]
fn same_name_index_with_wrong_columns_and_direction_is_rejected_unchanged() {
    let (_directory, path) = temporary_database();
    let exact_messages = "CREATE TABLE news_messages (
        delivery_id INTEGER PRIMARY KEY CHECK (delivery_id > 0),
        created_at_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        received_at_ms INTEGER NOT NULL
    )";
    let wrong_index = "CREATE INDEX idx_news_messages_created_at
        ON news_messages(delivery_id ASC)";
    let exact_state = "CREATE TABLE news_sync_state (
        singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
        cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
        last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
        initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
        source_fingerprint TEXT NOT NULL DEFAULT ''
    )";
    seed_v1_schema(&path, exact_messages, wrong_index, exact_state, &[1]);
    let original = fs::read(&path).expect("original database bytes");

    let error = NewsDatabase::open(&path).expect_err("wrong same-name index must fail");

    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    assert!(path.is_file());
    assert_eq!(fs::read(&path).expect("preserved database bytes"), original);
    assert_eq!(
        schema_sql(&path, "index", "idx_news_messages_created_at"),
        wrong_index
    );
}

#[test]
fn additional_sync_state_rows_are_rejected_without_being_removed() {
    let (_directory, path) = temporary_database();
    let exact_messages = "CREATE TABLE news_messages (
        delivery_id INTEGER PRIMARY KEY CHECK (delivery_id > 0),
        created_at_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        received_at_ms INTEGER NOT NULL
    )";
    let exact_index = "CREATE INDEX idx_news_messages_created_at
        ON news_messages(created_at_ms DESC, delivery_id DESC)";
    let weak_state = "CREATE TABLE news_sync_state (
        singleton_id INTEGER,
        cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
        last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
        initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
        source_fingerprint TEXT NOT NULL DEFAULT ''
    )";
    seed_v1_schema(&path, exact_messages, exact_index, weak_state, &[1, 2]);
    let original = fs::read(&path).expect("original database bytes");

    let error = NewsDatabase::open(&path).expect_err("additional singleton row must fail");

    assert_eq!(error.kind(), NewsStorageErrorKind::Database);
    assert!(path.is_file());
    assert_eq!(fs::read(&path).expect("preserved database bytes"), original);
    let connection = raw_connection(&path);
    let rows: Vec<i64> = connection
        .prepare("SELECT singleton_id FROM news_sync_state ORDER BY singleton_id")
        .expect("state query")
        .query_map([], |row| row.get(0))
        .expect("state rows")
        .collect::<Result<_, _>>()
        .expect("state values");
    assert_eq!(rows, vec![1, 2]);
    drop(connection);
    assert_eq!(schema_sql(&path, "table", "news_sync_state"), weak_state);
}

fn seed_v1_schema(
    path: &std::path::Path,
    messages_sql: &str,
    index_sql: &str,
    state_sql: &str,
    singleton_ids: &[i64],
) {
    fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
    let connection = Connection::open(path).expect("seed database");
    connection
        .execute_batch(messages_sql)
        .expect("messages table");
    connection.execute_batch(index_sql).expect("messages index");
    connection
        .execute_batch(state_sql)
        .expect("sync state table");
    for singleton_id in singleton_ids {
        connection
            .execute(
                "INSERT INTO news_sync_state(singleton_id) VALUES (?1)",
                [singleton_id],
            )
            .expect("sync state row");
    }
    connection
        .pragma_update(None, "user_version", 1)
        .expect("v1 version");
}

fn schema_sql(path: &std::path::Path, object_type: &str, name: &str) -> String {
    raw_connection(path)
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
            [object_type, name],
            |row| row.get(0),
        )
        .expect("schema definition")
}
