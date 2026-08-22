# PRD-12 Settings Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a full-page, extensible Settings Center that owns general settings and the existing account center, preserves a credential-only quick setup flow, and exposes safe runtime behavior for polling and WebSocket preferences.

**Architecture:** `AppShell` remains the navigation owner and mounts a dedicated `SettingsCenterPage` in the existing Tauri window. General settings use a narrow Rust command plus a frontend debounced single-flight queue; account, credential, risk, and reconnect operations keep explicit transactional boundaries. Rust reconfigures the single `MarketFallback` loop through a watch channel so interval changes do not abort in-flight scheduler state.

**Tech Stack:** Vue 3.5, TypeScript 5.6, Pinia 3, Naive UI 2, Vitest 4, Vue Test Utils 2, Tauri 2, Rust 2021, Tokio 1.

**Spec:** `docs/superpowers/specs/2026-08-22-settings-center-design.md`

## Global Constraints

- Keep the current Vue 3 + Pinia + Naive UI runtime; do not introduce Vue Router or mount the React scaffold.
- Render Settings Center in the existing main Tauri window; do not create another window.
- Normal gear entry always starts on `general`; never persist or restore the current settings section.
- Use exactly these settings keys: `general`, `account`, `plugins`, `notifications`, `searchCommands`, `hotkeys`, `workspace`, `appearance`, `languageRegion`, `about`.
- Use explicit navigation fields `settingsSection` and `accountSection`; do not overload a generic `section` field for settings deep links.
- Treat polling interval values as finite seconds in the inclusive range `1.0..=3600.0`; do not convert milliseconds.
- General settings must use `update_general_settings`; the new UI must not send a full `AppConfig` snapshot.
- WebSocket preference persistence must not interrupt an active connection. Application requires a user-triggered `disconnect → connect` reconnect.
- Credentials remain write-only in Keyring. Account deletion/switching and risk updates remain explicit operations with their existing rollback behavior.
- Placeholder sections are clickable and show title, purpose, and `规划中`; they contain no fake or disabled form controls.
- Keep the current dark theme. Do not implement PRD-11 or PRD-13 through PRD-18 in this plan.
- Preserve the existing chart-workspace flush on every page transition.
- Do not use `git add -A`. Stage only files listed by the current task.
- `src-tauri/Cargo.toml` currently reports a pre-existing line-ending-only modification. Before Task 1, record `git diff --numstat -- src-tauri/Cargo.toml`; Task 1 may stage only the real `test-util` feature addition and must verify the cached diff contains no unrelated rewrite.
- The repository has known Rust formatting debt outside this feature. Format touched Rust files directly; run the repository-wide format check and report unrelated failures without reformatting unrelated files.

---

### Task 1: Make the MarketFallback scheduler interval dynamically reconfigurable

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/models/config.rs`
- Modify: `src-tauri/src/services/scheduler.rs`
- Modify: `src-tauri/src/services/scheduler/tests/mod.rs`
- Create: `src-tauri/src/services/scheduler/tests/rescheduling.rs`

**Interfaces:**

- Consumes: `AppConfig.ticker_poll_interval: f64`, shared inclusive poll-interval bounds, existing `TaskRunState`, and existing `run_scheduled_task` single-flight coordination.
- Produces: `configured_task_interval(TaskId, &AppConfig) -> Option<Duration>` and `SchedulerService::set_market_fallback_interval(Duration) -> ()` for Task 2.
- Produces: one persistent MarketFallback loop whose interval is updated through `tokio::sync::watch` without aborting the loop or its in-flight run state.

- [ ] **Step 1: Record the existing Cargo manifest state**

Run:

```powershell
git status --short -- src-tauri/Cargo.toml
git diff --numstat -- src-tauri/Cargo.toml
git diff -- src-tauri/Cargo.toml
```

Expected: status may show `M`, while the content diff and numstat are empty. Save the output in the task notes; do not normalize the file.

- [ ] **Step 2: Enable paused-time tests and define the shared interval bounds**

Change only the dev-dependency feature list:

```toml
[dev-dependencies]
tokio = { version = "1", features = ["rt", "macros", "test-util"] }
```

Add the bounds once in `src-tauri/src/models/config.rs`; Task 1's scheduler and Task 2's command both import them:

```rust
pub const MIN_TICKER_POLL_INTERVAL_SECS: f64 = 1.0;
pub const MAX_TICKER_POLL_INTERVAL_SECS: f64 = 3600.0;
```

Run:

```powershell
git diff --numstat -- src-tauri/Cargo.toml
git diff -- src-tauri/Cargo.toml
git status --short -- src-tauri/Cargo.lock
```

Expected: Cargo.toml has exactly one added and one removed line, and Cargo.lock has no status. Do not use `git add --renormalize`.

- [ ] **Step 3: Add failing scheduler interval tests**

Add `mod rescheduling;` to `src-tauri/src/services/scheduler/tests/mod.rs`. In the new test file, cover configured startup intervals and reset-from-change-time behavior:

```rust
use super::super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn market_fallback_interval_comes_from_config_without_changing_other_tasks() {
    let mut config = AppConfig::default();
    config.ticker_poll_interval = 17.5;

    assert_eq!(
        configured_task_interval(TaskId::MarketFallback, &config),
        Some(Duration::from_secs_f64(17.5)),
    );
    assert_eq!(
        configured_task_interval(TaskId::FundingRate, &config),
        Some(Duration::from_secs(60)),
    );
}

#[tokio::test(start_paused = true)]
async fn interval_change_rearms_from_change_time_without_duplicate_tick() {
    let (tx, rx) = tokio::sync::watch::channel(Duration::from_secs(1));
    let running = Arc::new(AtomicBool::new(true));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let runs = Arc::new(AtomicUsize::new(0));
    let runs_for_loop = Arc::clone(&runs);

    let handle = tokio::spawn(run_reschedulable_periodic(
        Arc::clone(&running),
        run_state,
        rx,
        Duration::ZERO,
        move || {
            let runs = Arc::clone(&runs_for_loop);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        },
    ));

    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    tx.send_replace(Duration::from_secs(10));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(9)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(runs.load(Ordering::SeqCst), 2);

    running.store(false, Ordering::SeqCst);
    tx.send_replace(Duration::from_secs(1));
    handle.await.unwrap();
}
```

Add the in-flight regression in the same file:

```rust
#[tokio::test(start_paused = true)]
async fn reschedule_during_in_flight_run_keeps_one_owner_and_uses_latest_period() {
    let (tx, rx) = tokio::sync::watch::channel(Duration::from_secs(1));
    let running = Arc::new(AtomicBool::new(true));
    let run_state = Arc::new(Mutex::new(TaskRunState::default()));
    let started = Arc::new(AtomicUsize::new(0));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_in_flight = Arc::new(AtomicUsize::new(0));
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first_release = Arc::new(Mutex::new(Some(release_rx)));

    let handle = tokio::spawn(run_reschedulable_periodic(
        Arc::clone(&running),
        run_state,
        rx,
        Duration::ZERO,
        {
            let started = Arc::clone(&started);
            let in_flight = Arc::clone(&in_flight);
            let max_in_flight = Arc::clone(&max_in_flight);
            let first_release = Arc::clone(&first_release);
            move || {
                let started = Arc::clone(&started);
                let in_flight = Arc::clone(&in_flight);
                let max_in_flight = Arc::clone(&max_in_flight);
                let first_release = Arc::clone(&first_release);
                async move {
                    let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                    max_in_flight.fetch_max(current, Ordering::SeqCst);
                    let run_number = started.fetch_add(1, Ordering::SeqCst) + 1;
                    if run_number == 1 {
                        let receiver = first_release.lock().await.take();
                        if let Some(receiver) = receiver {
                            let _ = receiver.await;
                        }
                    }
                    in_flight.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }
            }
        },
    ));

    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    tx.send_replace(Duration::from_secs(5));
    tokio::time::advance(Duration::from_secs(20)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);

    release_tx.send(()).unwrap();
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(4)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 1);

    tokio::time::advance(Duration::from_secs(1)).await;
    tokio::task::yield_now().await;
    assert_eq!(started.load(Ordering::SeqCst), 2);
    assert_eq!(max_in_flight.load(Ordering::SeqCst), 1);

    running.store(false, Ordering::SeqCst);
    tx.send_replace(Duration::from_secs(1));
    handle.await.unwrap();
}
```

- [ ] **Step 4: Run the scheduler tests and confirm RED**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::rescheduling -- --nocapture
```

Expected: Cargo discovers both named tests and compilation fails only because `configured_task_interval` and `run_reschedulable_periodic` do not exist. A successful command reporting `running 0 tests` is not a valid RED result.

- [ ] **Step 5: Implement the reschedulable periodic loop**

Add a `watch::Sender<Duration>` field for MarketFallback, initialize it with the default interval, and use the loaded config when `start()` launches tasks. Keep existing fixed task intervals unchanged:

```rust
fn configured_task_interval(task: TaskId, config: &AppConfig) -> Option<Duration> {
    if task == TaskId::MarketFallback {
        let seconds = config.ticker_poll_interval;
        let safe = if seconds.is_finite()
            && (MIN_TICKER_POLL_INTERVAL_SECS..=MAX_TICKER_POLL_INTERVAL_SECS)
                .contains(&seconds)
        {
            seconds
        } else {
            1.0
        };
        Some(Duration::from_secs_f64(safe))
    } else {
        task.interval()
    }
}

pub fn set_market_fallback_interval(&self, interval: Duration) {
    self.market_fallback_interval_tx.send_replace(interval);
}
```

Launch MarketFallback through one loop and reset its ticker in place:

```rust
async fn run_reschedulable_periodic<F, Fut>(
    running: Arc<AtomicBool>,
    run_state: Arc<Mutex<TaskRunState>>,
    mut intervals: tokio::sync::watch::Receiver<Duration>,
    first_delay: Duration,
    mut execute: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = AppResult<()>>,
{
    let initial = *intervals.borrow();
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + first_delay, initial);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            changed = intervals.changed() => {
                if changed.is_err() { break; }
                if !running.load(Ordering::Relaxed) { break; }
                let next = *intervals.borrow_and_update();
                ticker = tokio::time::interval_at(tokio::time::Instant::now() + next, next);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            }
            _ = ticker.tick() => {
                if !running.load(Ordering::Relaxed) { break; }
                let _ = run_scheduled_task(run_state.as_ref(), false, &mut execute).await;
            }
        }
    }
}
```

Do not abort a MarketFallback `JoinHandle` to apply a new interval. An abort while the task owns `TaskRunState.in_flight` could leave the shared state permanently busy.

Wire the loop through one service field and the existing task handle:

```rust
market_fallback_interval_tx: tokio::sync::watch::Sender<Duration>,
```

In `new()`, create `watch::channel(INTERVAL_MARKET_FALLBACK)` and retain the sender. In `start()`, read the loaded config once, call `send_replace(configured_task_interval(TaskId::MarketFallback, &config).unwrap())`, skip MarketFallback in the fixed-period spawn branch, and call:

```rust
async fn spawn_market_fallback(&self) {
    let runtime = self
        .tasks
        .get(&TaskId::MarketFallback)
        .expect("task registered");
    let scheduler = self.clone_refs();
    let run_state = Arc::clone(&runtime.run_state);
    let receiver = self.market_fallback_interval_tx.subscribe();
    let handle = tauri::async_runtime::spawn(run_reschedulable_periodic(
        Arc::clone(&self.running),
        run_state,
        receiver,
        Duration::ZERO,
        move || scheduler.execute(TaskId::MarketFallback),
    ));
    *runtime.handle.lock().await = Some(handle);
}
```

`set_market_fallback_interval` and `spawn_market_fallback` must use the same sender. `stop()` continues to abort the one handle stored in `TaskRuntime`, so restart cannot leave an orphan loop.

- [ ] **Step 6: Run focused scheduler tests and the existing scheduler suite**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::rescheduling -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests -- --nocapture
```

Expected: the new interval tests pass; existing KlineFlush, bootstrap, coordination, and fallback tests remain green.

- [ ] **Step 7: Format only the touched Rust files and verify the staged manifest diff**

Run:

```powershell
git status --short
rustfmt --edition 2021 --config skip_children=true src-tauri/src/models/config.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/rescheduling.rs
git status --short
git diff --check -- src-tauri/Cargo.toml src-tauri/src/models/config.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/rescheduling.rs
```

Expected: no whitespace errors and no file outside the Task 1 list changes. If rustfmt touches another file, stop and restore only that unexpected formatter change before staging.

- [ ] **Step 8: Commit the dynamic scheduler**

```powershell
git add src-tauri/Cargo.toml
git add src-tauri/src/models/config.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/services/scheduler/tests/rescheduling.rs
git diff --cached --check
git diff --cached --numstat -- src-tauri/Cargo.toml
git diff --cached -- src-tauri/Cargo.toml
git commit -m "feat(scheduler): support dynamic market fallback interval"
```

Expected: the cached Cargo numstat is exactly `1  1`, the diff contains only the `test-util` feature addition, and Cargo.lock is absent.

---

### Task 2: Add an atomic narrow command for general settings

**Files:**

- Modify: `src-tauri/src/models/config.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Create: `src-tauri/src/commands/config/general_settings.rs`
- Create: `src-tauri/src/commands/config/general_settings/tests.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

- Consumes: `SchedulerService::set_market_fallback_interval(Duration)` from Task 1 and the shared `AccountLifecycleCoordinator` mutation lock.
- Produces: `UpdateGeneralSettingsRequest`, `GeneralSettings`, and the Tauri command `update_general_settings(request) -> GeneralSettings` for Task 3.
- Guarantees: persistence succeeds before runtime state changes; unchanged poll interval does not notify the scheduler; no non-general field is accepted from the frontend.

- [ ] **Step 1: Establish test discovery and add the first failing validation test**

Add `mod general_settings;` near the top of `src-tauri/src/commands/config.rs`. Create `src-tauri/src/commands/config/general_settings.rs` with only:

```rust
#[cfg(test)]
mod tests;
```

Create `src-tauri/src/commands/config/general_settings/tests.rs`. Import `super::*`, define `request(seconds)` using the expected `UpdateGeneralSettingsRequest`, and add:

```rust
use crate::commands::market::persist_market_config_update;
use crate::error::AppError;
use crate::models::config::AppConfig;
use crate::services::AccountLifecycleCoordinator;
use crate::storage::ConfigStore;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

fn request(seconds: f64) -> UpdateGeneralSettingsRequest {
    UpdateGeneralSettingsRequest {
        use_websocket: false,
        ticker_poll_interval: seconds,
    }
}
```

```rust
#[test]
fn rejects_non_finite_and_out_of_range_intervals() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.0,
        3600.000_001,
    ] {
        assert!(validate_request(request(value)).is_err(), "{value}");
    }
    assert!(validate_request(request(1.0)).is_ok());
    assert!(validate_request(request(3600.0)).is_ok());
}
```

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::general_settings::tests::rejects_non_finite_and_out_of_range_intervals -- --exact --nocapture
```

Expected: compilation fails and stderr points at `commands/config/general_settings/tests.rs` because the request type and `validate_request` do not exist. A successful `running 0 tests` result is invalid and means the module discovery chain is incomplete.

- [ ] **Step 2: Add the DTOs and make validation GREEN**

In `src-tauri/src/models/config.rs`, consume the bounds introduced by Task 1 and add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGeneralSettingsRequest {
    pub use_websocket: bool,
    pub ticker_poll_interval: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    pub use_websocket: bool,
    pub ticker_poll_interval: f64,
}

impl From<&AppConfig> for GeneralSettings {
    fn from(config: &AppConfig) -> Self {
        Self {
            use_websocket: config.use_websocket,
            ticker_poll_interval: config.ticker_poll_interval,
        }
    }
}
```

Implement `validate_request` in `general_settings.rs`:

```rust
fn validate_request(request: UpdateGeneralSettingsRequest) -> AppResult<GeneralSettings> {
    let seconds = request.ticker_poll_interval;
    if !seconds.is_finite()
        || !(MIN_TICKER_POLL_INTERVAL_SECS..=MAX_TICKER_POLL_INTERVAL_SECS)
            .contains(&seconds)
    {
        return Err(AppError::Config(
            "行情轮询间隔必须是 1 到 3600 秒之间的有限数值".into(),
        ));
    }
    Ok(GeneralSettings {
        use_websocket: request.use_websocket,
        ticker_poll_interval: seconds,
    })
}
```

Rerun the exact test and require `1 passed`, not merely exit code zero.

- [ ] **Step 3: Add failing persistence, ordering, and concurrency tests**

Add reusable helpers in `tests.rs`:

```rust
fn test_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-general-settings-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4(),
    ))
}

fn without_general_fields(config: &AppConfig) -> serde_json::Value {
    let mut value = serde_json::to_value(config).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("useWebsocket");
    object.remove("tickerPollInterval");
    value
}

fn cleanup_config(path: &std::path::Path) {
    for candidate in [
        path.to_path_buf(),
        std::path::PathBuf::from(format!("{}.bak", path.display())),
        std::path::PathBuf::from(format!("{}.tmp", path.display())),
    ] {
        let _ = std::fs::remove_file(candidate);
    }
}
```

The narrow-field and persistence-failure tests are concrete:

```rust
#[tokio::test]
async fn update_changes_only_general_fields_after_persistence() {
    let path = test_path("narrow").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let coordinator = AccountLifecycleCoordinator::new();
    let mut initial = AppConfig::default();
    initial.active_symbol = "ETHUSDT".into();
    initial.active_account_id = "backup".into();
    initial.accounts = vec!["primary".into(), "backup".into()];
    initial.window_width = 1555;
    initial.watchlist_symbols = vec!["ETHUSDT".into(), "SOLUSDT".into()];
    initial.risk_max_order_qty = "25.5".into();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial.clone()));
    let notifications = Arc::new(std::sync::Mutex::new(Vec::new()));

    let result = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        UpdateGeneralSettingsRequest {
            use_websocket: false,
            ticker_poll_interval: 15.0,
        },
        {
            let notifications = Arc::clone(&notifications);
            move |interval| notifications.lock().unwrap().push(interval)
        },
    )
    .await
    .unwrap();

    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    assert_eq!(without_general_fields(&memory), without_general_fields(&initial));
    assert_eq!(without_general_fields(&disk), without_general_fields(&initial));
    for config in [&memory, &disk] {
        assert!(!config.use_websocket);
        assert_eq!(config.ticker_poll_interval, 15.0);
    }
    assert_eq!(result.ticker_poll_interval, 15.0);
    assert!(!result.use_websocket);
    assert_eq!(*notifications.lock().unwrap(), [Duration::from_secs(15)]);
    cleanup_config(&path);
}

#[tokio::test]
async fn persistence_failure_changes_neither_runtime_nor_schedule() {
    let blocker = test_path("blocked-parent");
    std::fs::write(&blocker, "not a directory").unwrap();
    let store = ConfigStore::with_path(blocker.join("config.toml"));
    let initial = AppConfig::default();
    let runtime = Arc::new(RwLock::new(initial.clone()));
    let notifications = Arc::new(AtomicUsize::new(0));

    let result = apply_general_settings_update(
        &AccountLifecycleCoordinator::new(),
        &store,
        &runtime,
        request(30.0),
        {
            let notifications = Arc::clone(&notifications);
            move |_| { notifications.fetch_add(1, Ordering::SeqCst); }
        },
    )
    .await;

    assert!(result.is_err());
    let memory = runtime.read().await.clone();
    assert_eq!(without_general_fields(&memory), without_general_fields(&initial));
    assert_eq!(memory.use_websocket, initial.use_websocket);
    assert_eq!(memory.ticker_poll_interval, initial.ticker_poll_interval);
    assert_eq!(notifications.load(Ordering::SeqCst), 0);
    std::fs::remove_file(blocker).unwrap();
}
```

Also add:

- `scheduler_notification_observes_committed_disk_and_runtime_state`: inside the synchronous callback, load the same path through a second `ConfigStore` and use `runtime.try_read()`; both already contain the new interval.
- `unchanged_interval_does_not_rearm_market_fallback`: change only `use_websocket` and assert zero callbacks.
- `concurrent_general_and_market_updates_preserve_both_modules`: import `crate::commands::market::persist_market_config_update`, hold the coordinator guard, poll general then market into the fair queue, release, join, and assert both disk/runtime contain the general values plus `active_symbol = "ETHUSDT"` and `kline_interval = "15"`.
- `queued_lifecycle_fields_then_general_preserve_account_window_and_risk`: first queue a `run_serialized_account_mutation` that clones runtime, changes `active_account_id`, `accounts`, both window dimensions, `risk_enabled`, `risk_max_order_qty`, `risk_max_daily_orders`, and `trading_day_timezone`, persists and replaces runtime; queue the general update second. After join, assert the later general save retained every lifecycle/window/risk field on disk and in memory.
- `queued_general_updates_notify_in_commit_order`: while holding the guard, poll the 10-second update before the 20-second update, release, join, and assert callback durations are `[10s, 20s]`; the last callback equals the disk/runtime interval.

For every queued test, use `tokio::pin!`, assert each initial `futures_util::poll!` is `std::task::Poll::Pending`, and only then drop the held guard. This makes queue order deterministic rather than scheduler-dependent.

Use this exact queue pattern for the cross-module cases:

```rust
#[tokio::test]
async fn concurrent_general_and_market_updates_preserve_both_modules() {
    let path = test_path("general-market").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let coordinator = AccountLifecycleCoordinator::new();
    let held = coordinator.mutation_guard().await;

    let general = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        request(20.0),
        |_| {},
    );
    let market = persist_market_config_update(
        &coordinator,
        &store,
        &runtime,
        |next| {
            next.active_symbol = "ETHUSDT".into();
            next.kline_interval = "15".into();
        },
        || async {},
    );
    tokio::pin!(general);
    tokio::pin!(market);
    assert!(matches!(
        futures_util::poll!(&mut general),
        std::task::Poll::Pending,
    ));
    assert!(matches!(
        futures_util::poll!(&mut market),
        std::task::Poll::Pending,
    ));
    drop(held);

    let (general_result, market_result) = tokio::join!(general, market);
    general_result.unwrap();
    market_result.unwrap();
    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    for config in [&memory, &disk] {
        assert_eq!(config.ticker_poll_interval, 20.0);
        assert_eq!(config.active_symbol, "ETHUSDT");
        assert_eq!(config.kline_interval, "15");
    }
    cleanup_config(&path);
}

#[tokio::test]
async fn queued_lifecycle_fields_then_general_preserve_account_window_and_risk() {
    let path = test_path("general-lifecycle").with_extension("toml");
    let store = ConfigStore::with_path(path.clone());
    let initial = AppConfig::default();
    store.save(&initial).unwrap();
    let runtime = Arc::new(RwLock::new(initial));
    let coordinator = AccountLifecycleCoordinator::new();
    let held = coordinator.mutation_guard().await;

    let lifecycle_store = &store;
    let lifecycle_runtime = &runtime;
    let lifecycle = crate::services::account_profiles::run_serialized_account_mutation(
        &coordinator,
        move || async move {
            let mut next = lifecycle_runtime.read().await.clone();
            next.active_account_id = "backup".into();
            next.accounts = vec!["primary".into(), "backup".into()];
            next.window_width = 1555;
            next.window_height = 955;
            next.risk_enabled = false;
            next.risk_max_order_qty = "25.5".into();
            next.risk_max_daily_orders = 20;
            next.trading_day_timezone = "UTC".into();
            lifecycle_store.save(&next)?;
            *lifecycle_runtime.write().await = next;
            Ok::<(), AppError>(())
        },
    );
    let general = apply_general_settings_update(
        &coordinator,
        &store,
        &runtime,
        request(30.0),
        |_| {},
    );
    tokio::pin!(lifecycle);
    tokio::pin!(general);
    assert!(matches!(
        futures_util::poll!(&mut lifecycle),
        std::task::Poll::Pending,
    ));
    assert!(matches!(
        futures_util::poll!(&mut general),
        std::task::Poll::Pending,
    ));
    drop(held);

    let (lifecycle_result, general_result) = tokio::join!(lifecycle, general);
    lifecycle_result.unwrap();
    general_result.unwrap();
    let memory = runtime.read().await.clone();
    let disk = store.load().unwrap();
    for config in [&memory, &disk] {
        assert_eq!(config.ticker_poll_interval, 30.0);
        assert_eq!(config.active_account_id, "backup");
        assert_eq!(
            config.accounts,
            vec!["primary".to_string(), "backup".to_string()],
        );
        assert_eq!((config.window_width, config.window_height), (1555, 955));
        assert!(!config.risk_enabled);
        assert_eq!(config.risk_max_order_qty, "25.5");
        assert_eq!(config.risk_max_daily_orders, 20);
        assert_eq!(config.trading_day_timezone, "UTC");
    }
    cleanup_config(&path);
}
```

Use the same held-guard/poll/drop pattern for the two general updates; the callback records into `Arc<std::sync::Mutex<Vec<Duration>>>`, and the test asserts the exact vector before comparing its last element with disk and runtime.

- [ ] **Step 4: Run the full module and confirm the persistence/helper tests are RED**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::general_settings::tests -- --nocapture
```

Expected: the validation test remains green, while compilation fails on the missing `apply_general_settings_update`. Stderr must reference the newly added test file; `running 0 tests` is invalid.

- [ ] **Step 5: Implement the serialized helper and Tauri command**

Use a synchronous, infallible callback, invoked zero or one time inside the same lifecycle mutation lock after disk and runtime commit:

```rust
async fn apply_general_settings_update<F>(
    coordinator: &AccountLifecycleCoordinator,
    config_store: &ConfigStore,
    runtime_config: &Arc<RwLock<AppConfig>>,
    request: UpdateGeneralSettingsRequest,
    notify_interval_committed: F,
) -> AppResult<GeneralSettings>
where
    F: FnOnce(Duration),
{
    let requested = validate_request(request)?;
    crate::services::account_profiles::run_serialized_account_mutation(
        coordinator,
        move || async move {
            let mut next = runtime_config.read().await.clone();
            let interval_changed =
                next.ticker_poll_interval != requested.ticker_poll_interval;
            next.use_websocket = requested.use_websocket;
            next.ticker_poll_interval = requested.ticker_poll_interval;
            config_store.save(&next)?;
            *runtime_config.write().await = next.clone();
            if interval_changed {
                notify_interval_committed(Duration::from_secs_f64(
                    requested.ticker_poll_interval,
                ));
            }
            Ok(GeneralSettings::from(&next))
        },
    )
    .await
}

#[tauri::command]
pub async fn update_general_settings(
    state: State<'_, AppState>,
    request: UpdateGeneralSettingsRequest,
) -> AppResult<GeneralSettings> {
    let scheduler = Arc::clone(&state.scheduler);
    apply_general_settings_update(
        state.account_lifecycle.as_ref(),
        &state.config_store,
        &state.config,
        request,
        move |interval| scheduler.set_market_fallback_interval(interval),
    )
    .await
}
```

- [ ] **Step 6: Make the helper suite GREEN, then export and register the command**

Run the module and require every named test to execute and pass. Then change `config.rs` from the private module declaration to:

```rust
mod general_settings;
pub use general_settings::update_general_settings;
```

Register `update_general_settings` immediately after `save_config` in `src-tauri/src/lib.rs`, then run `cargo check --manifest-path src-tauri/Cargo.toml --locked` so the generated Tauri handler is type-checked.

- [ ] **Step 7: Run focused coordination and full Rust tests**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::config::general_settings::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked services::scheduler::tests::rescheduling -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked commands::risk::tests::coordination -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked services::account_profiles::tests -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Expected: each focused command reports nonzero executed tests and all pass, including the real account/risk transaction regressions.

- [ ] **Step 8: Format, inspect, and commit the narrow command**

```powershell
git status --short
rustfmt --edition 2021 --config skip_children=true src-tauri/src/models/config.rs src-tauri/src/commands/config.rs src-tauri/src/commands/config/general_settings.rs src-tauri/src/commands/config/general_settings/tests.rs src-tauri/src/lib.rs
git status --short
git diff --check -- src-tauri/src/models/config.rs src-tauri/src/commands/config.rs src-tauri/src/commands/config/general_settings.rs src-tauri/src/commands/config/general_settings/tests.rs src-tauri/src/lib.rs
git add src-tauri/src/models/config.rs src-tauri/src/commands/config.rs src-tauri/src/commands/config/general_settings.rs src-tauri/src/commands/config/general_settings/tests.rs src-tauri/src/lib.rs
git commit -m "feat(settings): add atomic general settings update"
```

Expected: rustfmt changes only Task 2 files. If another file changes, stop and restore only the unexpected formatter output before staging.

---

### Task 3: Add frontend general-settings state and single-flight autosave

**Files:**

- Modify: `src/types/models.ts`
- Modify: `src/stores/config.ts`
- Create: `src/composables/useGeneralSettingsAutosave.ts`
- Create: `tests/frontend/generalSettingsAutosave.test.ts`

**Interfaces:**

- Consumes: `update_general_settings` from Task 2.
- Produces: `configStore.updateGeneralSettings(request): Promise<GeneralSettings>` and `configStore.adoptGeneralSettings(settings): void`.
- Produces: `useGeneralSettingsAutosave(save, reconcile, debounceMs)` with `draft`, `lastSaved`, `status`, `error`, `initialize`, `update`, `flush`, `retry`, and `dispose` for Task 4.
- Guarantees: initialization never saves; 300ms debounce; one request in flight; edits during save coalesce into one latest draft; failure pauses auto-retry and preserves the draft.

- [ ] **Step 1: Add failing store tests for the narrow command**

In `tests/frontend/generalSettingsAutosave.test.ts`, create a Pinia per test and mock `tauriInvoke`. Verify the store sends only the narrow request and adopts only returned fields:

```ts
it('updates only the returned general settings fields', async () => {
  const store = useConfigStore()
  store.config = makeConfig({ activeSymbol: 'ETHUSDT', windowWidth: 1555 })
  vi.mocked(tauriInvoke).mockResolvedValueOnce({
    useWebsocket: false,
    tickerPollInterval: 15,
  })

  await store.updateGeneralSettings({ useWebsocket: false, tickerPollInterval: 15 })

  expect(tauriInvoke).toHaveBeenCalledWith('update_general_settings', {
    request: { useWebsocket: false, tickerPollInterval: 15 },
  })
  expect(store.config).toMatchObject({
    activeSymbol: 'ETHUSDT',
    windowWidth: 1555,
    useWebsocket: false,
    tickerPollInterval: 15,
  })
})
```

Define `makeConfig(overrides)` in the test with every existing `AppConfig` field so later model changes cannot hide missing fixtures. Also start a deferred `fetchConfig()`, complete `updateGeneralSettings()`, then resolve the fetch with stale general fields; assert the late fetch cannot overwrite the committed general fields. This verifies `adoptGeneralSettings` invalidates older full-config reads just like the existing account/risk authority paths.

- [ ] **Step 2: Run the store test and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/generalSettingsAutosave.test.ts
```

Expected: TypeScript or runtime failure because the general settings types and store methods do not exist.

- [ ] **Step 3: Add TypeScript models and store methods**

Add matching camelCase types:

```ts
export interface GeneralSettings {
  useWebsocket: boolean
  tickerPollInterval: number
}

export type UpdateGeneralSettingsRequest = GeneralSettings
```

Implement store adoption without replacing unrelated fields:

```ts
function adoptGeneralSettings(settings: GeneralSettings): void {
  latestFetchRequest += 1
  loading.value = false
  if (config.value) config.value = { ...config.value, ...settings }
}

async function updateGeneralSettings(
  request: UpdateGeneralSettingsRequest,
): Promise<GeneralSettings> {
  const result = await tauriInvoke<GeneralSettings>('update_general_settings', { request })
  adoptGeneralSettings(result)
  return result
}
```

Keep `saveConfig` for existing compatibility callers, but do not use it in the new panel.

- [ ] **Step 4: Add failing autosave lifecycle tests**

Use fake timers and deferred promises. Cover all of these exact transitions:

```ts
it('does not save initialization and coalesces rapid edits', async () => {
  vi.useFakeTimers()
  const save = vi.fn(async (value: GeneralSettings) => value)
  const autosave = useGeneralSettingsAutosave(save, vi.fn(async () => undefined), 300)

  autosave.initialize({ useWebsocket: true, tickerPollInterval: 1 })
  autosave.update({ tickerPollInterval: 2 })
  autosave.update({ tickerPollInterval: 3 })
  await vi.advanceTimersByTimeAsync(299)
  expect(save).not.toHaveBeenCalled()
  await vi.advanceTimersByTimeAsync(1)
  await flushPromises()
  expect(save).toHaveBeenCalledTimes(1)
  expect(save).toHaveBeenLastCalledWith({ useWebsocket: true, tickerPollInterval: 3 })
  expect(autosave.status.value).toBe('saved')
})
```

Add a deferred first save, edit during the in-flight request, resolve it, and assert a second call saves only the latest complete draft. Add a rejection test that asserts status `error`, draft unchanged, reconciliation called once, no timed automatic retry, and `retry()` succeeds. Add `dispose()` coverage that flushes a still-debounced draft once. Add a failed-dispose case where that final save rejects: `dispose()` must reject with the save error after reconciliation so the unmount caller can create a durable global error record.

- [ ] **Step 5: Run autosave tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/generalSettingsAutosave.test.ts
```

Expected: the store test passes and autosave tests fail because the composable does not exist.

- [ ] **Step 6: Implement the autosave composable**

Expose Vue refs and a stable API. The core methods must follow this concrete state machine:

```ts
export type GeneralSettingsSaveStatus = 'idle' | 'saving' | 'saved' | 'error'

export function useGeneralSettingsAutosave(
  save: (draft: GeneralSettings) => Promise<GeneralSettings>,
  reconcile: () => Promise<void>,
  debounceMs = 300,
) {
  const draft = ref<GeneralSettings | null>(null)
  const lastSaved = ref<GeneralSettings | null>(null)
  const status = ref<GeneralSettingsSaveStatus>('idle')
  const error = ref<string | null>(null)
  let timer: ReturnType<typeof globalThis.setTimeout> | null = null
  let dirty = false
  let inFlight: Promise<void> | null = null

  function clearTimer(): void {
    if (timer !== null) globalThis.clearTimeout(timer)
    timer = null
  }

  function initialize(value: GeneralSettings): void {
    clearTimer()
    draft.value = { ...value }
    lastSaved.value = { ...value }
    dirty = false
    status.value = 'idle'
    error.value = null
  }

  function schedule(): void {
    clearTimer()
    timer = globalThis.setTimeout(() => { void flush() }, debounceMs)
  }

  function update(patch: Partial<GeneralSettings>): void {
    if (!draft.value) throw new Error('通用设置尚未初始化')
    draft.value = { ...draft.value, ...patch }
    dirty = true
    error.value = null
    if (status.value === 'error') status.value = 'idle'
    schedule()
  }

  async function drain(): Promise<void> {
    while (dirty && draft.value) {
      const snapshot = { ...draft.value }
      dirty = false
      status.value = 'saving'
      error.value = null
      try {
        const committed = await save(snapshot)
        lastSaved.value = { ...committed }
        status.value = dirty ? 'saving' : 'saved'
      } catch (cause) {
        dirty = true
        status.value = 'error'
        error.value = cause instanceof Error ? cause.message : String(cause)
        await reconcile().catch(() => undefined)
        return
      }
    }
  }

  async function flush(): Promise<void> {
    clearTimer()
    if (inFlight) {
      await inFlight
      if (dirty && status.value !== 'error') await flush()
      return
    }
    if (!dirty || !draft.value || status.value === 'error') return
    inFlight = drain().finally(() => { inFlight = null })
    await inFlight
  }

  async function retry(): Promise<void> {
    if (!draft.value) return
    dirty = true
    status.value = 'idle'
    error.value = null
    await flush()
  }

  async function dispose(): Promise<void> {
    clearTimer()
    if (status.value !== 'error') await flush()
    if (status.value === 'error') {
      throw new Error(error.value ?? '通用设置保存失败')
    }
  }

  return {
    draft,
    lastSaved,
    status,
    error,
    initialize,
    update,
    flush,
    retry,
    dispose,
  }
}
```

`flush()` loops while dirty, snapshots the complete draft, clears dirty before awaiting, and checks whether a newer edit marked dirty during the request. On success, update `lastSaved` from the backend response. On failure, retain the previous `lastSaved`, restore dirty, set `error`, await `reconcile()` once, and stop until `retry()` or a later explicit edit schedules a new attempt. `flush()` retains UI-oriented non-throwing semantics, while `dispose()` converts a final error state into a rejection for global reporting. Never assign a successful response into `draft`; only the config store and `lastSaved` adopt the backend response, so a late response cannot overwrite newer user input.

- [ ] **Step 7: Run focused frontend tests and TypeScript build**

```powershell
pnpm exec vitest run tests/frontend/generalSettingsAutosave.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/risk.test.ts
pnpm build
```

Expected: autosave, account authority, risk authority, and strict TypeScript checks pass.

- [ ] **Step 8: Commit frontend general-settings state**

```powershell
git add src/types/models.ts src/stores/config.ts src/composables/useGeneralSettingsAutosave.ts tests/frontend/generalSettingsAutosave.test.ts
git diff --cached --check
git commit -m "feat(settings): add general settings autosave"
```

---

### Task 4: Build the General settings panel and explicit reconnect flow

**Files:**

- Modify: `src/stores/connection.ts`
- Modify: `tests/frontend/connection.test.ts`
- Create: `src/components/settings/GeneralSettingsPanel.vue`
- Create: `src/components/settings/GeneralSettingsPanel.css`
- Create: `tests/frontend/generalSettingsPanel.test.ts`

**Interfaces:**

- Consumes: Task 3 autosave API and config-store narrow update.
- Produces: `connectionStore.reconnect(startRealtime: boolean): Promise<void>`, `reconnecting`, and `reconnectError` for Task 5.
- Produces: a prop-free `GeneralSettingsPanel` for Task 6.
- Guarantees: reconnect is single-flight and ordered `disconnect` before `connect`; changing WebSocket preference never calls either command automatically.

- [ ] **Step 1: Add failing connection-store reconnect tests**

Extend `tests/frontend/connection.test.ts`:

```ts
it('reconnects once by disconnecting before connecting', async () => {
  const store = useConnectionStore()
  vi.mocked(tauriInvoke).mockImplementation((command) => {
    if (command === 'get_connection_status') return Promise.resolve('connected')
    if (command === 'scheduler_run_task') return Promise.resolve(undefined)
    return Promise.resolve(undefined)
  })

  await Promise.all([store.reconnect(false), store.reconnect(false)])

  const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
  expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
  expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
  expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
  expect(store.reconnectError).toBeNull()
})
```

Add a failed-connect test: disconnect resolves, connect rejects, `reconnect()` rejects, `reconnecting` returns false, and `reconnectError` contains the connection failure.

- [ ] **Step 2: Run the connection tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/connection.test.ts
```

Expected: failure because `reconnect`, `reconnecting`, and `reconnectError` do not exist.

- [ ] **Step 3: Implement single-flight reconnect**

Add local request state without changing existing `connect()` semantics:

```ts
const reconnecting = ref(false)
const reconnectError = ref<string | null>(null)
let reconnectPromise: Promise<void> | null = null

function reconnect(startRealtime: boolean): Promise<void> {
  if (reconnectPromise) return reconnectPromise
  reconnecting.value = true
  reconnectError.value = null
  reconnectPromise = (async () => {
    try {
      await disconnect()
      await connect(startRealtime)
    } catch (error) {
      reconnectError.value = formatInvokeError(error)
      throw error
    } finally {
      reconnecting.value = false
      reconnectPromise = null
    }
  })()
  return reconnectPromise
}
```

Return the new state and method from the store.

- [ ] **Step 4: Add failing panel tests**

Create `tests/frontend/generalSettingsPanel.test.ts` with one Pinia per test, fake timers, a complete `AppConfig`, and mocked Tauri calls. Cover:

- mount initializes WebSocket and poll values without invoking `update_general_settings`;
- two quick interval edits produce one request with the latest seconds;
- `null`, `0`, negative, non-finite, and values above `3600` show a field error and never invoke the command; exact boundaries `1` and `3600` save successfully;
- changing WebSocket while connected persists but does not call `disconnect` or `connect`;
- after successful persistence, the panel shows `重连后生效`; clicking `data-testid="general-reconnect"` calls disconnect then connect with the saved preference;
- persistence failure retains the control value, shows a save error, and does not expose the reconnect action as applied;
- clicking `data-testid="general-settings-save-retry"` retries the retained draft and clears the save error only after success;
- reconnect failure is caught by the click handler (no unhandled rejection), rendered separately from save status, and keeps the reconnect action available; clicking it again performs a second ordered `disconnect → connect` attempt without another settings save.
- after mode B is saved and awaits reconnect, editing back to A and failing that save retains the pending action for committed mode B; the unsaved A draft and its retry stay separate.
- changing the preference while disconnected shows no reconnect action; if a later natural connection succeeds, no stale reconnect action appears.
- with a deferred `get_config`, fields remain disabled, no default draft is saved, and successful resolution initializes without invoking `update_general_settings`;
- a rejected initial `get_config` renders `role="alert"` plus `data-testid="general-settings-retry"`; retry success initializes once and still does not save.
- unmounting with a still-debounced save failure calls the global error reporter with `离开通用设置前保存失败` after the composable's `dispose()` rejects.

The reconnect test must assert exact call order and payload:

```ts
expect(tauriInvoke).toHaveBeenCalledWith('connect', {
  startRealtime: false,
  credential: undefined,
})
```

- [ ] **Step 5: Run the panel tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/generalSettingsPanel.test.ts
```

Expected: failure because `GeneralSettingsPanel.vue` does not exist.

- [ ] **Step 6: Implement the panel**

Use `NSwitch`, `NInputNumber`, existing buttons, and inline status text. Track `initializing` and `initializationError`. Initialize once from `configStore.config`; if it is null, await `fetchConfig()` before enabling fields. Never create a default draft while the config is unknown. A failed load renders an inline alert and explicit retry. Treat the numeric update as `number | null` and call `autosave.update` only after `Number.isFinite(value)` and the inclusive `1 <= value <= 3600` guard pass; otherwise retain the visible draft/error and make no IPC call. Wire valid updates through Task 3:

```ts
const autosave = useGeneralSettingsAutosave(
  (draft) => configStore.updateGeneralSettings(draft),
  async () => {
    try {
      await configStore.fetchConfig()
    } catch (error) {
      reportError(error, '通用设置对账失败')
      throw error
    }
  },
)
const appliedWebsocketMode = ref<boolean | null>(null)
const reconnectCandidate = ref<boolean | null>(null)
const websocketEditPending = ref(false)
const pendingWebsocketMode = ref<boolean | null>(null)
const reconnectRequired = computed(() =>
  pendingWebsocketMode.value !== null
  && (!websocketEditPending.value || autosave.status.value === 'error'),
)

function updateWebsocket(value: boolean): void {
  websocketEditPending.value = true
  reconnectCandidate.value = connectionStore.connected ? value : null
  autosave.update({ useWebsocket: value })
}

async function reconnectNow(): Promise<void> {
  const mode = pendingWebsocketMode.value
  if (mode === null) return
  try {
    await connectionStore.reconnect(mode)
    appliedWebsocketMode.value = mode
    pendingWebsocketMode.value = null
  } catch {
    // The store owns reconnectError. Keep the committed pending mode retryable.
  }
}
```

Set `appliedWebsocketMode` during initialization. When autosave reaches `saved`, promote `reconnectCandidate` into `pendingWebsocketMode` only if the connection is still connected, `lastSaved.useWebsocket` equals the candidate, and it differs from the applied mode; then clear the candidate. This prevents a failed save from exposing a reconnect action.

```ts
watch(
  () => [
    autosave.status.value,
    autosave.lastSaved.value?.useWebsocket,
  ] as const,
  ([status, savedMode]) => {
    if (status !== 'saved' || !websocketEditPending.value) return
    const candidate = reconnectCandidate.value
    pendingWebsocketMode.value = connectionStore.connected
      && candidate !== null
      && savedMode === candidate
      && candidate !== appliedWebsocketMode.value
      ? candidate
      : null
    websocketEditPending.value = false
    reconnectCandidate.value = null
  },
  { flush: 'sync' },
)

watch(
  () => connectionStore.connected,
  (connected, wasConnected) => {
    if (connected && !wasConnected) {
      appliedWebsocketMode.value =
        autosave.lastSaved.value?.useWebsocket ?? null
      pendingWebsocketMode.value = null
      reconnectCandidate.value = websocketEditPending.value
        ? autosave.draft.value?.useWebsocket ?? null
        : null
      return
    }
    if (!connected && wasConnected && !connectionStore.reconnecting) {
      pendingWebsocketMode.value = null
      reconnectCandidate.value = null
    }
  },
)
```

Watch connection transitions with these rules:

- `false → true`: a natural or successful explicit connection consumed the saved preference, so set `appliedWebsocketMode = autosave.lastSaved.value?.useWebsocket ?? null` and clear pending/candidate state;
- `true → false` while `connectionStore.reconnecting` is true: retain `pendingWebsocketMode`, because disconnect is the first half of explicit reconnect;
- `true → false` outside explicit reconnect: clear pending state because there is no active connection to reconfigure.

Because pending mode is independent of `connectionStore.connected`, a failed explicit reconnect leaves the action visible for retry. Call `void autosave.dispose().catch((error) => reportError(error, '离开通用设置前保存失败'))` from `onBeforeUnmount`. Render `saving`/`saved` in a non-interrupting `role="status" aria-live="polite"`; render initialization, validation, save, and connection errors with `role="alert"`; give retry/reconnect controls explicit accessible names. Do not show success Toasts for auto-save.

- [ ] **Step 7: Run focused tests, lint, and build**

```powershell
pnpm exec vitest run tests/frontend/connection.test.ts tests/frontend/generalSettingsAutosave.test.ts tests/frontend/generalSettingsPanel.test.ts
pnpm lint
pnpm build
```

Expected: reconnect, autosave, validation, strict TypeScript, and lint pass.

- [ ] **Step 8: Commit the General panel**

```powershell
git add src/stores/connection.ts src/components/settings/GeneralSettingsPanel.vue src/components/settings/GeneralSettingsPanel.css tests/frontend/connection.test.ts tests/frontend/generalSettingsPanel.test.ts
git commit -m "feat(settings): add general settings panel"
```

---

### Task 5: Move account content into a settings-owned page

**Files:**

- Modify: `src/types/navigation.ts`
- Create: `src/components/settings/AccountSettingsPage.vue`
- Create: `tests/frontend/accountSettingsPage.test.ts`
- Modify: `src/components/account/AccountProfilesPanel.vue`
- Create: `tests/frontend/accountProfilesReconnect.test.ts`

**Interfaces:**

- Consumes: `connectionStore.reconnect` from Task 4 and existing account/assets/risk panels.
- Produces: `AccountSettingsSection = 'api' | 'assets' | 'risk'` and `AccountSettingsPage({ initialSection?: AccountSettingsSection })` for Task 6.
- Temporarily preserves `export type AccountSection = AccountSettingsSection` so the legacy AppShell remains buildable until Task 7.
- Guarantees: only the active account subpage is mounted; active credentials require an explicit reconnect after save, and that action stays bound to the account whose credentials were saved.

- [ ] **Step 1: Add the settings navigation types without breaking legacy navigation**

Add:

```ts
export type AccountSettingsSection = 'api' | 'assets' | 'risk'
export type AccountSection = AccountSettingsSection

export type SettingsSection =
  | 'general' | 'account' | 'plugins' | 'notifications'
  | 'searchCommands' | 'hotkeys' | 'workspace' | 'appearance'
  | 'languageRegion' | 'about'
```

Do not remove `NavKey.account` in this task.

- [ ] **Step 2: Add failing account-page tests**

Create tests that mount the page with `api`, `assets`, and `risk` initial sections. With a connected store, assert `assets` invokes `fetch_funding_balances` and not `get_risk_status`. Assert default `api` invokes `list_account_profiles` and neither hidden private request. Click the third `role="tab"` button and assert only `RiskControlPanel` mounts and calls `get_risk_status`.

Use this component contract in the test:

```ts
const wrapper = mount(AccountSettingsPage, {
  props: { initialSection: 'assets' },
  global: { plugins: [pinia] },
})
```

- [ ] **Step 3: Run the account-page test and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/accountSettingsPage.test.ts
```

Expected: failure because `AccountSettingsPage.vue` does not exist.

- [ ] **Step 4: Implement the account page with active-only mounting**

Use `AppTabs` and an `AccountSettingsSection` ref. Watch `initialSection` so an explicit parent deep link can update the page:

```ts
const props = withDefaults(defineProps<{ initialSection?: AccountSettingsSection }>(), {
  initialSection: 'api',
})
const activeSection = ref<AccountSettingsSection>(props.initialSection)
watch(() => props.initialSection, (section) => { activeSection.value = section })
```

Render `AccountProfilesPanel` with `v-if`, `AccountAssetsPanel` with `v-else-if` and `:active="true"`, and `RiskControlPanel` with `v-else` and `:active="true"`. Do not keep all three panels mounted.

- [ ] **Step 5: Add failing credential-reconnect tests**

Mount `AccountProfilesPanel` with an active `primary` profile and a connected connection store. Emit `saved` from `CredentialEditor` and assert no disconnect/connect occurs until clicking `data-testid="account-reconnect"`. After the click, assert disconnect precedes connect and the connect payload uses `configStore.config.useWebsocket`.

Add a failure test where disconnect succeeds and connect rejects; assert the UI says credentials were saved, renders the connection error with `role="alert"`, keeps the retry action, and never invokes `save_credentials` again. Add a non-active `backup` saved event and assert no reconnect banner appears. Add two account-binding tests:

- with `configStore.config = null`, clicking reconnect first calls `get_config`, then reconnects with the returned `useWebsocket: false`; there is no `?? true` fallback;
- after saving `primary`, change the authoritative active account to `backup`; the action disappears and clicking cannot reconnect `backup` using `primary`'s saved event.

- [ ] **Step 6: Run reconnect tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/accountProfilesReconnect.test.ts
```

Expected: failure because `AccountProfilesPanel` ignores `CredentialEditor.saved`.

- [ ] **Step 7: Wire explicit reconnect into AccountProfilesPanel**

Add Task 4 stores, a pending account ID, and a separate panel error. Connect the existing editor event:

```ts
const pendingReconnectAccountId = ref<string | null>(null)

function handleCredentialSaved(accountId: string): void {
  reconnectError.value = null
  pendingReconnectAccountId.value = accountId === store.activeAccountId
    && connectionStore.connected
    ? accountId
    : null
}

async function reconnectActiveAccount(): Promise<void> {
  const accountId = pendingReconnectAccountId.value
  if (!accountId || accountId !== store.activeAccountId) {
    pendingReconnectAccountId.value = null
    return
  }
  reconnectError.value = null
  try {
    const config = configStore.config ?? await configStore.fetchConfig()
    if (accountId !== store.activeAccountId) {
      pendingReconnectAccountId.value = null
      return
    }
    await connectionStore.reconnect(config.useWebsocket)
    pendingReconnectAccountId.value = null
  } catch (error) {
    reconnectError.value = reportError(error, '账户重新连接失败')
  }
}
```

Watch `store.activeAccountId` and clear a pending ID when it no longer matches. Also watch `connectionStore.connected`: clear pending on a later successful natural connection, or on an external `true → false` transition when `connectionStore.reconnecting` is false; retain it during the disconnect phase/failure of explicit reconnect. Render `凭据已保存，重新连接后生效` and the retryable action only while the pending ID still equals the active account. Reconnect failure must not be labeled as credential persistence failure.

- [ ] **Step 8: Verify and commit the account settings page**

```powershell
pnpm exec vitest run tests/frontend/accountSettingsPage.test.ts tests/frontend/accountProfilesReconnect.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/accountProfilesPanelFailures.test.ts tests/frontend/accountAssets.test.ts tests/frontend/riskPanel.test.ts tests/frontend/risk.test.ts
pnpm build
git add src/types/navigation.ts src/components/settings/AccountSettingsPage.vue src/components/account/AccountProfilesPanel.vue tests/frontend/accountSettingsPage.test.ts tests/frontend/accountProfilesReconnect.test.ts
git commit -m "feat(settings): add account settings page"
```

---

### Task 6: Build the Settings Center presentation and placeholders

**Files:**

- Create: `src/components/settings/settingsSections.ts`
- Create: `src/components/settings/SettingsSidebar.vue`
- Create: `src/components/settings/SettingsPlaceholder.vue`
- Create: `src/components/settings/AboutSettingsPanel.vue`
- Create: `src/components/settings/SettingsCenterPage.vue`
- Create: `src/components/settings/SettingsCenterPage.css`
- Create: `tests/frontend/settingsCenter.test.ts`

**Interfaces:**

- Consumes: `SettingsSection` and `AccountSettingsSection` from Task 5, `GeneralSettingsPanel` from Task 4, and `AccountSettingsPage` from Task 5.
- Produces: `SettingsCenterPage({ initialSection?, initialAccountSection? })` and `back` event for Task 7.
- Produces: ordered section metadata used by the dedicated sidebar and placeholders.

- [ ] **Step 1: Add failing metadata and rendering tests**

Mount `SettingsCenterPage` with the same Pinia created in `beforeEach`. Stub the two functional panels and assert the sidebar renders this exact order:

```ts
expect(wrapper.findAll('[data-testid^="settings-nav-"]').map((item) => item.text()))
  .toEqual([
    '通用', '账户', '插件', '通知', '搜索与命令', '快捷键',
    '工作区与窗口', '外观', '语言与地区', '关于',
  ])
```

Assert four group headings, `aria-current="page"` on General, and General content by default. Use `it.each` for all seven placeholder keys. Click each category and, within the placeholder component only, assert the exact description from Step 3, `规划中`, and no `button`, `a[href]`, `input`, `select`, `textarea`, `[role="switch"]`, or `[tabindex]`.

Set `useAppStore().markReady('0.4.1-test')`, click About, and assert EasiFlux plus the exact version. Mount with `initialSection: 'account', initialAccountSection: 'assets'` and assert the account page receives `assets`. Click `data-testid="settings-back"` and assert one `back` emission.

Use one Pinia per test and stub the functional children so presentation tests cannot invoke Tauri:

```ts
function mountCenter(props: Record<string, unknown> = {}) {
  return mount(SettingsCenterPage, {
    props,
    global: {
      plugins: [pinia],
      stubs: {
        GeneralSettingsPanel: {
          template: '<div data-testid="general-settings-stub" />',
        },
        AccountSettingsPage: {
          props: ['initialSection'],
          template: '<div data-testid="account-settings-stub" />',
        },
      },
    },
  })
}
```

Read the Account stub's `initialSection` prop rather than mounting real private panels.

- [ ] **Step 2: Run presentation tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/settingsCenter.test.ts
```

Expected: failure because the presentation components do not exist.

- [ ] **Step 3: Implement immutable settings metadata**

Define:

```ts
export interface SettingsSectionDefinition {
  key: SettingsSection
  label: string
  description: string
}

export interface SettingsSectionGroup {
  key: 'basic' | 'features' | 'personalization' | 'system'
  label: string
  items: readonly SettingsSectionDefinition[]
}

export const SETTINGS_SECTION_GROUPS: readonly SettingsSectionGroup[]
export const SETTINGS_SECTION_BY_KEY: Readonly<Record<SettingsSection, SettingsSectionDefinition>>
```

Use the four groups and ten labels from the spec. Placeholder descriptions are exact product copy and are asserted verbatim:

| Key | Description |
| --- | --- |
| `plugins` | `管理插件的启用状态、权限与插件级配置。` |
| `notifications` | `配置通知渠道、提醒方式与免打扰规则。` |
| `searchCommands` | `配置全局搜索与命令面板的行为。` |
| `hotkeys` | `查看并管理应用快捷键。` |
| `workspace` | `配置窗口布局、工作区保存与恢复行为。` |
| `appearance` | `配置主题、颜色与界面显示方式。` |
| `languageRegion` | `配置语言、地区与时间格式。` |

- [ ] **Step 4: Implement the sidebar, placeholder, About, and center page**

`SettingsSidebar` accepts `active: SettingsSection` and emits `select(section)`. Each button uses `data-testid="settings-nav-${key}"` and conditional `aria-current="page"`.

`SettingsCenterPage` accepts:

```ts
const props = withDefaults(defineProps<{
  initialSection?: SettingsSection
  initialAccountSection?: AccountSettingsSection
}>(), {
  initialSection: 'general',
  initialAccountSection: 'api',
})
const emit = defineEmits<{ back: [] }>()
```

Keep the active settings section local and non-persistent. Render explicit branches for General, Account, and About; all others use `SettingsPlaceholder`. The dedicated sidebar is fixed at 220–240px, the root has `min-width: 0`, `min-height: 0`, and `overflow: hidden`, and only the content pane scrolls.

- [ ] **Step 5: Run presentation tests, accessibility assertions, lint, and build**

```powershell
pnpm exec vitest run tests/frontend/settingsCenter.test.ts
pnpm lint
pnpm build
```

Expected: four groups, ten sections, placeholders, version, deep-linked account content, and back event pass.

- [ ] **Step 6: Commit the Settings Center presentation**

```powershell
git add src/components/settings/settingsSections.ts src/components/settings/SettingsSidebar.vue src/components/settings/SettingsPlaceholder.vue src/components/settings/AboutSettingsPanel.vue src/components/settings/SettingsCenterPage.vue src/components/settings/SettingsCenterPage.css tests/frontend/settingsCenter.test.ts
git commit -m "feat(settings): add settings center presentation"
```

---

### Task 7: Replace legacy account navigation with the full-page Settings route

**Files:**

- Modify: `src/types/navigation.ts`
- Modify: `src/components/layout/AppShell.vue`
- Modify: `src/components/layout/NavigationRail.vue`
- Modify: `src/components/layout/sidebarSections.ts`
- Modify: `src/components/dashboard/DashboardPage.vue`
- Modify: `src/components/dashboard/types.ts`
- Modify: `src/App.vue`
- Delete: `src/views/TradingView.vue`
- Modify: `tests/frontend/settingsCenter.test.ts`
- Modify: `tests/frontend/chartWorkspacePage.test.ts`
- Delete: `src/components/account/AccountCenterPage.vue`
- Delete: `tests/frontend/accountNavigation.test.ts`

**Interfaces:**

- Consumes: `SettingsCenterPage` from Task 6.
- Produces final `PrimaryPage`, `NavigationRequest`, `SettingsNavigationTarget`, and Dashboard account-assets deep link.
- Guarantees: gear is real navigation, previous non-settings page is remembered only for return, every new settings session remounts on General unless an explicit deep link is supplied.

- [ ] **Step 1: Add failing AppShell navigation tests**

Extend `settingsCenter.test.ts` with an AppShell mount helper and stubs for trading/chart heavy children. Assert:

- no primary `账户` button;
- clicking `设置` mounts `SettingsCenterPage`, selects the gear, defaults General, and hides the generic `Sidebar`;
- entering from Trading then clicking settings back returns to Trading;
- clicking Plugins directly leaves settings;
- the generic Sidebar exists on Home and Plugins but is not mounted on Trading, Charts, or Settings;
- selecting Notifications, leaving to Home, then clicking the gear remounts General;
- Dashboard `查看资产` opens settings/account/assets;
- after that deep link, leaving and normal gear entry still opens General.

Use exact assertions on `NavigationRail.props('active')` and `data-testid` values, not component implementation state.

- [ ] **Step 2: Run navigation tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/settingsCenter.test.ts tests/frontend/chartWorkspacePage.test.ts
```

Expected: old gear only emits `openSettings`, Account remains in the rail, and the settings page is unreachable.

- [ ] **Step 3: Finalize navigation types**

Replace legacy account navigation with:

```ts
export type PrimaryPage = 'home' | 'trading' | 'charts' | 'plugins' | 'settings'
export type NavKey = PrimaryPage
export type AccountSettingsSection = 'api' | 'assets' | 'risk'

export type SettingsNavigationTarget =
  | { page: 'settings'; settingsSection?: Exclude<SettingsSection, 'account'> }
  | { page: 'settings'; settingsSection: 'account'; accountSection?: AccountSettingsSection }

export type NavigationTarget =
  | { page: Exclude<PrimaryPage, 'settings'> }
  | SettingsNavigationTarget

export type NavigationRequest = PrimaryPage | NavigationTarget

export type HomeSection = 'welcome' | 'updates'
export type PluginSection = 'installed' | 'market' | 'manage'
export type SidebarSectionKey = HomeSection | PluginSection
export type SidebarTarget =
  | { page: 'home'; section: HomeSection }
  | { page: 'plugins'; section: PluginSection }
```

Remove the temporary `AccountSection` and `NonAccountSection` aliases. In `sidebarSections.ts`, type the map as `Partial<Record<'home' | 'plugins', SidebarSection[]>>` and delete the account group.

- [ ] **Step 4: Implement AppShell settings-session navigation**

Use these state variables and behavior:

```ts
const activePage = ref<PrimaryPage>('home')
const previousNonSettingsPage = ref<Exclude<PrimaryPage, 'settings'>>('home')
const settingsSessionId = ref(0)
const activeHomeSection = ref<HomeSection>('welcome')
const activePluginSection = ref<PluginSection>('installed')
const settingsTarget = ref({
  settingsSection: 'general' as SettingsSection,
  accountSection: 'api' as AccountSettingsSection,
})
```

Normalize and apply targets without persisting any settings section:

```ts
function normalizeNavigation(request: NavigationRequest): NavigationTarget {
  if (typeof request !== 'string') return request
  if (request === 'settings') return { page: 'settings' }
  return { page: request }
}

function applyNavigation(target: NavigationTarget): void {
  if (target.page === 'settings') {
    if (activePage.value !== 'settings') {
      previousNonSettingsPage.value = activePage.value
    }
    settingsTarget.value = target.settingsSection === 'account'
      ? {
          settingsSection: 'account',
          accountSection: target.accountSection ?? 'api',
        }
      : {
          settingsSection: target.settingsSection ?? 'general',
          accountSection: 'api',
        }
    settingsSessionId.value += 1
    activePage.value = 'settings'
    return
  }

  activePage.value = target.page
  if (target.page === 'trading') tradingVisited.value = true
  if (target.page === 'charts') chartsVisited.value = true
  if (target.page === 'home') activeHomeSection.value = 'welcome'
  if (target.page === 'plugins') activePluginSection.value = 'installed'
}
```

The generic sidebar target must be nullable and page-specific so Trading, Charts, and Settings cannot carry an unrelated secondary key:

```ts
const sidebarTarget = computed<SidebarTarget | null>(() => {
  if (activePage.value === 'home') {
    return { page: 'home', section: activeHomeSection.value }
  }
  if (activePage.value === 'plugins') {
    return { page: 'plugins', section: activePluginSection.value }
  }
  return null
})
```

Render the generic `Sidebar` only with `v-if="sidebarTarget"`. `selectSection` updates `activeHomeSection` only for `welcome | updates` on Home and `activePluginSection` only for `installed | market | manage` on Plugins.

`navigateTo` starts the existing chart flush, applies navigation immediately, and returns the pending promise. Do not `await` the flush before changing pages:

```ts
function navigateTo(request: NavigationRequest): Promise<void> {
  const normalized = normalizeNavigation(request)
  if (normalized.page === 'settings'
    && activePage.value === 'settings'
    && typeof request === 'string') {
    return Promise.resolve()
  }
  const pending = flushActiveChartWorkspace('page').catch((error: unknown) => {
    reportError(error, '图表页面切换前保存失败')
  })
  applyNavigation(normalized)
  return pending
}
```

Keep the deferred-flush assertion in `chartWorkspacePage.test.ts`: Settings is visible while the flush promise is still pending. When entering settings from a non-settings page, save that page, normalize missing settings fields to General/API, increment `settingsSessionId`, and mount:

```vue
<SettingsCenterPage
  v-if="activePage === 'settings'"
  :key="settingsSessionId"
  :initial-section="settingsTarget.settingsSection"
  :initial-account-section="settingsTarget.accountSection"
  @back="returnToWorkspace"
/>
```

Clicking the gear while already in settings is a no-op. `returnToWorkspace()` calls `navigateTo(previousNonSettingsPage.value)`.

- [ ] **Step 5: Remove legacy account routing atomically**

- Delete the User import and Account rail item; make the gear emit `select('settings')`.
- Remove `openSettings` emits from NavigationRail and AppShell.
- Remove the now-obsolete `@open-settings` listener from `App.vue`; keep its startup `SettingsDialog` state temporarily until Task 8 replaces that dialog.
- Delete the unreferenced `src/views/TradingView.vue`, which otherwise keeps forwarding AppShell's removed `openSettings` event and is still included by TypeScript.
- Remove `AccountCenterPage`, `activeAccountSection`, account sidebar handling, and the account page branch from AppShell.
- Remove the account group from `sidebarSections.ts`.
- Change Dashboard assets to:

```ts
emit('navigate', {
  page: 'settings',
  settingsSection: 'account',
  accountSection: 'assets',
})
```

- Remove `account` from `DashboardNavTarget`.
- Delete the now-unreferenced AccountCenterPage file only after `rg "AccountCenterPage|page: 'account'|NavKey.*account" src tests` shows that all valuable coverage has been migrated.

- [ ] **Step 6: Migrate chart and account navigation coverage**

In `chartWorkspacePage.test.ts`, replace account navigation stubs/targets with settings. Retain assertions that TopBar and NavigationRail remain the same DOM nodes, charts hide, settings displays, and `flushActiveChartWorkspace('page')` is called.

Move the old account test's three-panel and request-gating expectations to `accountSettingsPage.test.ts`; move rail/settings/deep-link expectations to `settingsCenter.test.ts`; then delete `accountNavigation.test.ts`.

- [ ] **Step 7: Run navigation and account regression tests**

```powershell
pnpm exec vitest run tests/frontend/settingsCenter.test.ts tests/frontend/chartWorkspacePage.test.ts tests/frontend/accountSettingsPage.test.ts tests/frontend/accountProfilesReconnect.test.ts tests/frontend/accountAssets.test.ts tests/frontend/riskPanel.test.ts
pnpm build
```

Expected: all final navigation, deep link, chart flush, account subpage, and strict TypeScript checks pass.

- [ ] **Step 8: Commit the atomic navigation migration**

```powershell
git add src/App.vue src/views/TradingView.vue src/types/navigation.ts src/components/layout/AppShell.vue src/components/layout/NavigationRail.vue src/components/layout/sidebarSections.ts src/components/dashboard/DashboardPage.vue src/components/dashboard/types.ts src/components/account/AccountCenterPage.vue tests/frontend/settingsCenter.test.ts tests/frontend/chartWorkspacePage.test.ts tests/frontend/accountNavigation.test.ts
git commit -m "feat(settings): add full-page settings navigation"
```

---

### Task 8: Narrow SettingsDialog into credential-only QuickSetup

**Files:**

- Modify: `src/App.vue`
- Create: `src/components/settings/QuickSetupDialog.vue`
- Create: `src/components/settings/QuickSetupDialog.css`
- Delete: `src/components/settings/SettingsDialog.vue`
- Delete: `src/components/settings/SettingsDialog.css`
- Create: `tests/frontend/appQuickSetup.test.ts`
- Modify: `tests/frontend/settingsCredentialEditor.test.ts`
- Modify: `tests/frontend/settingsCredentialFailures.test.ts`
- Modify: `tests/frontend/settingsProfileReadiness.test.ts`
- Modify: `tests/frontend/accountReconciliationUi.test.ts`
- Modify: `tests/frontend/accountProfilesConcurrency.test.ts`
- Modify: `tests/frontend/appAccountEventHandlers.test.ts`

**Interfaces:**

- Consumes: account profile three-state credential status and saved `config.useWebsocket`.
- Produces: `QuickSetupDialog({ show })`, `update:show`, and a retry action after credentials saved but connection failed.
- Guarantees: only `credentialState === 'missing'` opens QuickSetup at startup; config, Keyring-unavailable, account-list, and auto-connect failures use global errors without mislabeling credentials as missing.

- [ ] **Step 1: Add failing App startup tests for credential three-state handling**

Create `appQuickSetup.test.ts` with stubs for AppShell and QuickSetup. Mock startup commands in order: version, config, and `list_account_profiles`. Cover:

- active profile `missing`: QuickSetup show is true and connect is not invoked;
- active profile `present`: QuickSetup is false and connect uses saved `useWebsocket`;
- active profile `unavailable`: QuickSetup is false, connect is not invoked, and global error reporting receives a credential-storage error;
- config load rejection: QuickSetup remains false and the config error is reported;
- `list_account_profiles` rejection: QuickSetup remains false and the profile-list error is reported;
- the configured active account is absent from the returned list: QuickSetup remains false and `活动账户不在账户列表中` is reported;
- automatic connect rejection: QuickSetup remains false and the connection error is reported.
- with a present profile, mount the real AppShell once (stub its heavy page children), click the normal settings gear, and assert Settings Center opens while QuickSetup remains false.

Do not mock `has_credentials` in these tests; the final startup path must not call it.

- [ ] **Step 2: Run the startup tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/appQuickSetup.test.ts
```

Expected: failure because App still uses `has_credentials` and opens SettingsDialog for unrelated failures.

- [ ] **Step 3: Add failing QuickSetup workflow tests**

Update `settingsCredentialEditor.test.ts` to mount `QuickSetupDialog`. Verify:

- `save_credentials` occurs before `connect`;
- `save_config` is never invoked;
- connect payload uses the persisted WebSocket preference;
- credential save failure leaves the CredentialEditor draft and never invokes connect;
- credential save succeeds but connect fails: outer dialog says `凭据已保存，连接失败`, keeps show true, and exposes `data-testid="quick-setup-retry"`;
- clicking retry invokes only connect again with the saved preference and closes on success.
- after a failed connection, close and reopen the dialog; `credentialsSaved`, connection error, editor-open state, and any secret draft are reset, and no stale retry action remains.

Remove old assertions that QuickSetup owns WebSocket or ticker inputs.

- [ ] **Step 4: Run QuickSetup tests and confirm RED**

```powershell
pnpm exec vitest run tests/frontend/settingsCredentialEditor.test.ts tests/frontend/settingsCredentialFailures.test.ts
```

Expected: failure because SettingsDialog still mixes general settings and credentials.

- [ ] **Step 5: Implement credential-only QuickSetupDialog**

Reuse active profile summary, `AccountReconciliationStatus`, and `CredentialEditor`. Delete all `NForm`, `NSwitch`, `NInputNumber`, local general drafts, and `saveGeneralSettings()` code. Use this flow state:

```ts
const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ 'update:show': [value: boolean] }>()
const editorOpen = ref(false)
const credentialsSaved = ref(false)
const connecting = ref(false)
const connectionError = ref<string | null>(null)
let flowSession = 0

function resetFlow(): void {
  editorOpen.value = false
  credentialsSaved.value = false
  connecting.value = false
  connectionError.value = null
}

watch(
  () => props.show,
  (visible) => {
    flowSession += 1
    resetFlow()
    if (visible) void accountProfilesStore.refreshProfiles().catch((error) => {
      reportError(error, '加载账户配置失败')
    })
  },
  { immediate: true },
)

function closeFlow(): void {
  flowSession += 1
  resetFlow()
  emit('update:show', false)
}
```

After `CredentialEditor.saved`, connect stored credentials with authoritative config:

```ts
async function connectStoredCredentials(session = flowSession): Promise<void> {
  connecting.value = true
  connectionError.value = null
  try {
    const config = configStore.config ?? await configStore.fetchConfig()
    await connectionStore.connect(config.useWebsocket)
    if (session === flowSession && props.show) emit('update:show', false)
  } catch (error) {
    if (session === flowSession && props.show) {
      connectionError.value = reportError(error, '凭据已保存，连接失败')
    }
  } finally {
    if (session === flowSession) connecting.value = false
  }
}

function handleCredentialSaved(): void {
  credentialsSaved.value = true
  editorOpen.value = false
  void connectStoredCredentials()
}

function retryConnection(): void {
  if (!credentialsSaved.value || connecting.value) return
  void connectStoredCredentials()
}
```

Render `CredentialEditor` only when `!credentialsSaved`, connect it to `@saved="handleCredentialSaved"`, and unmount it on save or close so its secret draft cannot cross sessions. If connect fails after credentials save, keep `credentialsSaved = true`, show the separate connection error and `data-testid="quick-setup-retry"`, and let retry call only `retryConnection`. Do not reopen a required secret draft or roll back Keyring. Connection success closes through `update:show=false`; manual close uses `closeFlow`. The session token prevents a late connection completion from closing a later dialog session.

- [ ] **Step 6: Narrow App startup responsibility**

Rename `showSettings` to `showQuickSetup`, import QuickSetup, capture the successfully loaded config, and refresh account profiles. Use this exact branch order:

```ts
const config = await configStore.fetchConfig()
const profiles = await accountProfilesStore.refreshProfiles()
const activeProfile = profiles.find(
  (profile) => profile.accountId === accountProfilesStore.activeAccountId,
)
if (!activeProfile) {
  reportError('加载活动账户失败', new Error('活动账户不在账户列表中'))
  return
}
if (activeProfile.credentialState === 'missing') {
  showQuickSetup.value = true
  return
}
if (activeProfile.credentialState === 'unavailable') {
  reportError('读取账户凭据失败', new Error('凭据存储不可用'))
  return
}
await connectionStore.connect(config.useWebsocket)
```

Catch config, profile-list, and auto-connect failures separately and report them without opening QuickSetup. Remove `normalizeAccountId` and `hasCredentials` from App, and confirm the `AppShell @open-settings` listener removed in Task 7 has not been reintroduced.
Preserve the existing post-config startup work unchanged: adopt symbol/interval into the market store, load watchlist instruments, refresh environment, wait for Tauri listeners, start the time store, and retain every account/market/order event subscription.

- [ ] **Step 7: Update shared recovery and concurrency tests**

Replace component imports/mocks with QuickSetup in `settingsProfileReadiness`, `accountReconciliationUi`, `accountProfilesConcurrency`, and `appAccountEventHandlers`. Keep their existing stale-response, unavailable-Keyring, reconciliation, draft lifetime, and secret-redaction assertions. Delete obsolete assertions that the old dialog persists general config; Task 4 already owns that coverage. QuickSetup must never invoke `save_config`.

- [ ] **Step 8: Run the full QuickSetup regression slice**

```powershell
pnpm exec vitest run tests/frontend/appQuickSetup.test.ts tests/frontend/settingsCredentialEditor.test.ts tests/frontend/settingsCredentialFailures.test.ts tests/frontend/settingsProfileReadiness.test.ts tests/frontend/accountReconciliationUi.test.ts tests/frontend/accountProfilesConcurrency.test.ts tests/frontend/appAccountEventHandlers.test.ts tests/frontend/connection.test.ts
pnpm lint
pnpm build
```

Expected: startup three-state logic, credential save/retry, reconciliation, concurrency, secret lifetime, and strict TypeScript all pass.

- [ ] **Step 9: Commit QuickSetup separation**

```powershell
git add src/App.vue src/components/settings/QuickSetupDialog.vue src/components/settings/QuickSetupDialog.css src/components/settings/SettingsDialog.vue src/components/settings/SettingsDialog.css tests/frontend/appQuickSetup.test.ts tests/frontend/settingsCredentialEditor.test.ts tests/frontend/settingsCredentialFailures.test.ts tests/frontend/settingsProfileReadiness.test.ts tests/frontend/accountReconciliationUi.test.ts tests/frontend/accountProfilesConcurrency.test.ts tests/frontend/appAccountEventHandlers.test.ts
git commit -m "feat(settings): separate credential quick setup"
```

---

### Task 9: Run full verification and desktop acceptance

**Files:**

- No planned source files. Any failure is fixed in the task that owns the behavior and verified with that task's focused commands before rerunning this task.

**Interfaces:**

- Consumes: all Task 1–8 commits.
- Produces: verified automated and manual acceptance evidence; no empty or catch-all “final fixes” commit.

- [ ] **Step 1: Inspect final change scope**

```powershell
git status --short --branch
git diff --check main...HEAD
git diff --stat main...HEAD
git log --oneline --decorate main..HEAD
```

Expected: only PRD-12 documents and implementation files are committed; no unrelated file is staged. `src-tauri/Cargo.toml` contains only Task 1's feature addition relative to main.

- [ ] **Step 2: Run the focused Settings Center regression**

```powershell
pnpm exec vitest run tests/frontend/generalSettingsAutosave.test.ts tests/frontend/generalSettingsPanel.test.ts tests/frontend/settingsCenter.test.ts tests/frontend/accountSettingsPage.test.ts tests/frontend/accountProfilesReconnect.test.ts tests/frontend/appQuickSetup.test.ts tests/frontend/settingsCredentialEditor.test.ts tests/frontend/settingsCredentialFailures.test.ts tests/frontend/settingsProfileReadiness.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/accountProfilesConcurrency.test.ts tests/frontend/accountReconciliationUi.test.ts tests/frontend/accountAssets.test.ts tests/frontend/riskPanel.test.ts tests/frontend/risk.test.ts tests/frontend/connection.test.ts tests/frontend/appAccountEventHandlers.test.ts tests/frontend/chartWorkspacePage.test.ts
```

Expected: every focused frontend test passes.

- [ ] **Step 3: Run all frontend gates**

```powershell
pnpm lint
pnpm test
pnpm build
```

Expected: ESLint, the complete Vitest suite, `vue-tsc --noEmit`, and the production Vite build all pass.

- [ ] **Step 4: Run all Rust gates**

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

Expected: the complete Rust tests, Clippy, and build pass.

- [ ] **Step 5: Run and classify the repository-wide Rust format check**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
```

Expected: touched Rust files are formatted. If the known unrelated formatting debt still fails, record exact filenames and diff summary; do not format those files as part of PRD-12.

- [ ] **Step 6: Run the desktop application**

```powershell
pnpm tauri dev
```

Expected: a real Tauri window opens; do not substitute browser-only Vite mode for IPC acceptance.

- [ ] **Step 7: Verify navigation and layout manually**

At 1024×640 and the normal 1400×900 window size, verify: gear opens full-page Settings on General; gear is selected; return goes to the prior page; primary navigation exits settings; no Account rail item exists; Dashboard assets opens Settings/Account/Assets; a later normal gear entry returns to General; there is no page-level horizontal scroll; all ten categories and account tabs are keyboard reachable.

- [ ] **Step 8: Verify general settings manually**

Change polling interval and confirm the saved state appears, the runtime MarketFallback cadence changes, and the value survives restart. Change WebSocket preference while connected and confirm the connection is not interrupted; click explicit reconnect and confirm disconnect precedes connect. Force a reconnect failure and confirm saved preference and connection error are shown separately.

- [ ] **Step 9: Verify account and QuickSetup behavior manually**

Verify API, assets, and risk tabs; hidden tabs do not start private loading. Add/edit/switch/delete test accounts without using production Keyring records. Confirm secrets never render, deletion names the target, risk remains explicit, asset section errors stay isolated, and active credential edits show explicit reconnect. With a disposable Windows profile or test Keyring service, verify missing credentials open QuickSetup, present credentials do not, unavailable storage is not labeled missing, and auto-connect failure does not open QuickSetup.

- [ ] **Step 10: Stop the development process and capture final status**

Stop `pnpm tauri dev`, then run:

```powershell
git status --short --branch
git log --oneline --decorate main..HEAD
```

Expected: no untracked runtime artifacts are present; only any explicitly documented pre-existing state remains. Report command outputs and manual acceptance results before claiming PRD-12 complete.
