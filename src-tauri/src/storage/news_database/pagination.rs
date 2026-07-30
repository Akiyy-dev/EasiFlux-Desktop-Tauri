use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection};

use super::queries::{latest_delivery_id, unread_count};
use super::{NewsDatabase, NewsStorageError};
use crate::models::news::{NewsMessageDto, NewsPage};

impl NewsDatabase {
    pub(crate) fn list_messages(
        &self,
        before_delivery_id: Option<i64>,
        limit: usize,
    ) -> Result<NewsPage, NewsStorageError> {
        if !(1..=50).contains(&limit) {
            return Err(NewsStorageError::invalid_input());
        }
        let connection = self.lock()?;
        let initial_sync_complete: bool = connection
            .query_row(
                "SELECT initial_sync_complete FROM news_sync_state WHERE singleton_id = ?1",
                [1_i64],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .map_err(|_| NewsStorageError::database())?;
        let latest = latest_delivery_id(&connection)?;
        let unread = unread_count(&connection, initial_sync_complete)?;
        if !initial_sync_complete {
            return Ok(NewsPage {
                items: Vec::new(),
                has_more: false,
                latest_delivery_id: latest.map(|id| id.to_string()),
                unread_count: 0,
            });
        }

        let fetch_limit =
            i64::try_from(limit + 1).map_err(|_| NewsStorageError::invalid_input())?;
        let mut items = if let Some(before) = before_delivery_id {
            query_messages(
                &connection,
                "SELECT delivery_id, created_at_ms, text FROM news_messages \
                 WHERE delivery_id < ?1 ORDER BY delivery_id DESC LIMIT ?2",
                params![before, fetch_limit],
            )?
        } else {
            query_messages(
                &connection,
                "SELECT delivery_id, created_at_ms, text FROM news_messages \
                 ORDER BY delivery_id DESC LIMIT ?1",
                params![fetch_limit],
            )?
        };
        let has_more = items.len() > limit;
        items.truncate(limit);
        Ok(NewsPage {
            items,
            has_more,
            latest_delivery_id: latest.map(|id| id.to_string()),
            unread_count: unread,
        })
    }
}

fn query_messages<P>(
    connection: &Connection,
    sql: &str,
    parameters: P,
) -> Result<Vec<NewsMessageDto>, NewsStorageError>
where
    P: rusqlite::Params,
{
    let mut statement = connection
        .prepare(sql)
        .map_err(|_| NewsStorageError::database())?;
    let rows = statement
        .query_map(parameters, |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| NewsStorageError::database())?;
    let mut items = Vec::new();
    for row in rows {
        let (delivery_id, created_at_ms, text) = row.map_err(|_| NewsStorageError::database())?;
        let created_at = DateTime::<Utc>::from_timestamp_millis(created_at_ms)
            .ok_or_else(NewsStorageError::database)?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        items.push(NewsMessageDto {
            delivery_id: delivery_id.to_string(),
            created_at,
            text,
        });
    }
    Ok(items)
}
