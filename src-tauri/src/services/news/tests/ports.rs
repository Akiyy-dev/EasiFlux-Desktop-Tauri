use std::sync::Arc;

use crate::services::news::ports::NewsRepository;
use crate::storage::NewsDatabase;

use super::support::page;

#[test]
fn real_database_port_delegates_without_reimplementing_storage() {
    let directory = tempfile::tempdir().unwrap();
    let database: Arc<dyn NewsRepository> =
        Arc::new(NewsDatabase::open(directory.path().join("news.sqlite3")).unwrap());

    database.prepare_source("source-a").unwrap();
    let outcome = database.commit_page(&page(&[3, 8], false), 99).unwrap();
    let listed = database.list_messages(None, 50).unwrap();
    let seen = database.mark_seen(8).unwrap();

    assert_eq!(outcome.inserted_count, 2);
    assert_eq!(
        listed
            .items
            .iter()
            .map(|item| item.delivery_id.as_str())
            .collect::<Vec<_>>(),
        ["8", "3"]
    );
    assert_eq!(seen.latest_delivery_id.as_deref(), Some("8"));
}
