# PRD-13 Notification Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver an account-aware, persistent in-app notification center with actionable order, risk/account, connection/system notifications, configurable realtime Toasts, reliable read/delete/clear behavior, and no duplicate logging or Toast delivery.

**Architecture:** Rust is the sole authority for notification facts. A typed policy maps explicit business outcomes into safe records; `NotificationService` serializes copy-on-write mutations and commits an atomic JSON snapshot before emitting `notification:changed`. Vue owns only IPC adaptation, current-account caching, realtime Toast gating, and presentation. `TopBar` owns the popover open state, while `AppShell` remains the only navigation owner.

**Tech Stack:** Vue 3.5, TypeScript 5.6, Pinia 3, Naive UI 2, Vitest 4, Vue Test Utils 2, Tauri 2, Rust 2021, Tokio 1, Serde/serde_json, UUID 1, Chrono 0.4.

**Spec:** `docs/superpowers/specs/2026-08-23-notification-center-design.md`

## Global Constraints

- Implement only PRD-13 v1: in-app center plus in-app Toast. Do not add an OS notification plugin, SQLite, sound, DND, a history page, arbitrary URLs, or a new window.
- Rust owns IDs, content, severity, action, dedupe, persistence, scope validation, retention, and revision. Vue must never persist notifications in `localStorage` or construct trusted notification records.
- Persist only safe structured content. Never store API keys, secrets, signatures, raw server bodies, stacks, or arbitrary frontend error text.
- Scope is exactly `{ type: "global" } | { type: "account", accountId }`. A current view contains its Account partition plus Global; an absent account context is Global-only.
- IPC revisions are decimal strings. Never convert them to JavaScript `number`; event continuity is string equality on `previousRevision`.
- First-wave business producers are Account-scoped exactly as specified. The Global partition remains a fully tested storage/query/mutation protocol for future producers, but PRD-13 v1 does not invent a Global producer.
- `notification:changed` is emitted only after disk and memory commit. Only `Created` may carry a safe `toastCandidate`; historical queries, recovery, updates, removals, and resets never create Toasts.
- Opening the popover does not mark anything read. Read/delete/clear operations are committed by Rust before Vue changes visible state.
- Order snapshots only seed observer state. Only command-confirmed terminal results or Realtime `non-final -> final` transitions may create terminal-order notifications.
- Generic `error:occurred` and `log:entry` never create persistent notifications. A logical error must not append the same diagnostic twice or show the same Toast twice.
- Use a closed client bridge only for the two frontend-orchestrated account failures. It accepts controlled identifiers/enums, never title/body/severity/action.
- Preserve current chart-workspace flushing and the existing `NavigationTarget` boundary. Notification components emit the closed `NotificationUiAction` union; only `AppShell` maps it to navigation, and UI-only actions are never persisted.
- No new Cargo or NPM dependency is expected. Encode opaque cursors with an internal versioned codec using existing `serde_json` plus a small hex codec.
- Keep frontend `AppConfig` unchanged. Notification preferences use the independent `NotificationSettings` DTO/service/store so existing config fixtures do not gain a new required field.
- Use TDD for every behavior change: write the focused failing test, run it and confirm the expected failure, implement the minimum behavior, run the focused test green, then run the nearest regression group.
- Format only touched Rust files during task work. Run repository-wide format/clippy/build gates at the end and distinguish pre-existing failures from feature regressions.
- Do not stage `.superpowers/`, generated visual-companion files, or unrelated working-tree changes. Never use `git add -A`; stage only paths named by the current task.

---

### Task 1: Define the notification domain and narrow Toast settings model

**Files:**

- Create: `src-tauri/src/models/notification.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Modify: `src-tauri/src/models/config.rs`
- Modify: `src-tauri/src/storage/config.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Create: `src-tauri/src/commands/config/notification_settings.rs`
- Create: `src-tauri/src/commands/config/notification_settings/tests.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

- Produces the complete Rust serde contract for records, scopes, categories, kinds, severity, controlled content/scalars/actions, filters, opaque-cursor requests, pages, summaries, mutation results, client bridge requests, and change events.
- Produces `NotificationSettings`, `get_notification_settings`, and `update_notification_settings` without exposing or overwriting a full `AppConfig` snapshot.
- Extends `AppConfig` with `#[serde(default)] pub notification_settings: NotificationSettings`; all three Toast flags default to `true`.
- Extends the hand-written `TomlConfig` and both conversion directions so old files default safely and new values survive restart.

- [ ] **Step 1: Add failing model serialization and validation tests**

In `models/notification.rs`, start with tests that assert the exact camelCase/tagged-union wire shape and reject unsafe content:

```rust
#[test]
fn account_scope_and_action_use_tagged_camel_case_wire_shapes() {
    assert_eq!(
        serde_json::to_value(NotificationScope::Account { account_id: "alpha".into() }).unwrap(),
        json!({ "type": "account", "accountId": "alpha" }),
    );
    assert_eq!(
        serde_json::to_value(NotificationAction::OpenTrading { order_id: Some("o-1".into()) }).unwrap(),
        json!({ "type": "openTrading", "orderId": "o-1" }),
    );
}

#[test]
fn content_validation_rejects_non_finite_or_uncontrolled_values() {
    let content = NotificationContent::new(
        "risk.orderBlocked",
        [("limit", NotificationScalar::Number(f64::NAN))],
        "订单被风控拦截",
        "请检查风控设置",
    );
    assert_eq!(content.unwrap_err().code(), "INVALID_NOTIFICATION_CONTENT");
}
```

Also cover: blank account IDs, UUID text validation, `occurrenceCount == 0`, time ordering, JavaScript-safe timestamps, every controlled enum, and `toastCandidate` omission for non-Created changes.

- [ ] **Step 2: Run the focused model tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked models::notification::tests::
```

Expected: compilation fails because the notification model does not exist.

- [ ] **Step 3: Implement the domain contract and validation helpers**

Use tagged enums and a finite scalar type rather than `serde_json::Value`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NotificationScope {
    Global,
    Account { account_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotificationScalar {
    String(String),
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NotificationAction {
    OpenTrading { order_id: Option<String> },
    OpenAccountSettings { account_section: AccountNotificationSection },
    OpenGeneralSettings,
}
```

Define `NotificationRecord`, `NotificationInput`, `NotificationFilter`, `ListNotificationsRequest`, `NotificationPage`, `NotificationSummary`, `NotificationMutationResult`, `NotificationChange`, `NotificationChangedEvent`, and `NotificationToastCandidate`. Use `BTreeMap<String, NotificationScalar>` for deterministic output. Keep `revision`, `previous_revision`, and response revisions as `String` at the IPC edge.

- [ ] **Step 4: Add failing config-default and transactional-update tests**

Cover an old TOML config without `notification_settings`, the hand-written TOML round trip, narrow model conversion, and persistence failure retaining runtime state:

```rust
#[test]
fn notification_settings_default_all_toasts_to_enabled() {
    assert_eq!(
        NotificationSettings::default(),
        NotificationSettings {
            trading_toast: true,
            risk_account_toast: true,
            connection_system_toast: true,
        },
    );
}

```

In `storage/config.rs`, add `legacy_toml_defaults_notification_settings` using a literal legacy TOML string and `notification_settings_round_trip` using `ConfigStore::with_path`. In the command test, use the existing blocked-parent `ConfigStore` pattern and assert both the runtime value and last committed settings remain unchanged after the save error.

- [ ] **Step 5: Run settings tests RED, then implement the narrow commands**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::notification_settings::tests::
```

Implement `apply_notification_settings_update` with `run_serialized_account_mutation`: clone the latest runtime config, change only `notification_settings`, save it, then replace runtime state. Add `#[cfg(test)] mod tests;` in `notification_settings.rs`. Register:

```rust
#[tauri::command]
pub async fn get_notification_settings(
    state: State<'_, AppState>,
) -> AppResult<NotificationSettings>;

#[tauri::command]
pub async fn update_notification_settings(
    state: State<'_, AppState>,
    settings: NotificationSettings,
) -> AppResult<NotificationSettings>;
```

Update `merge_authoritative_config` so legacy `save_config` cannot overwrite the latest notification settings.

- [ ] **Step 6: Run focused and config regression tests GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked models::notification::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::notification_settings::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::tests::
```

- [ ] **Step 7: Format, review, and commit Task 1**

```powershell
rustfmt --edition 2021 src-tauri/src/models/notification.rs src-tauri/src/models/config.rs src-tauri/src/storage/config.rs src-tauri/src/commands/config.rs src-tauri/src/commands/config/notification_settings.rs src-tauri/src/commands/config/notification_settings/tests.rs src-tauri/src/models/mod.rs src-tauri/src/lib.rs
git diff --check
git add src-tauri/src/models/notification.rs src-tauri/src/models/mod.rs src-tauri/src/models/config.rs src-tauri/src/storage/config.rs src-tauri/src/commands/config.rs src-tauri/src/commands/config/notification_settings.rs src-tauri/src/commands/config/notification_settings/tests.rs src-tauri/src/lib.rs
git diff --cached --check
git commit -m "feat(notifications): define domain and toast settings"
```

---

### Task 2: Add the atomic schema-v1 notification store and recovery behavior

**Files:**

- Create: `src-tauri/src/storage/notification_store.rs`
- Create: `src-tauri/src/storage/notification_store/tests.rs`
- Modify: `src-tauri/src/storage/mod.rs`

**Interfaces:**

- Produces `NotificationFileV1 { schema_version, revision, source_event_index, partitions }`, internal `NotificationSourceEventIndexEntry { scope, source_event_id, notification_id }`, and `NotificationPartition` as the complete disk snapshot. The index uses `serde(default)` for legacy v1 compatibility and is never exposed through `NotificationRecord` or frontend DTOs.
- Produces `NotificationStore::new()`, test-only `with_path`, `load()`, and `save(&NotificationFileV1)`.
- Produces a crate-private `NotificationPersistence: Send + Sync` trait implemented by `NotificationStore`, allowing service tests to inject deterministic save failures without touching real user data.
- Loads candidates in `main -> tmp -> bak` order, distinguishes unsupported future schema from corruption, preserves corrupt evidence, and normalizes a recovered candidate without changing revision.

- [ ] **Step 1: Write failing round-trip, validation, and candidate-order tests**

Mirror the test organization of `chart_state_store/tests.rs`. Add named cases `round_trip_preserves_revision_partitions_and_records`, `load_prefers_main_then_tmp_then_backup` (write distinct revisions and assert the selected one), `future_main_schema_is_not_downgraded_to_an_older_backup`, and `all_corrupt_v1_candidates_are_preserved_and_empty_state_is_returned`. Also test duplicate partition scope, duplicate record ID across partitions, invalid timestamps/scalars/enums, main rotation, tmp promotion failure, backup restore failure, no-file startup, and recovered-main normalization.

- [ ] **Step 2: Run store tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked storage::notification_store::tests::
```

Expected: module/types are missing.

- [ ] **Step 3: Implement strict disk validation and atomic save**

Follow `ChartStateStore`'s filesystem discipline:

```rust
pub struct NotificationStore {
    path: PathBuf,
    write_lock: Mutex<()>,
}

pub(crate) trait NotificationPersistence: Send + Sync {
    fn save(&self, file: &NotificationFileV1) -> AppResult<()>;
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::with_path(
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(APP_NAME)
                .join("notifications")
                .join("notifications.v1.json"),
        )
    }

    pub fn load(&self) -> AppResult<NotificationLoadOutcome>;
    pub fn save(&self, file: &NotificationFileV1) -> AppResult<()>;
}
```

`save` must validate the complete copy, create the directory, write/sync `.tmp`, rotate current main to `.bak`, promote `.tmp`, and best-effort restore `.bak` after promotion failure. Map unavailable writes to stable code `NOTIFICATION_STORAGE_UNAVAILABLE` through the notification command/service error type; log only sanitized path/error-kind metadata.

- [ ] **Step 4: Implement corrupt/future-schema recovery semantics**

Return a typed outcome rather than silently flattening all cases:

```rust
pub enum NotificationLoadStatus {
    Clean,
    Recovered { source: RecoverySource },
    ResetFromCorruption,
    UnsupportedSchema { found: u32 },
}

pub struct NotificationLoadOutcome {
    pub file: NotificationFileV1,
    pub status: NotificationLoadStatus,
}
```

If a higher-priority parseable file has `schemaVersion > 1`, return the unsupported state and never overwrite/rename it. If all v1 candidates are corrupt, rename/copy them to timestamped `.corrupt-*` evidence and return an empty schema-v1 state. Recovery writes must not increment revision or emit Created.

- [ ] **Step 5: Run store tests GREEN and the chart-store regression**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked storage::notification_store::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked storage::chart_state_store::tests::
```

- [ ] **Step 6: Format, review, and commit Task 2**

```powershell
rustfmt --edition 2021 src-tauri/src/storage/notification_store.rs src-tauri/src/storage/notification_store/tests.rs src-tauri/src/storage/mod.rs
git diff --check
git add src-tauri/src/storage/notification_store.rs src-tauri/src/storage/notification_store/tests.rs src-tauri/src/storage/mod.rs
git diff --cached --check
git commit -m "feat(notifications): persist atomic notification snapshots"
```

---

### Task 3: Implement the authoritative notification service, policy, pagination, and retention

**Files:**

- Create: `src-tauri/src/services/notification.rs`
- Create: `src-tauri/src/services/notification/policy.rs`
- Create: `src-tauri/src/services/notification/tests/mod.rs`
- Create: `src-tauri/src/services/notification/tests/support.rs`
- Create: `src-tauri/src/services/notification/tests/policy.rs`
- Create: `src-tauri/src/services/notification/tests/service.rs`
- Create: `src-tauri/src/services/notification/tests/lifecycle.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/storage/notification_store.rs`
- Modify: `src-tauri/src/storage/notification_store/tests.rs`

**Task 3 schema ruling:** durable absolute idempotency after semantic merges requires retaining every observed `(scope, sourceEventId)`. Task 3 therefore adds the minimal internal `sourceEventIndex` extension above, backfills legacy records on service load, and removes index entries with their target record during delete/clear/prune/account cleanup. `NotificationRecord.sourceEventId` remains the stable creating source; history is not privately encoded into that frontend-facing scalar.

**Interfaces:**

- Produces a single serialized `NotificationService` for publish/query/read/delete/clear/prune/account-cleanup.
- Produces a controlled `NotificationPolicy` mapping each approved event kind to category, severity, content, action, source ID, and dedupe key.
- Produces opaque cursor codec `n1.<hex-json>` containing version, account/global query fingerprint, filter, and last `(createdAtMs,id)` key.
- Produces `observe_order`, `observe_connection`, `observe_environment`, and client-account policy inputs while keeping producer wiring for later tasks. Every first-wave policy record is Account-scoped; Global capability is verified through service fixtures because v1 intentionally has no Global producer.

- [ ] **Step 1: Write failing policy mapping and safety tests**

Cover every first-wave event and exact output. Example:

```rust
#[test]
fn risk_block_maps_to_controlled_warning_and_risk_settings_action() {
    let input = policy.risk_order_blocked(context("alpha", 4, "submission-1"), violation());
    assert_eq!(input.kind, NotificationKind::RiskOrderBlocked);
    assert_eq!(input.category, NotificationCategory::RiskAccount);
    assert_eq!(input.severity, NotificationSeverity::Warning);
    assert_eq!(input.action, Some(NotificationAction::OpenAccountSettings {
        account_section: AccountNotificationSection::Risk,
    }));
    assert!(!serde_json::to_string(&input).unwrap().contains("raw server body"));
}
```

Cover filled/canceled/rejected, session expired, recovery/reconciliation failure, API/WebSocket unavailable/recovered, and environment unavailable/recovered. Assert every first-wave producer requires explicit Account scope, Global fixtures still work through the service API, and no bridge accepts frontend display text/action.

- [ ] **Step 2: Write failing service tests for transactional mutation semantics**

Use an injected `NotificationPersistence` fake and an `emit_changed` collector. Cover:

- Same `(scope, sourceEventId)` is a no-op: no record mutation, revision, save, or event.
- Same semantic `dedupeKey` merges occurrence/update without resetting `readAtMs` and without a second Toast candidate.
- Save failure preserves old memory/revision and emits nothing.
- Concurrent publishes serialize and commit consecutive revisions.
- Only a newly Created record carries a Toast candidate.

- [ ] **Step 3: Run policy/service tests and confirm RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::policy::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::service::
```

- [ ] **Step 4: Implement clone-mutate-save-swap-emit**

Use one async commit mutex around the authoritative snapshot. Queue or synchronously deliver the event after save/swap but before releasing the commit gate, so revisions can never be observed out of order:

```rust
async fn commit<F>(&self, mutate: F) -> Result<CommitOutcome, NotificationError>
where
    F: FnOnce(&mut NotificationFileV1) -> Result<Mutation, NotificationError>,
{
    let mut guard = self.state.lock().await;
    let mut next = guard.clone();
    let mutation = mutate(&mut next)?;
    if mutation.is_noop() { return Ok(mutation.into_outcome(guard.revision)); }
    let previous = next.revision;
    next.revision = previous.checked_add(1).ok_or(NotificationError::RevisionExhausted)?;
    self.persistence.save(&next)?;
    *guard = next;
    let (outcome, event) = mutation.into_commit(previous, guard.revision);
    if let Some(event) = event { (self.emit_changed)(event); }
    Ok(outcome)
}
```

The callback is a documented synchronous, non-reentrant `AppHandle::emit` adapter and may not call back into `NotificationService`. A callback failure is diagnostic only and cannot roll back committed state. Add a two-publisher test that proves emitted `(previousRevision,revision)` pairs are ordered.

- [ ] **Step 5: Add failing query/cursor/visibility tests**

Cover Global-only, Account+Global merge, `(createdAtMs DESC,id DESC)`, All vs Unread, limits `1..=100` with default 50, unread count over all records, cursor query fingerprint, insertion between pages, scope mismatch, Global read visibility, and stale-account mutation rejection with `NOTIFICATION_SCOPE_MISMATCH`. Include named cases `opaque_cursor_cannot_cross_account_or_filter` and `current_account_view_merges_global_without_exposing_other_accounts`, each asserting both result IDs and the stable error code where applicable.

- [ ] **Step 6: Implement query/mutation APIs and opaque cursor codec**

Expose the spec-named APIs:

```rust
pub async fn list(&self, context: ViewContext, request: ListNotificationsRequest, now_ms: u64)
    -> Result<NotificationPage, NotificationError>;
pub async fn summary(&self, context: ViewContext, now_ms: u64)
    -> Result<NotificationSummary, NotificationError>;
pub async fn mark_read(&self, context: ViewContext, id: &str, now_ms: u64)
    -> Result<NotificationMutationResult, NotificationError>;
pub async fn mark_visible_read(&self, context: ViewContext, now_ms: u64)
    -> Result<NotificationMutationResult, NotificationError>;
pub async fn delete_visible(&self, context: ViewContext, id: &str, now_ms: u64)
    -> Result<NotificationMutationResult, NotificationError>;
pub async fn clear_account(&self, active_account_id: &str, requested_account_id: &str, now_ms: u64)
    -> Result<NotificationMutationResult, NotificationError>;
```

The cursor codec must parse only the internal `CursorV1`, validate its fingerprint against account/filter, and return stable `INVALID_NOTIFICATION_CURSOR` rather than exposing serde details.

- [ ] **Step 7: Add failing retention, incident-recovery, and orphan tests**

Cover 90 days, 1000 per partition including Global, oldest-first deterministic pruning, a single revision/Reset, query-time expiry filtering, 24-hour due checks, startup orphan account cleanup, and reconstruction of active incidents from stored notification history.

- [ ] **Step 8: Implement retention and edge-state observers**

Add:

```rust
pub async fn prune(&self, configured_accounts: &HashSet<String>, now_ms: u64)
    -> Result<PruneOutcome, NotificationError>;
pub async fn delete_account_partition(&self, account_id: &str, now_ms: u64)
    -> Result<(), NotificationError>;
pub async fn observe_connection(&self, observation: ConnectionObservation, now_ms: u64)
    -> Result<PublishOutcome, NotificationError>;
pub async fn observe_environment(&self, observation: EnvironmentObservation, now_ms: u64)
    -> Result<PublishOutcome, NotificationError>;
```

API, private WebSocket, and environment channels maintain independent incidents. Repeated unavailable callbacks are no-ops; recovery only closes a known incident; startup healthy may close a stored open incident; startup healthy without an incident creates nothing.

- [ ] **Step 9: Run all notification service tests GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked storage::notification_store::tests::
```

- [ ] **Step 10: Format, review, and commit Task 3**

```powershell
rustfmt --edition 2021 src-tauri/src/services/notification.rs src-tauri/src/services/notification/policy.rs src-tauri/src/services/notification/tests/mod.rs src-tauri/src/services/notification/tests/support.rs src-tauri/src/services/notification/tests/policy.rs src-tauri/src/services/notification/tests/service.rs src-tauri/src/services/notification/tests/lifecycle.rs src-tauri/src/services/mod.rs
git diff --check
git add src-tauri/src/services/notification.rs src-tauri/src/services/notification src-tauri/src/services/mod.rs
git diff --cached --check
git commit -m "feat(notifications): add authoritative notification service"
```

---

### Task 4: Expose notification commands, events, app lifecycle, and maintenance

**Files:**

- Create: `src-tauri/src/commands/notification.rs`
- Create: `src-tauri/src/commands/notification/tests.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/events/emitter.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/services/scheduler.rs`
- Modify: `src-tauri/src/services/scheduler/tests/mod.rs`
- Create: `src-tauri/src/services/scheduler/tests/notification_maintenance.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

- Adds the six user-facing query/mutation commands from the spec and registers them in `generate_handler!`; the closed client bridge is added only in Task 7 after its validation policy exists.
- Adds `EventEmitter::emit_notification_changed(&NotificationChangedEvent)` with event name `notification:changed`.
- Loads `NotificationRuntime::{Available,Unavailable}` during `AppState::new`, prunes orphans/expired records without preventing app startup, and schedules one `TaskId::NotificationMaintenance` 24-hour task with clean shutdown.
- Commands derive the authoritative active account and epoch from Rust; they never trust UI state implicitly.

- [ ] **Step 1: Write failing command-boundary tests**

Test parameter parsing and context rules independently of a real Tauri window. Add named cases `omitted_context_is_global_only_even_when_an_account_is_active`, `stale_account_context_returns_scope_mismatch`, and `revision_is_serialized_as_a_decimal_string`; assert the exact visible IDs/error code/JSON value in each case. Cover all command names/shapes, default/max limit, mutation results, affected scopes, current-account clear validation, and structured notification command errors.

- [ ] **Step 2: Run command tests RED, then implement thin commands**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::notification::tests::
```

Add `#[cfg(test)] mod tests;` to `commands/notification.rs`, then implement these six exact spec commands:

```rust
list_notifications
get_notification_summary
mark_notification_read
mark_visible_notifications_read
delete_notification
clear_account_notifications
```

Use a serializable `NotificationCommandError { code, message, notification_id }` for this command family. Do not globally change `AppError` serialization in this task.

- [ ] **Step 3: Add failing event-after-commit and startup-recovery tests**

Assert exact `previousRevision/revision/change/affectedScopes/notificationId/toastCandidate` JSON. Verify failed save emits nothing, recovered history emits no Created, unsupported schema leaves service unavailable without stopping the rest of `AppState`, and notification callback errors do not recurse through `emit_error`.

- [ ] **Step 4: Wire `AppState`, emitter, commands, and startup status**

Construct a non-fatal runtime before producers that consume it:

```rust
let notification = Arc::new(match NotificationService::load(
    NotificationStore::new(),
    &loaded.accounts,
    now_ms,
    notification_emitter,
) {
    Ok(service) => NotificationRuntime::Available(Arc::new(service)),
    Err(error) => {
        tracing::warn!(code = error.code(), "notification center unavailable at startup");
        NotificationRuntime::Unavailable(error.availability())
    }
});
```

All-corrupt v1 candidates recover to an available empty service; future schema and unreadable/unavailable storage degrade only notification commands/producers. Commands return a stable availability error; producers no-op with sanitized tracing. Do not make the desktop app fail to launch.

- [ ] **Step 5: Write RED maintenance tests, then add the 24-hour task**

Use paused Tokio time to prove one prune per period, no duplicate loop after scheduler start, and shutdown cancellation. Add `TaskId::NotificationMaintenance` to `TaskId::all`, `configured_task_interval`, runtime-state registration, periodic spawn, and stop coverage. `SchedulerService::new` receives `Arc<NotificationRuntime>` and the shared runtime config; the task snapshots configured account IDs, calls `notification.prune(configured_accounts, now_ms)` when available, and logs sanitized failures without creating a notification.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::notification_maintenance::
```

- [ ] **Step 6: Run focused integration tests GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::notification::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::notification_maintenance::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::
```

- [ ] **Step 7: Format, review, and commit Task 4**

```powershell
rustfmt --edition 2021 src-tauri/src/commands/notification.rs src-tauri/src/commands/notification/tests.rs src-tauri/src/commands/mod.rs src-tauri/src/events/emitter.rs src-tauri/src/state.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/notification_maintenance.rs src-tauri/src/lib.rs
git diff --check
git add src-tauri/src/commands/notification.rs src-tauri/src/commands/notification src-tauri/src/commands/mod.rs src-tauri/src/events/emitter.rs src-tauri/src/state.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/notification_maintenance.rs src-tauri/src/lib.rs
git diff --cached --check
git commit -m "feat(notifications): expose notification IPC and maintenance"
```

---

### Task 5: Connect order terminal outcomes and structured risk violations

**Files:**

- Modify: `src-tauri/src/models/risk.rs`
- Modify: `src-tauri/src/models/trading.rs`
- Modify: `src-tauri/src/services/risk.rs`
- Modify: `src-tauri/src/services/risk/validation.rs`
- Modify: `src-tauri/src/services/risk/tests/order_validation.rs`
- Modify: `src-tauri/src/services/risk/tests/usage_mutation.rs`
- Modify: `src-tauri/src/services/trading.rs`
- Create: `src-tauri/src/services/trading/tests/notification_observer.rs`
- Modify: `src-tauri/src/commands/trading/mutations.rs`
- Modify: `src-tauri/src/ws/manager.rs`
- Modify: `src-tauri/src/state.rs`

**Interfaces:**

- Produces `SubmissionContext { submission_id, account_id, session_epoch }`, generated by Rust at the `place_order` boundary and threaded through risk and API submission.
- Produces `RiskViolation { code, safe_params }`; user-facing `AppError::Risk` remains available, but notification policy consumes the typed violation rather than parsing text.
- Produces `OrderObservationOrigin::{Command,Realtime,Snapshot}` and an observer that creates only valid terminal transitions.
- Produces `TradingFailureKind::Rejected` from controlled API response codes/status, distinct from connection/internal/ambiguous failures; the submission error branch can therefore create rejection notifications without an `Order` response.

- [ ] **Step 1: Write failing structured-risk tests for every rule**

Assert typed codes for invalid quantity, non-positive quantity, max order quantity, missing/invalid/non-positive limit price, price deviation, daily order limit, and ledger unavailable. Assert `safe_params` is finite and contains no raw request/server data.

```rust
assert_eq!(
    validate_order(&request, &config).unwrap_err().code,
    RiskViolationCode::MaxOrderQty,
);
```

- [ ] **Step 2: Run risk tests RED, then implement typed violations**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::risk::tests::order_validation::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::risk::tests::usage_mutation::
```

Keep an explicit conversion `impl From<RiskViolation> for AppError`; do not infer codes from localized strings anywhere.

- [ ] **Step 3: Write failing order-observer tests**

Cover:

- Snapshot terminal orders seed state and create nothing.
- Command terminal results create exactly once.
- Realtime `New/PartiallyFilled -> Filled|Cancelled|Rejected` creates exactly once.
- Duplicate REST/WS representations are idempotent.
- Final-to-final mismatch logs a diagnostic but creates no second record.
- Partially filled and optimistic/pending states never persist notifications.
- API-final rejection without an order ID dedupes by `submissionId`.

- [ ] **Step 4: Run trading tests RED, then implement submission context and observer**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::trading::tests::notification_observer::
```

Generate `Uuid::new_v4().to_string()` when `place_order` enters Rust. Pass explicit account/epoch captured under the account lifecycle read guard. Add `mod notification_observer;` inside the existing inline `services::trading::tests` module and put the new cases in `tests/notification_observer.rs`. Call the same observer from command side effects, WebSocket order parsing, and `refresh_orders` with the correct origin. In `execute_reserved_order`, handle `submit().await` with a typed `TradingFailureKind::Rejected` branch that publishes by `submissionId`; do not depend on receiving an `Order`. Do not create notifications in the Vue `order:updated` listener.

- [ ] **Step 5: Make notification-aware command errors explicit**

When a risk block or confirmed trading rejection creates a persistent notification, return the stable structured command error with `notificationId`. Connection/internal/ambiguous failures remain ordinary command errors and must not invent a rejected notification.

- [ ] **Step 6: Run focused and nearest regressions GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::risk::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::trading::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::trading::
cargo test --manifest-path src-tauri/Cargo.toml --locked ws::
pnpm test -- tests/frontend/risk.test.ts tests/frontend/riskPanel.test.ts tests/frontend/tradingSwitchGuard.test.ts
```

- [ ] **Step 7: Format, review, and commit Task 5**

```powershell
rustfmt --edition 2021 src-tauri/src/models/risk.rs src-tauri/src/models/trading.rs src-tauri/src/services/risk.rs src-tauri/src/services/risk/validation.rs src-tauri/src/services/risk/tests/order_validation.rs src-tauri/src/services/risk/tests/usage_mutation.rs src-tauri/src/services/trading.rs src-tauri/src/services/trading/tests/notification_observer.rs src-tauri/src/commands/trading/mutations.rs src-tauri/src/ws/manager.rs src-tauri/src/state.rs
git diff --check
git add src-tauri/src/models/risk.rs src-tauri/src/models/trading.rs src-tauri/src/services/risk.rs src-tauri/src/services/risk src-tauri/src/services/trading.rs src-tauri/src/services/trading src-tauri/src/commands/trading/mutations.rs src-tauri/src/ws/manager.rs src-tauri/src/state.rs
git diff --cached --check
git commit -m "feat(notifications): observe order and risk outcomes"
```

---

### Task 6: Connect account-scoped connection, environment, and session edges

**Files:**

- Modify: `src-tauri/src/api/client.rs`
- Modify: `src-tauri/src/api/response.rs`
- Modify: `src-tauri/src/services/connection.rs`
- Create: `src-tauri/src/services/connection/tests.rs`
- Modify: `src-tauri/src/ws/manager.rs`
- Modify: `src-tauri/src/events/emitter.rs`
- Modify: `src-tauri/src/services/scheduler.rs`
- Modify: `src-tauri/src/services/scheduler/tests/mod.rs`
- Create: `src-tauri/src/services/scheduler/tests/environment_notifications.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src/types/models.ts`
- Modify: `src/composables/useAccountSessionEvent.ts`
- Modify: `tests/frontend/accountEventEpoch.test.ts`
- Modify: `tests/frontend/appAccountEventHandlers.test.ts`

**Interfaces:**

- Feeds `NotificationService::observe_connection` with explicit account ID, session epoch, channel, and normalized state at the authoritative state-write points.
- Feeds `observe_environment` after a confirmed environment probe result, scoped to the account/config snapshot that initiated the probe.
- Produces `SessionContext { account_id, session_epoch }` once at connection/WS start and passes that immutable context through spawned workers, order/connection observers, and account event envelopes.
- Adds `accountId` to `AccountSessionEvent<T>` and rejects both old-epoch and wrong-account payloads in Vue; never lets a late worker infer ownership from the current account.
- Produces controlled `AuthFailureKind::SessionExpired` from known HTTP/API authentication codes, distinct from missing local credentials, Keyring failure, timestamp retry, and generic signing/config errors.
- If a connect/reconnect failure also commits an incident notification, the command returns controlled `{ code, message, notificationId }`; it does not also call Rust `emit_error`. Ordinary connection failures without a committed notification remain caller-owned errors.

- [ ] **Step 1: Write failing channel-edge tests**

Test API and private WebSocket independently:

```rust
healthy -> unavailable      // one unavailable
unavailable -> unavailable  // no-op
unavailable -> healthy      // one recovered, same incident
healthy -> healthy          // no-op
```

Also test old epoch rejection, `Connecting`/user-requested `Disconnected` suppression, WebSocket public-only noise suppression, and one channel recovery not closing another incident. Add response classification tests for known session-invalid HTTP/API codes. Only `AuthFailureKind::SessionExpired` may create `account.session_expired`; local credential absence/corruption and transient timestamp/sign retry paths must not.

- [ ] **Step 2: Run connection tests RED, then inject the observer**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::connection::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::lifecycle::
```

`ConnectionService::connect` captures `SessionContext` under the lifecycle read guard and passes it to `WsManager::start`. Change spawned public/private session functions and the message handler to accept this value explicitly; use it for `emit_order`, the order observer, and channel observers. Do not read config/coordinator after an async operation completes to determine ownership. Wire `observe_session_expired(context)` only after controlled private API response classification is complete and dedupe it once per effective session edge.

- [ ] **Step 3: Write failing environment-edge tests**

Cover unavailable/recovered cycles, repeated probe results, old account/epoch late completions, independent environment key (`baseUrl` normalized to a safe label/key), and no raw probe error in stored content.

- [ ] **Step 4: Run environment tests RED, then wire the scheduler/probe path**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::environment_notifications::
```

The environment observer receives the account/config snapshot used for the probe; it must not use the UI's active account at completion time.

Extend the Rust `AccountSessionEvent` wire envelope and TypeScript `AccountSessionEvent<T>` with `accountId`. Update `useAccountSessionEvent` to require both the authoritative active account and accepted epoch before invoking its handler; add wrong-account/late-epoch frontend regressions.

- [ ] **Step 5: Verify diagnostics and notifications do not recursively report one another**

Add a regression asserting a notification persistence failure writes a sanitized tracing/log diagnostic only and never calls `publish`, `emit_error`, or the notification Toast channel recursively.

- [ ] **Step 6: Run focused regressions GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::connection::tests::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::environment_notifications::
cargo test --manifest-path src-tauri/Cargo.toml --locked services::notification::tests::lifecycle::
cargo test --manifest-path src-tauri/Cargo.toml --locked ws::
pnpm test -- tests/frontend/accountEventEpoch.test.ts tests/frontend/appAccountEventHandlers.test.ts
```

- [ ] **Step 7: Format, review, and commit Task 6**

```powershell
rustfmt --edition 2021 src-tauri/src/api/client.rs src-tauri/src/api/response.rs src-tauri/src/services/connection.rs src-tauri/src/services/connection/tests.rs src-tauri/src/ws/manager.rs src-tauri/src/events/emitter.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/environment_notifications.rs src-tauri/src/state.rs
git diff --check
git add src-tauri/src/api/client.rs src-tauri/src/api/response.rs src-tauri/src/services/connection.rs src-tauri/src/services/connection src-tauri/src/ws/manager.rs src-tauri/src/events/emitter.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/environment_notifications.rs src-tauri/src/state.rs src/types/models.ts src/composables/useAccountSessionEvent.ts tests/frontend/accountEventEpoch.test.ts tests/frontend/appAccountEventHandlers.test.ts
git diff --cached --check
git commit -m "feat(notifications): observe connection and environment incidents"
```

---

### Task 7: Add the closed account-failure bridge and best-effort partition cleanup

**Files:**

- Modify: `src-tauri/src/models/notification.rs`
- Modify: `src-tauri/src/commands/notification.rs`
- Modify: `src-tauri/src/commands/notification/tests.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/services/account_profiles.rs`
- Modify: `src-tauri/src/services/account_profiles/mutations.rs`
- Modify: `src-tauri/src/services/account_profiles/tests/support.rs`
- Modify: `src-tauri/src/services/account_profiles/tests/mutations.rs`
- Modify: `src-tauri/src/commands/account_profiles.rs`
- Create: `src/types/notification.ts`
- Create: `src/services/notificationService.ts`
- Modify: `src/stores/accountProfiles.ts`
- Modify: `src/services/accountReconciliationService.ts`
- Modify: `src/types/models.ts`
- Modify: `tests/frontend/accountSwitchReconciliation.test.ts`
- Modify: `tests/frontend/accountReconciliationSerialization.test.ts`
- Modify: `tests/frontend/accountProfilesConcurrency.test.ts`
- Modify: `tests/frontend/accountProfilesPanelFailures.test.ts`
- Modify: `tests/frontend/accountProfilesReconnect.test.ts`

**Interfaces:**

- `create_client_notification` accepts only `AccountRecoveryFailed | AccountReconciliationFailed`, current `accountId`, captured `sessionEpoch`, unique `attemptId`, and controlled `Config|Profiles|Connection|Bootstrap` steps.
- Rust validates active account/epoch and derives all persisted/display fields.
- `delete_account` returns `DeleteAccountResult { notificationCleanupPending, warningCode? }`; only `NOTIFICATION_CLEANUP_PENDING` is allowed. Notification cleanup failure never rolls back the committed account deletion.
- Frontend `deleteAccount()` consumes and returns the typed result (or records a typed `lastDeleteWarning`); it never parses a warning from an error string.

- [ ] **Step 1: Write failing Rust bridge validation tests**

Cover allowed kinds, non-empty unique failed steps, attempt ID shape/length, active account, current epoch, late request rejection, and inability to submit title/body/severity/action.

- [ ] **Step 2: Run bridge tests RED, then implement the closed mapping**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::notification::tests::client_bridge_
```

The command constructs `NotificationInput` through policy methods only. A repeated `(scope,attemptId,kind)` is idempotent. Register `create_client_notification` in `lib.rs` only now, after the closed DTO/policy is complete.

- [ ] **Step 3: Write failing account-deletion lifecycle tests**

Assert:

- Successful deletion awaits partition cleanup inside the mutation guard.
- Cleanup success returns no warning.
- Cleanup failure returns pending/warning but keeps credentials/config/runtime deletion committed.
- A later startup orphan prune deletes Account partitions absent from config but never Global.
- Concurrent switch/update cannot interleave through the cleanup boundary.

- [ ] **Step 4: Run lifecycle tests RED, then extend the lifecycle port/result**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::account_profiles::tests::mutations::
```

Add a notification-partition cleanup port to the account profile service seam. Log a sanitized warning on failure and return the typed result; do not restore credentials or config after the account deletion has committed.

- [ ] **Step 5: Write failing frontend reconciliation/late-response tests**

Create one `attemptId` per recovery/reconciliation run and capture `accountId/sessionEpoch` before tasks start. Verify the bridge receives controlled failed-step enums; a late result after account switch is ignored/rejected; bridge success suppresses the old aggregate generic Toast.

```powershell
pnpm test -- tests/frontend/accountSwitchReconciliation.test.ts tests/frontend/accountReconciliationSerialization.test.ts tests/frontend/accountProfilesConcurrency.test.ts
```

- [ ] **Step 6: Implement frontend bridge calls and cleanup warnings**

Create the minimal closed DTO and `createClientNotification` wrapper in `src/types/notification.ts` and `notificationService.ts`; Task 8 will expand those files to the full notification IPC surface. Use that typed service call rather than direct `invoke` in the store. Preserve existing inline reconciliation state. Change the delete command wrapper/store contract to `Promise<DeleteAccountResult>` (or a state-backed equivalent) and display `NOTIFICATION_CLEANUP_PENDING` as a non-blocking warning after successful account deletion; do not claim the deletion failed.

- [ ] **Step 7: Run Rust and frontend regressions GREEN**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::notification::tests::client_bridge_
cargo test --manifest-path src-tauri/Cargo.toml --locked services::account_profiles::tests::
pnpm test -- tests/frontend/accountSwitchReconciliation.test.ts tests/frontend/accountReconciliationSerialization.test.ts tests/frontend/accountProfilesConcurrency.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/accountProfilesPanelFailures.test.ts tests/frontend/accountProfilesReconnect.test.ts
```

- [ ] **Step 8: Format, review, and commit Task 7**

```powershell
rustfmt --edition 2021 src-tauri/src/models/notification.rs src-tauri/src/commands/notification.rs src-tauri/src/commands/notification/tests.rs src-tauri/src/services/account_profiles.rs src-tauri/src/services/account_profiles/mutations.rs src-tauri/src/services/account_profiles/tests/support.rs src-tauri/src/services/account_profiles/tests/mutations.rs src-tauri/src/commands/account_profiles.rs src-tauri/src/lib.rs
pnpm lint
git diff --check
git add src-tauri/src/models/notification.rs src-tauri/src/commands/notification.rs src-tauri/src/commands/notification src-tauri/src/services/account_profiles.rs src-tauri/src/services/account_profiles src-tauri/src/commands/account_profiles.rs src-tauri/src/lib.rs src/types/notification.ts src/services/notificationService.ts src/stores/accountProfiles.ts src/services/accountReconciliationService.ts src/types/models.ts tests/frontend/accountSwitchReconciliation.test.ts tests/frontend/accountReconciliationSerialization.test.ts tests/frontend/accountProfilesConcurrency.test.ts tests/frontend/accountProfilesPanelFailures.test.ts tests/frontend/accountProfilesReconnect.test.ts
git diff --cached --check
git commit -m "feat(notifications): bridge account failures and cleanup"
```

---

### Task 8: Add TypeScript notification contracts, IPC adapter, and dedicated Toast delivery

**Files:**

- Modify: `src/types/notification.ts`
- Modify: `src/services/notificationService.ts`
- Create: `src/services/notificationToastService.ts`
- Create: `tests/frontend/notificationService.test.ts`
- Create: `tests/frontend/notificationToastService.test.ts`
- Modify: `src/services/errorService.ts`
- Modify: `src/components/common/ErrorToastBridge.vue`
- Modify: `src-tauri/src/events/emitter.rs`
- Modify: `src/types/models.ts`
- Modify: `src/App.vue`
- Create: `tests/frontend/errorDeliveryOwnership.test.ts`
- Modify: `tests/frontend/appAccountEventHandlers.test.ts`

**Interfaces:**

- Mirrors all Rust DTOs as discriminated TypeScript unions without widening action/scope/kind to arbitrary strings.
- Wraps every notification command with `useTauriCommand`; listener uses `@tauri-apps/api/event.listen` directly and returns `UnlistenFn`.
- Provides `shouldToast(candidate, settings, activeAccountId)` and `showNotificationToast(candidate)` with no log write.
- Splits ordinary frontend errors from already-logged backend events.
- Keeps notification settings out of frontend `AppConfig`; they live only in the independent notification DTO/store.
- Produces `decodeCommandError` for controlled `{ code, message, eventId?, notificationId? }` errors. Callers branch on typed fields, never message text.

- [ ] **Step 1: Write failing service-contract tests**

Mock `invoke` and `listen`; assert exact camelCase command arguments, response pass-through, nullable account context, default limit ownership, and unlisten teardown:

```ts
expect(invoke).toHaveBeenCalledWith('list_notifications', {
  request: { accountId: 'alpha', filter: 'all', cursor: undefined, limit: 50 },
})
```

Assert `NotificationChangedEvent` uses `previousRevision` and `revision` strings and Created candidates can be consumed without a list refetch.

Add decoder cases for structured and legacy string command errors. Structured markers remain optional for ordinary commands, but `notificationId` and `eventId` must round-trip exactly.

- [ ] **Step 2: Run service tests RED, then implement types and IPC adapter**

```powershell
pnpm vitest run tests/frontend/notificationService.test.ts
```

Do not use `useTauriEvent` for the singleton store listener because it binds component lifecycle and does not expose an explicit stop handle.

- [ ] **Step 3: Write failing Toast-gating and dedupe tests**

Cover category settings, current Account plus Global, inactive account suppression, Critical still obeying its category toggle, Created-only behavior, `(revision,id)` process dedupe, no focus mutation, and no log-store calls.

Also test the provider bridge: before injection `showNotificationToast` is a safe no-op; after `installNotificationToastApi(message)` it uses the expected severity method and still never touches `useLogStore`.

- [ ] **Step 4: Run Toast tests RED, then implement the adapter**

```powershell
pnpm vitest run tests/frontend/notificationToastService.test.ts
```

Map `success/info/warning/error/critical` deterministically to the existing message API; Critical may use error styling but remains its own persisted severity. Export `installNotificationToastApi(message: MessageApi)` and have the existing `ErrorToastBridge` inject the same provider into both generic-error and notification adapters.

- [ ] **Step 5: Write failing error-ownership regression tests**

Prove:

- `reportFrontendError` appends one frontend log and shows one Toast.
- `showBackendError` shows one Toast and does not append a log already emitted by Rust.
- Backend `eventId` replay shows once.
- Notification Toast never uses either generic error path.

- [ ] **Step 6: Implement the explicit error APIs and event envelope**

Change backend errors from naked strings to a typed event `{ eventId, message }`; have `App.vue` call `showBackendError`. Keep a compatibility `reportError` export delegating to `reportFrontendError` for existing command/UI callers until they are migrated. Rust `emit_error` generates one event ID and sends it in both the error envelope and the corresponding `log:entry`; add optional `eventId` to the TypeScript `LogEntry` DTO and extend emitter serialization tests.

- [ ] **Step 7: Run focused and app-event regressions GREEN**

```powershell
pnpm vitest run tests/frontend/notificationService.test.ts tests/frontend/notificationToastService.test.ts tests/frontend/errorDeliveryOwnership.test.ts
pnpm test -- tests/frontend/appAccountEventHandlers.test.ts tests/frontend/accountEventEpoch.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --locked events::emitter::tests::
```

- [ ] **Step 8: Lint, review, and commit Task 8**

```powershell
pnpm lint
rustfmt --edition 2021 src-tauri/src/events/emitter.rs
git diff --check
cargo test --manifest-path src-tauri/Cargo.toml --locked events::emitter::tests::
git add src/types/notification.ts src/services/notificationService.ts src/services/notificationToastService.ts tests/frontend/notificationService.test.ts tests/frontend/notificationToastService.test.ts src/services/errorService.ts src/components/common/ErrorToastBridge.vue src-tauri/src/events/emitter.rs src/types/models.ts src/App.vue tests/frontend/errorDeliveryOwnership.test.ts tests/frontend/appAccountEventHandlers.test.ts
git diff --cached --check
git commit -m "feat(notifications): add frontend notification adapters"
```

---

### Task 9: Implement the account-aware Pinia notification store

**Files:**

- Create: `src/stores/notification.ts`
- Create: `tests/frontend/notificationStore.test.ts`
- Modify: `src/App.vue`

**Interfaces:**

- State: `accountId`, `filter`, `items`, `nextCursor`, `unreadCount`, `observedRevision`, `initialLoading`, `loadingMore`, `error`, `pageError`, `loadGeneration`, `settingsDraft`, `settingsCommitted`, and settings save status.
- Actions: `start`, `stop`, `setAccount`, `open`, `reload`, `loadMore`, `setFilter`, `markRead`, `markAllRead`, `remove`, `clearCurrentAccount`, `loadSettings`, and `updateSettings`.
- Listener registers before initial summary/settings loads; start/stop are idempotent.

- [ ] **Step 1: Write failing initialization and lifecycle tests**

Cover one listener across repeated `start`, one unlisten across repeated `stop`, listener-first initialization order, Global-only startup, settings readiness before Toast, and App mount/unmount ownership.

- [ ] **Step 2: Write failing account-generation and pagination tests**

Cover initial 50, opaque cursor forwarding, next-page append/dedupe, page error retaining existing items/cursor, filter reset, rapid account switch ignoring late responses, and cached view reset on context change.

- [ ] **Step 3: Write failing revision-event tests**

Cover:

- Continuous `previousRevision === observedRevision` reload behavior.
- Unrelated account events advance observed revision but do not alter current list/Toast.
- Revision jump or Reset clears items/cursor and reloads page one.
- Created candidate for current Account/Global is Toasted once.
- Initialization/reload/history never Toasts.
- A listener callback captures scope/account/generation before awaiting.

- [ ] **Step 4: Run store tests RED**

```powershell
pnpm vitest run tests/frontend/notificationStore.test.ts
```

- [ ] **Step 5: Implement store state machine and authoritative mutations**

Do not perform irreversible optimistic updates. On a successful command, set returned revision/unread count and either update the returned record or reload. On failure, keep cached data and expose a local non-blocking error.

Use lexically captured generation:

```ts
const generation = ++loadGeneration.value
const page = await listNotifications({
  accountId: accountId.value,
  filter: filter.value,
  cursor: undefined,
  limit: 50,
})
if (generation !== loadGeneration.value) return
items.value = page.items
```

Use `settingsCommitted` for realtime Toast decisions while a draft save is pending or failed.

- [ ] **Step 6: Start the store once from `App.vue`**

After listeners are ready, call `notificationStore.start()` before waiting on account/config startup so Global events are not missed. Feed the authoritative account after `refreshProfiles`; call `stop()` from `onBeforeUnmount`. Store watcher/explicit setter handles future account changes.

- [ ] **Step 7: Run store and startup regressions GREEN**

```powershell
pnpm vitest run tests/frontend/notificationStore.test.ts
pnpm test -- tests/frontend/appAccountEventHandlers.test.ts tests/frontend/appQuickSetup.test.ts tests/frontend/accountProfilesConcurrency.test.ts
```

- [ ] **Step 8: Lint, review, and commit Task 9**

```powershell
pnpm lint
git diff --check
git add src/stores/notification.ts tests/frontend/notificationStore.test.ts src/App.vue
git diff --cached --check
git commit -m "feat(notifications): add account-aware notification store"
```

---

### Task 10: Build notification items, list states, popover, badge, and navigation

**Files:**

- Create: `src/components/notifications/NotificationItem.vue`
- Create: `src/components/notifications/NotificationList.vue`
- Create: `src/components/notifications/NotificationPopover.vue`
- Create: `src/components/notifications/NotificationPopover.css`
- Modify: `src/components/layout/TopBar.vue`
- Modify: `src/components/layout/AppShell.vue`
- Create: `tests/frontend/notificationItem.test.ts`
- Create: `tests/frontend/notificationPopover.test.ts`
- Create: `tests/frontend/notificationNavigation.test.ts`

**Interfaces:**

- `NotificationItem` receives one record and pending flags; emits `activate(record)` and `delete(id)` from sibling buttons inside an `article`. It never imports the Store or invokes IPC.
- `NotificationList` receives items/loading/error/has-more/filter and emits semantic filter/pagination/retry/mark-all events.
- `NotificationPopover` owns dialog behavior and consumes the Pinia store; emits only controlled `navigate(NotificationUiAction)` and `update:show`. `NotificationUiAction` is the persisted `NotificationAction` union plus UI-only `{ type: 'openNotificationSettings' }`; the latter is never written to disk.
- `TopBar` owns local open state and forwards actions; `AppShell` maps them to existing object-form navigation targets.

- [ ] **Step 1: Write failing item semantics/accessibility tests**

Assert no nested interactive element, unread semantics beyond color, Enter/Space activation, Delete only from the primary item control, sibling delete behavior, and a single typed activation event. Read/action ordering belongs to Popover tests because Item has no Store dependency.

- [ ] **Step 2: Run item tests RED, then implement `NotificationItem`**

```powershell
pnpm vitest run tests/frontend/notificationItem.test.ts
```

Root markup shape:

```vue
<article :data-unread="notification.readAtMs == null">
  <button class="notification-item__primary" @click="emit('activate', notification)">
    <span>{{ notification.content.fallbackTitle }}</span>
    <span>{{ notification.content.fallbackBody }}</span>
  </button>
  <button class="notification-item__delete" aria-label="删除通知" @click="emit('delete', notification.id)">
    删除
  </button>
</article>
```

`NotificationPopover` handles activation: await `store.markRead(id)`, surface a non-blocking failure if needed, and in either case emit the record's whitelist action. This keeps IPC out of the presentational item while satisfying “mark failure does not block navigation.”

- [ ] **Step 3: Write failing list/popover/badge tests**

Cover skeleton, distinct All/Unread empty states, first-load error, page-tail error/retry, today/earlier grouping, load-more, mark-all, badge hidden/1/99/99+, accessible bell label, Esc/outside close, focus return, `aria-live=polite`, settings deep link, and current Account+Global footer copy.

- [ ] **Step 4: Run popover tests RED, then implement list/popover/TopBar**

```powershell
pnpm vitest run tests/frontend/notificationPopover.test.ts
```

Use an `NPopover`/controlled stub seam; width is `min(400px, calc(100vw - 24px))`, list scrolls independently, and the dialog is non-modal with `aria-modal="false"`. Opening calls `store.open()` but never read mutations.

- [ ] **Step 5: Write failing navigation mapping tests**

Cover `OpenTrading`, account API/risk settings, general settings, and UI-only `OpenNotificationSettings`. Assert the existing chart workspace flush is still invoked before navigation.

- [ ] **Step 6: Implement the AppShell mapping and focus target**

```ts
function handleNotificationAction(action: NotificationUiAction): Promise<void> {
  switch (action.type) {
    case 'openTrading': return navigateTo({ page: 'trading' })
    case 'openAccountSettings': return navigateTo({
      page: 'settings', settingsSection: 'account', accountSection: action.accountSection,
    })
    case 'openGeneralSettings': return navigateTo({ page: 'settings', settingsSection: 'general' })
    case 'openNotificationSettings': return navigateTo({
      page: 'settings', settingsSection: 'notifications',
    })
  }
}
```

The footer emits `{ type: 'openNotificationSettings' }`, which resolves to the object-form settings target; it must not degrade to the string `'settings'` or become a persisted record action.

- [ ] **Step 7: Run UI and settings-shell regressions GREEN**

```powershell
pnpm vitest run tests/frontend/notificationItem.test.ts tests/frontend/notificationPopover.test.ts tests/frontend/notificationNavigation.test.ts
pnpm test -- tests/frontend/settingsCenter.test.ts tests/frontend/chartWorkspaceSwitch.test.ts
```

- [ ] **Step 8: Lint, build, visually review, and commit Task 10**

```powershell
pnpm lint
pnpm build
git diff --check
git add src/components/notifications src/components/layout/TopBar.vue src/components/layout/AppShell.vue tests/frontend/notificationItem.test.ts tests/frontend/notificationPopover.test.ts tests/frontend/notificationNavigation.test.ts
git diff --cached --check
git commit -m "feat(notifications): add notification center popover"
```

---

### Task 11: Implement Notification Settings and clear-current-account flow

**Files:**

- Create: `src/components/settings/NotificationSettingsPanel.vue`
- Create: `src/components/settings/NotificationSettingsPanel.css`
- Modify: `src/components/settings/SettingsCenterPage.vue`
- Modify: `src/components/settings/settingsSections.ts`
- Create: `tests/frontend/notificationSettings.test.ts`
- Modify: `tests/frontend/settingsCenter.test.ts`

**Interfaces:**

- Three global auto-save switches: Trading Toast, Risk & Account Toast, Connection & System Toast.
- Draft and last-confirmed settings are separate. Realtime Toast gating uses only the last Rust-confirmed snapshot.
- Clear-current-account displays the account identity, requires `AppDialog` confirmation, calls the Store, and never deletes Global notifications.

- [ ] **Step 1: Write failing settings load/save tests**

Cover default/loaded values, one narrow update command, serial auto-save behavior, saving state, failure retaining draft plus committed snapshot, retry, and a later successful save updating Toast decisions.

- [ ] **Step 2: Write failing clear confirmation tests**

Cover account name/ID, disabled action without an account, open/cancel/confirm, successful clear, failure retaining UI state, no direct list mutation, and explicit text that Global notifications remain.

- [ ] **Step 3: Run settings tests RED**

```powershell
pnpm vitest run tests/frontend/notificationSettings.test.ts tests/frontend/settingsCenter.test.ts
```

- [ ] **Step 4: Implement the real notifications section**

Follow the existing General Settings visual/autosave conventions. Do not show fake sound/system-notification/DND controls. Mount:

```vue
<NotificationSettingsPanel v-else-if="activeSection === 'notifications'" />
```

Update the sidebar description to the v1 semantics: realtime in-app Toast preferences and notification cleanup.

- [ ] **Step 5: Run settings and store tests GREEN**

```powershell
pnpm vitest run tests/frontend/notificationSettings.test.ts tests/frontend/settingsCenter.test.ts tests/frontend/notificationStore.test.ts
```

- [ ] **Step 6: Lint, build, review, and commit Task 11**

```powershell
pnpm lint
pnpm build
git diff --check
git add src/components/settings/NotificationSettingsPanel.vue src/components/settings/NotificationSettingsPanel.css src/components/settings/SettingsCenterPage.vue src/components/settings/settingsSections.ts tests/frontend/notificationSettings.test.ts tests/frontend/settingsCenter.test.ts
git diff --cached --check
git commit -m "feat(notifications): add notification preferences and cleanup"
```

---

### Task 12: Remove duplicate delivery paths and complete end-to-end verification

**Files:**

- Modify as required by failing regressions only: `src/composables/useOrderPanel.ts`
- Modify as required by failing regressions only: `src/components/trading/OrderTable.vue`
- Modify as required by failing regressions only: `src/stores/accountProfiles.ts`
- Modify as required by failing regressions only: `src/stores/connection.ts`
- Modify as required by failing regressions only: `src/components/settings/QuickSetupDialog.vue`
- Modify as required by failing regressions only: `src/App.vue`
- Modify as required by failing regressions only: `tests/frontend/appQuickSetup.test.ts`
- Modify as required by failing regressions only: `tests/frontend/accountProfilesReconnect.test.ts`
- Modify: `docs/superpowers/specs/2026-08-23-notification-center-design.md`
- Modify: `docs/superpowers/plans/2026-08-23-notification-center.md`

**Interfaces:**

- Notification-aware command failures with `notificationId` render inline and rely on the Created event/category setting for Toast; they do not also call generic `reportError`.
- Ordinary command failures still produce one caller-owned log plus one generic Toast.
- Backend background error events produce one Rust log and one frontend Toast, with event-ID replay suppression.

- [ ] **Step 1: Add failing duplicate-delivery integration tests**

Cover a risk-blocked order, confirmed order rejection, reconciliation failure, connection incident (App auto-connect, Quick Setup, and account reconnect), ordinary command error, and background error. For each, assert exact counts for diagnostic log, persistent record, generic Toast, and notification Toast.

- [ ] **Step 2: Run the integration tests RED and inspect ownership failures**

```powershell
pnpm vitest run tests/frontend/errorDeliveryOwnership.test.ts tests/frontend/notificationStore.test.ts tests/frontend/accountSwitchReconciliation.test.ts tests/frontend/tradingSwitchGuard.test.ts tests/frontend/appQuickSetup.test.ts tests/frontend/accountProfilesReconnect.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --locked notification
```

Do not suppress failures by text matching. Fix the producer/caller ownership boundary or use the typed `notificationId` marker.

- [ ] **Step 3: Apply the minimum duplicate-path removals and rerun GREEN**

```powershell
pnpm vitest run tests/frontend/errorDeliveryOwnership.test.ts tests/frontend/notificationStore.test.ts tests/frontend/accountSwitchReconciliation.test.ts tests/frontend/tradingSwitchGuard.test.ts tests/frontend/appQuickSetup.test.ts tests/frontend/accountProfilesReconnect.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --locked notification
```

- [ ] **Step 4: Run the full frontend gate**

```powershell
pnpm lint
pnpm test
pnpm build
```

Expected: all commands exit 0. Record test counts in the final handoff.

- [ ] **Step 5: Run the full Rust gate**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

Expected: all feature tests/builds pass. If repository-wide formatting still reports unrelated pre-existing debt, prove touched-file formatting separately and report the exact unrelated files; do not reformat unrelated code.

- [ ] **Step 6: Run desktop acceptance**

```powershell
pnpm tauri dev
```

Verify at minimum: popover positioning at minimum window size; 0/1/99/99+ badge; no mark-read on open; read/all-read/delete/clear; current-account plus Global isolation; restart persistence; Toast category settings; one unavailable/recovered notification per incident; no snapshot spam; recovery from tmp/bak/corrupt storage; keyboard/Escape/outside-click/focus return; no duplicate log or Toast.

- [ ] **Step 7: Self-review spec coverage and repository hygiene**

```powershell
rg -n "TODO|FIXME|placeholder|mock later|not implemented" src src-tauri/src tests/frontend docs/superpowers/plans/2026-08-23-notification-center.md
git status --short
git diff --check
git log --oneline --decorate -12
```

Review every acceptance criterion in the spec, validate Rust/TypeScript enum spellings and camelCase fields match, confirm no secrets/raw bodies are persisted, and ensure `.superpowers/` is untracked and unstaged.

- [ ] **Step 8: Update status, request code review, and commit final cleanup**

Set the spec status to implemented only after all automatic gates and desktop acceptance pass. Use `superpowers:requesting-code-review`, address verified findings with `superpowers:receiving-code-review`, rerun affected gates, then:

```powershell
git add docs/superpowers/specs/2026-08-23-notification-center-design.md docs/superpowers/plans/2026-08-23-notification-center.md
git diff --cached --check
git commit -m "docs(notifications): record notification center verification"
```

Do not merge or push unless the user explicitly requests it.
