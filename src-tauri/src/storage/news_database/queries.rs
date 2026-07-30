use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{NewsDatabase, NewsDatabaseState, NewsStorageError};
use crate::models::news::NewsUnreadSnapshot;

const MAX_UNREAD_COUNT: i64 = 100;

impl NewsDatabase {
    pub(crate) fn state_snapshot(&self) -> Result<NewsDatabaseState, NewsStorageError> {
        let connection = self.lock()?;
        let (cursor, last_seen_delivery_id, initial_sync_complete): (i64, i64, bool) = connection
            .query_row(
                "SELECT cursor, last_seen_delivery_id, initial_sync_complete \
                     FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0)),
            )
            .map_err(|_| NewsStorageError::database())?;
        let synced_count = row_count(&connection)?;
        let latest_delivery_id = latest_delivery_id(&connection)?;
        let unread_count = unread_count(&connection, initial_sync_complete)?;
        Ok(NewsDatabaseState {
            cursor,
            last_seen_delivery_id,
            initial_sync_complete,
            synced_count,
            latest_delivery_id,
            unread_count,
        })
    }

    pub(crate) fn mark_seen(
        &self,
        through_delivery_id: i64,
    ) -> Result<NewsUnreadSnapshot, NewsStorageError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| NewsStorageError::database())?;
        let latest = latest_delivery_id(&transaction)?;
        let (current, initial_sync_complete): (i64, bool) = transaction
            .query_row(
                "SELECT last_seen_delivery_id, initial_sync_complete \
                 FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .map_err(|_| NewsStorageError::database())?;
        let maximum = latest.unwrap_or(0);
        if current > maximum {
            return Err(NewsStorageError::database());
        }
        let target = current.max(through_delivery_id.min(maximum));
        transaction
            .execute(
                "UPDATE news_sync_state SET last_seen_delivery_id = ?1 WHERE singleton_id = ?2",
                params![target, 1_i64],
            )
            .map_err(|_| NewsStorageError::database())?;
        let count = unread_count(&transaction, initial_sync_complete)?;
        transaction
            .commit()
            .map_err(|_| NewsStorageError::database())?;
        Ok(NewsUnreadSnapshot {
            latest_delivery_id: latest.map(|id| id.to_string()),
            unread_count: count,
        })
    }
}

fn row_count(connection: &Connection) -> Result<u64, NewsStorageError> {
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM news_messages", [], |row| row.get(0))
        .map_err(|_| NewsStorageError::database())?;
    u64::try_from(count).map_err(|_| NewsStorageError::database())
}

pub(super) fn latest_delivery_id(connection: &Connection) -> Result<Option<i64>, NewsStorageError> {
    connection
        .query_row("SELECT MAX(delivery_id) FROM news_messages", [], |row| {
            row.get(0)
        })
        .optional()
        .map(|value| value.flatten())
        .map_err(|_| NewsStorageError::database())
}

pub(super) fn unread_count(
    connection: &Connection,
    initial_sync_complete: bool,
) -> Result<u32, NewsStorageError> {
    if !initial_sync_complete {
        return Ok(0);
    }
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM news_messages \
             WHERE delivery_id > (SELECT last_seen_delivery_id FROM news_sync_state \
             WHERE singleton_id = ?1)",
            [1_i64],
            |row| row.get(0),
        )
        .map_err(|_| NewsStorageError::database())?;
    Ok(count.clamp(0, MAX_UNREAD_COUNT) as u32)
}
