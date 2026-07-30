mod queries;
mod schema;
mod transactions;

use std::path::Path;

use rusqlite::Connection;
use tempfile::TempDir;

use super::NewsDatabase;
use crate::models::news::{ValidatedNewsMessage, ValidatedNewsPage};

fn temporary_database() -> (TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("nested").join("news.sqlite3");
    (directory, path)
}

fn open_temporary() -> (TempDir, std::path::PathBuf, NewsDatabase) {
    let (directory, path) = temporary_database();
    let database = NewsDatabase::open(&path).expect("open database");
    (directory, path, database)
}

fn raw_connection(path: &Path) -> Connection {
    Connection::open(path).expect("open raw database")
}

fn message(delivery_id: i64, created_at_ms: i64, text: &str) -> ValidatedNewsMessage {
    ValidatedNewsMessage {
        delivery_id,
        created_at_ms,
        text: text.to_owned(),
    }
}

fn page(items: Vec<ValidatedNewsMessage>, next_cursor: i64, has_more: bool) -> ValidatedNewsPage {
    ValidatedNewsPage {
        items,
        next_cursor,
        has_more,
    }
}

fn page_for_ids(ids: &[i64], has_more: bool) -> ValidatedNewsPage {
    let items = ids
        .iter()
        .map(|id| message(*id, id.saturating_mul(1_000), &format!("message-{id}")))
        .collect();
    page(items, ids.last().copied().unwrap_or(0), has_more)
}
