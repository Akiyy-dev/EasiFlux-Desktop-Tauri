use std::collections::HashMap;
use std::sync::{Arc, Barrier};

use crate::error::{AppError, AppResult};
use crate::models::account::CredentialState;
use crate::models::config::{ApiCredential, ConnectionStatus, DEFAULT_BASE_URL};

use super::super::{
    build_account_profiles, delete_account, list_account_profiles_transaction,
    normalize_account_ids, AccountLifecycleCoordinator, CredentialRepository,
};
use super::support::FakeLifecyclePort;

enum FakeCredential {
    Present(ApiCredential),
    Missing,
    Unavailable,
}

struct FakeCredentialRepository(HashMap<String, FakeCredential>);

impl CredentialRepository for FakeCredentialRepository {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>> {
        match self.0.get(account_id) {
            Some(FakeCredential::Present(value)) => Ok(Some(value.clone())),
            Some(FakeCredential::Unavailable) => Err(AppError::Config(
                "raw keyring failure must stay private".into(),
            )),
            Some(FakeCredential::Missing) | None => Ok(None),
        }
    }

    fn save(&self, _: &str, _: &ApiCredential) -> AppResult<()> {
        unreachable!()
    }

    fn delete(&self, _: &str) -> AppResult<()> {
        unreachable!()
    }
}

#[test]
fn normalizes_deduplicates_and_prepends_missing_active_account() {
    let ids = normalize_account_ids(
        &[" backup ".into(), "".into(), "backup".into()],
        " primary ",
    );
    assert_eq!(ids, vec!["primary", "backup"]);
}

#[test]
fn preserves_configured_order_when_active_account_is_already_present() {
    let ids = normalize_account_ids(
        &["primary".into(), "backup".into(), "spare".into()],
        "backup",
    );
    assert_eq!(ids, vec!["primary", "backup", "spare"]);
}

#[test]
fn profile_list_is_sanitized_and_isolates_keyring_failures() {
    let repository = FakeCredentialRepository(HashMap::from([
        (
            "primary".into(),
            FakeCredential::Present(ApiCredential {
                api_key: "never-serialize-key".into(),
                api_secret: "never-serialize-secret".into(),
                base_url: DEFAULT_BASE_URL.into(),
                label: "Main".into(),
            }),
        ),
        ("backup".into(), FakeCredential::Missing),
        ("broken".into(), FakeCredential::Unavailable),
    ]));
    let profiles = build_account_profiles(
        &["primary".into(), "backup".into(), "broken".into()],
        "primary",
        &repository,
    );
    assert_eq!(profiles[0].credential_state, CredentialState::Present);
    assert_eq!(profiles[1].credential_state, CredentialState::Missing);
    assert_eq!(profiles[2].credential_state, CredentialState::Unavailable);
    assert_eq!(profiles[1].label, "backup");
    assert_eq!(profiles[1].base_url, DEFAULT_BASE_URL);
    let serialized = serde_json::to_string(&profiles).unwrap();
    assert!(!serialized.contains("apiSecret"));
    assert!(!serialized.contains("apiKey"));
    assert!(!serialized.contains("raw keyring failure"));
}

#[test]
fn profile_list_quarantines_unsafe_loaded_base_url_without_exposing_it() {
    let repository = FakeCredentialRepository(HashMap::from([(
        "primary".into(),
        FakeCredential::Present(ApiCredential {
            api_key: "key".into(),
            api_secret: "secret".into(),
            base_url: "https://user:raw-secret@example.test/api?token=raw".into(),
            label: "Main".into(),
        }),
    )]));

    let profiles = build_account_profiles(&["primary".into()], "primary", &repository);
    let serialized = serde_json::to_string(&profiles).unwrap();

    assert_eq!(profiles[0].credential_state, CredentialState::Unavailable);
    assert_eq!(profiles[0].base_url, DEFAULT_BASE_URL);
    assert!(!serialized.contains("raw-secret"));
    assert!(!serialized.contains("token="));
    assert!(!serialized.contains("example.test"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_list_holds_transaction_guard_across_all_credential_reads() {
    let port = Arc::new(FakeLifecyclePort::new(ConnectionStatus::Disconnected));
    let coordinator = Arc::new(AccountLifecycleCoordinator::new());
    let load_started = Arc::new(Barrier::new(2));
    let release_load = Arc::new(Barrier::new(2));
    port.delay_profile_load("spare", load_started.clone(), release_load.clone());

    let list_task = tokio::spawn({
        let port = port.clone();
        let coordinator = coordinator.clone();
        async move { list_account_profiles_transaction(&coordinator, port.as_ref()).await }
    });
    tokio::task::spawn_blocking({
        let load_started = load_started.clone();
        move || load_started.wait()
    })
    .await
    .unwrap();
    let delete_task = tokio::spawn({
        let port = port.clone();
        let coordinator = coordinator.clone();
        async move { delete_account(&coordinator, port.as_ref(), "spare").await }
    });
    tokio::task::yield_now().await;
    tokio::task::spawn_blocking(move || release_load.wait())
        .await
        .unwrap();

    let profiles = list_task.await.unwrap();
    delete_task.await.unwrap().unwrap();
    let spare = profiles
        .iter()
        .find(|profile| profile.account_id == "spare")
        .unwrap();
    assert_eq!(spare.credential_state, CredentialState::Present);
}
