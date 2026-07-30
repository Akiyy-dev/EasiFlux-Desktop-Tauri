mod error;
mod pagination;
mod queries;
mod schema;
mod schema_validation;
mod transactions;

use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::Connection;

pub(crate) use error::{NewsStorageError, NewsStorageErrorKind};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewsDatabaseState {
    pub cursor: i64,
    pub last_seen_delivery_id: i64,
    pub initial_sync_complete: bool,
    pub synced_count: u64,
    pub latest_delivery_id: Option<i64>,
    pub unread_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NewsCommitOutcome {
    pub inserted_count: u64,
    pub newest_delivery_id: Option<i64>,
    pub unread_count: u32,
    pub initial_sync_complete: bool,
}

pub(crate) struct NewsDatabase {
    connection: Mutex<Connection>,
}

impl fmt::Debug for NewsDatabase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewsDatabase")
            .finish_non_exhaustive()
    }
}

impl NewsDatabase {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, NewsStorageError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|_| NewsStorageError::database())?;
        }
        let connection = Connection::open(path).map_err(|_| NewsStorageError::database())?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub(super) fn open_in_memory_for_test() -> Result<Self, NewsStorageError> {
        let connection = Connection::open_in_memory().map_err(|_| NewsStorageError::database())?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, NewsStorageError> {
        schema::migrate(&mut connection)?;
        configure(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, NewsStorageError> {
        self.connection
            .lock()
            .map_err(|_| NewsStorageError::database())
    }
}

fn configure(connection: &Connection) -> Result<(), NewsStorageError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|_| NewsStorageError::database())?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| NewsStorageError::database())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|_| NewsStorageError::database())?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(|_| NewsStorageError::database())?;
    Ok(())
}

#[cfg(test)]
#[path = "news_database_tests/mod.rs"]
mod tests;
