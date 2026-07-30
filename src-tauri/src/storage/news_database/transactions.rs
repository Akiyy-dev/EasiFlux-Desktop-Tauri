use rusqlite::{params, TransactionBehavior};

use super::queries::{latest_delivery_id, unread_count};
use super::{NewsCommitOutcome, NewsDatabase, NewsStorageError};
use crate::models::news::ValidatedNewsPage;

impl NewsDatabase {
    pub(crate) fn prepare_source(&self, fingerprint: &str) -> Result<(), NewsStorageError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| NewsStorageError::database())?;
        let stored: String = transaction
            .query_row(
                "SELECT source_fingerprint FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| row.get(0),
            )
            .map_err(|_| NewsStorageError::database())?;
        if stored.is_empty() {
            transaction
                .execute(
                    "UPDATE news_sync_state SET source_fingerprint = ?1 WHERE singleton_id = ?2",
                    params![fingerprint, 1_i64],
                )
                .map_err(|_| NewsStorageError::database())?;
        } else if stored != fingerprint {
            return Err(NewsStorageError::source_mismatch());
        }
        transaction
            .commit()
            .map_err(|_| NewsStorageError::database())
    }

    pub(crate) fn commit_page(
        &self,
        page: &ValidatedNewsPage,
        received_at_ms: i64,
    ) -> Result<NewsCommitOutcome, NewsStorageError> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| NewsStorageError::database())?;
        let (cursor, was_complete): (i64, bool) = transaction
            .query_row(
                "SELECT cursor, initial_sync_complete \
                 FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
            )
            .map_err(|_| NewsStorageError::database())?;
        validate_page(page, cursor)?;

        let mut inserted_count = 0_u64;
        for item in &page.items {
            inserted_count += transaction
                .execute(
                    "INSERT INTO news_messages(delivery_id, created_at_ms, text, received_at_ms) \
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT(delivery_id) DO NOTHING",
                    params![
                        item.delivery_id,
                        item.created_at_ms,
                        item.text,
                        received_at_ms
                    ],
                )
                .map_err(|_| NewsStorageError::database())? as u64;
        }
        transaction
            .execute(
                "UPDATE news_sync_state SET cursor = ?1 WHERE singleton_id = ?2",
                params![page.next_cursor, 1_i64],
            )
            .map_err(|_| NewsStorageError::database())?;
        if !was_complete && !page.has_more {
            transaction
                .execute(
                    "UPDATE news_sync_state SET initial_sync_complete = ?1, \
                     last_seen_delivery_id = COALESCE((SELECT MAX(delivery_id) FROM news_messages), 0) \
                     WHERE singleton_id = ?2",
                    params![1_i64, 1_i64],
                )
                .map_err(|_| NewsStorageError::database())?;
        }
        let initial_sync_complete: bool = transaction
            .query_row(
                "SELECT initial_sync_complete FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .map_err(|_| NewsStorageError::database())?;
        let newest_delivery_id = latest_delivery_id(&transaction)?;
        let unread_count = unread_count(&transaction, initial_sync_complete)?;
        transaction
            .commit()
            .map_err(|_| NewsStorageError::database())?;
        Ok(NewsCommitOutcome {
            inserted_count,
            newest_delivery_id,
            unread_count,
            initial_sync_complete,
        })
    }
}

fn validate_page(page: &ValidatedNewsPage, cursor: i64) -> Result<(), NewsStorageError> {
    if page.items.is_empty() {
        return if page.next_cursor == cursor && !page.has_more {
            Ok(())
        } else {
            Err(NewsStorageError::invalid_input())
        };
    }
    let mut previous = cursor;
    for item in &page.items {
        if item.delivery_id <= previous {
            return Err(NewsStorageError::invalid_input());
        }
        previous = item.delivery_id;
    }
    if page.next_cursor != previous {
        return Err(NewsStorageError::invalid_input());
    }
    Ok(())
}
