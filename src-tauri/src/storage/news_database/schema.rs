use rusqlite::{Connection, TransactionBehavior};

use super::schema_validation::validate_v1;
use super::NewsStorageError;

const SCHEMA_VERSION: i64 = 1;

pub(super) const NEWS_MESSAGES_SQL: &str = "CREATE TABLE news_messages (
  delivery_id INTEGER PRIMARY KEY CHECK (delivery_id > 0),
  created_at_ms INTEGER NOT NULL,
  text TEXT NOT NULL,
  received_at_ms INTEGER NOT NULL
)";

pub(super) const NEWS_INDEX_SQL: &str = "CREATE INDEX idx_news_messages_created_at
  ON news_messages(created_at_ms DESC, delivery_id DESC)";

pub(super) const NEWS_SYNC_STATE_SQL: &str = "CREATE TABLE news_sync_state (
  singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
  cursor INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
  last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
  initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
  source_fingerprint TEXT NOT NULL DEFAULT ''
)";

pub(super) fn migrate(connection: &mut Connection) -> Result<(), NewsStorageError> {
    let version = connection
        .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
        .map_err(|_| NewsStorageError::database())?;
    match version {
        0 => migrate_to_v1(connection)?,
        SCHEMA_VERSION => {}
        _ => return Err(NewsStorageError::database()),
    }
    validate_v1(connection)
}

fn migrate_to_v1(connection: &mut Connection) -> Result<(), NewsStorageError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| NewsStorageError::database())?;
    for statement in [NEWS_MESSAGES_SQL, NEWS_INDEX_SQL, NEWS_SYNC_STATE_SQL] {
        transaction
            .execute_batch(statement)
            .map_err(|_| NewsStorageError::database())?;
    }
    transaction
        .execute(
            "INSERT INTO news_sync_state(singleton_id) VALUES (?1)",
            [1_i64],
        )
        .map_err(|_| NewsStorageError::database())?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|_| NewsStorageError::database())?;
    transaction
        .commit()
        .map_err(|_| NewsStorageError::database())
}
