use rusqlite::{Connection, OptionalExtension};

use super::schema::{NEWS_INDEX_SQL, NEWS_MESSAGES_SQL, NEWS_SYNC_STATE_SQL};
use super::NewsStorageError;

const MESSAGE_COLUMNS: &[&str] = &[
    "0|delivery_id|INTEGER|0|<null>|1",
    "1|created_at_ms|INTEGER|1|<null>|0",
    "2|text|TEXT|1|<null>|0",
    "3|received_at_ms|INTEGER|1|<null>|0",
];
const STATE_COLUMNS: &[&str] = &[
    "0|singleton_id|INTEGER|0|<null>|1",
    "1|cursor|INTEGER|1|0|0",
    "2|last_seen_delivery_id|INTEGER|1|0|0",
    "3|initial_sync_complete|INTEGER|1|0|0",
    "4|source_fingerprint|TEXT|1|''|0",
];
const INDEX_COLUMNS: &[&str] = &["0|1|created_at_ms", "1|0|delivery_id"];

pub(super) fn validate_v1(connection: &Connection) -> Result<(), NewsStorageError> {
    validate_sql(connection, "table", "news_messages", NEWS_MESSAGES_SQL)?;
    validate_sql(connection, "table", "news_sync_state", NEWS_SYNC_STATE_SQL)?;
    validate_sql(
        connection,
        "index",
        "idx_news_messages_created_at",
        NEWS_INDEX_SQL,
    )?;
    validate_metadata(connection, "news_messages", MESSAGE_COLUMNS)?;
    validate_metadata(connection, "news_sync_state", STATE_COLUMNS)?;
    validate_index(connection)?;
    validate_singleton(connection)
}

fn validate_sql(
    connection: &Connection,
    object_type: &str,
    name: &str,
    expected: &str,
) -> Result<(), NewsStorageError> {
    let actual: Option<String> = connection
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
            [object_type, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| NewsStorageError::database())?;
    if actual
        .as_deref()
        .is_none_or(|actual| normalize_sql(actual) != normalize_sql(expected))
    {
        return Err(NewsStorageError::database());
    }
    Ok(())
}

fn validate_metadata(
    connection: &Connection,
    table: &str,
    expected: &[&str],
) -> Result<(), NewsStorageError> {
    let mut statement = connection
        .prepare(
            "SELECT cid, name, type, \"notnull\", dflt_value, pk \
             FROM pragma_table_info(?1) ORDER BY cid",
        )
        .map_err(|_| NewsStorageError::database())?;
    let actual: Vec<String> = statement
        .query_map([table], |row| {
            let default: Option<String> = row.get(4)?;
            Ok(format!(
                "{}|{}|{}|{}|{}|{}",
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                default.as_deref().unwrap_or("<null>"),
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|_| NewsStorageError::database())?
        .collect::<Result<_, _>>()
        .map_err(|_| NewsStorageError::database())?;
    if actual.iter().map(String::as_str).collect::<Vec<_>>() != expected {
        return Err(NewsStorageError::database());
    }
    Ok(())
}

fn validate_index(connection: &Connection) -> Result<(), NewsStorageError> {
    let mut statement = connection
        .prepare("SELECT seqno, cid, name FROM pragma_index_info(?1) ORDER BY seqno")
        .map_err(|_| NewsStorageError::database())?;
    let actual: Vec<String> = statement
        .query_map(["idx_news_messages_created_at"], |row| {
            Ok(format!(
                "{}|{}|{}",
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| NewsStorageError::database())?
        .collect::<Result<_, _>>()
        .map_err(|_| NewsStorageError::database())?;
    if actual.iter().map(String::as_str).collect::<Vec<_>>() != INDEX_COLUMNS {
        return Err(NewsStorageError::database());
    }
    Ok(())
}

fn validate_singleton(connection: &Connection) -> Result<(), NewsStorageError> {
    let (total, valid): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN singleton_id = ?1 \
             AND cursor >= 0 AND last_seen_delivery_id >= 0 \
             AND initial_sync_complete IN (0, 1) THEN 1 ELSE 0 END), 0) \
             FROM news_sync_state",
            [1_i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| NewsStorageError::database())?;
    if (total, valid) != (1, 1) {
        return Err(NewsStorageError::database());
    }
    Ok(())
}

fn normalize_sql(sql: &str) -> String {
    sql.chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != ';')
        .flat_map(char::to_lowercase)
        .collect()
}
