# Significant Bug Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan. In this repository, use inline execution unless the user explicitly authorizes subagents.

**Goal:** Fix the confirmed risk-control, environment-probe, instrument-retry, and hidden-tab request bugs without changing public trading behavior or touching the legacy Desktop/SDK repositories.

**Architecture:** Replace the process-local risk counter with a timezone-aware, recoverable `risk_usage.toml` ledger and an atomic reservation API. Keep network probing in the scheduler while making the getter a pure cache read. Treat the instrument promise as in-flight state only, and gate private-panel refreshes on actual tab activation.

**Tech Stack:** Rust stable, Tokio, Tauri 2, serde/toml, chrono/chrono-tz, Vue 3, TypeScript strict, Pinia, Vitest, Vue Test Utils.

## Global Constraints

- Modify only `G:\EasiFlux\EasiFlux-Desktop-Tauri`.
- Do not embed Python or change the legacy Desktop/SDK repositories.
- Keep business rules in Rust; Vue only coordinates commands, stores, and rendering.
- Preserve the existing `AppConfig` persistence schema. The new usage ledger is independent from `config.toml`.
- Add no runtime dependency for atomic file replacement; use `std::fs` and a main/backup/temp recovery sequence compatible with Windows.
- Follow test-driven development: add a focused failing test, implement the minimum behavior, then rerun the focused test.
- Do not create Git commits unless the user explicitly requests them. The commit messages below are checkpoints only.

---

### Task 1: Add the recoverable daily-risk ledger and trading-day key

**Files:**

- Create: `src-tauri/src/storage/risk_usage.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/services/time.rs`

**Step 1: Write failing storage tests**

Add tests in `src-tauri/src/storage/risk_usage.rs` covering:

```rust
#[test]
fn round_trips_usage() {
    let path = test_path("round-trip");
    let store = RiskUsageStore::with_path(path.clone());
    let usage = RiskUsage {
        schema_version: 1,
        trading_day: "2026-07-21".into(),
        timezone: "Asia/Shanghai".into(),
        occupied_orders: 7,
        updated_at_ms: 1_784_606_400_000,
    };

    store.save(&usage).unwrap();
    assert_eq!(store.load().unwrap(), Some(usage));
    cleanup_test_files(&path);
}

#[test]
fn refuses_stale_backup_when_main_is_corrupt() {
    let path = test_path("corrupt-main");
    let store = RiskUsageStore::with_path(path.clone());
    let first = sample_usage(3);
    let second = sample_usage(4);

    store.save(&first).unwrap();
    store.save(&second).unwrap();
    std::fs::write(&path, "not toml").unwrap();

    assert!(store.load().is_err());
    cleanup_test_files(&path);
}

#[test]
fn recovers_complete_temp_when_main_is_missing() {
    let path = test_path("temp-recovery");
    let store = RiskUsageStore::with_path(path.clone());
    let usage = sample_usage(4);
    std::fs::write(path.with_extension("toml.tmp"), toml::to_string(&usage).unwrap()).unwrap();

    assert_eq!(store.load().unwrap(), Some(usage));
    cleanup_test_files(&path);
}

#[test]
fn reports_error_when_all_candidates_are_corrupt() {
    let path = test_path("corrupt");
    std::fs::write(&path, "not toml").unwrap();
    assert!(RiskUsageStore::with_path(path.clone()).load().is_err());
    cleanup_test_files(&path);
}
```

Use a unique path under `std::env::temp_dir()` based on process id plus an atomic test sequence; do not add `tempfile`.

**Step 2: Run the focused test and confirm failure**

Run:

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri\src-tauri
cargo test storage::risk_usage -- --nocapture
```

Expected: compilation fails because `risk_usage` and its types do not exist.

**Step 3: Implement the ledger**

Create these public types:

```rust
pub const RISK_USAGE_FILENAME: &str = "risk_usage.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskUsage {
    pub schema_version: u32,
    pub trading_day: String,
    pub timezone: String,
    pub occupied_orders: u32,
    pub updated_at_ms: u64,
}

pub struct RiskUsageStore {
    path: PathBuf,
}

impl RiskUsageStore {
    pub fn new() -> Self;
    pub(crate) fn with_path(path: PathBuf) -> Self;
    pub fn load(&self) -> AppResult<Option<RiskUsage>>;
    pub fn save(&self, usage: &RiskUsage) -> AppResult<()>;
}
```

`new()` resolves the same application data directory used by `ConfigStore` and appends `risk_usage.toml`. `load()` trusts a valid `risk_usage.toml`. If the main file is missing and a complete `risk_usage.toml.tmp` exists, recover that interrupted replacement. A corrupt or unreadable main file and a backup-only state return `AppError::Storage`; never silently load `.bak`, because it may contain a smaller, stale count. Missing all candidates means no usage has been recorded yet.

`save()` performs the following while its caller holds the risk mutex:

```rust
let serialized = toml::to_string_pretty(usage)?;
write_and_sync(tmp_path, serialized.as_bytes())?;
remove_existing_backup_if_present()?;
rename_main_to_backup_if_present()?;
rename_tmp_to_main_or_restore_backup()?;
```

Flush the temp file with `sync_all()` before renaming. If the final rename fails, attempt to restore the backup and return a storage error. Never mutate in-memory usage until `save()` succeeds.

Export `RiskUsage` and `RiskUsageStore` from `storage/mod.rs`.

**Step 4: Add the trading-day key test**

In `services/time.rs`:

```rust
#[test]
fn trading_day_key_uses_configured_timezone() {
    // 2026-07-21 00:30:00 +08:00 is still 2026-07-20 in UTC.
    let now_ms = 1_784_565_000_000;
    assert_eq!(trading_day_key(now_ms, "Asia/Shanghai"), "2026-07-21");
    assert_eq!(trading_day_key(now_ms, "UTC"), "2026-07-20");
}
```

Implement:

```rust
pub fn trading_day_key(now_ms: u64, timezone: &str) -> String {
    use chrono::{TimeZone, Utc};
    let resolved = resolve_trading_day_timezone(timezone);
    let tz = resolved.parse::<chrono_tz::Tz>().unwrap_or(chrono_tz::Asia::Shanghai);
    let now = Utc.timestamp_millis_opt(now_ms as i64).single().unwrap_or_else(Utc::now);
    now.with_timezone(&tz).date_naive().format("%Y-%m-%d").to_string()
}
```

Use a verified timestamp in the final test; calculate it once and keep the assertion deterministic.

**Step 5: Run focused tests**

```powershell
cargo test storage::risk_usage -- --nocapture
cargo test services::time -- --nocapture
```

Expected: all focused tests pass.

**Checkpoint commit message (do not commit automatically):** `feat(risk): add persistent daily usage ledger`

---

### Task 2: Replace validation-side counting with an atomic reservation API

**Files:**

- Modify: `src-tauri/src/models/config.rs`
- Modify: `src-tauri/src/services/risk.rs`

**Step 1: Write failing risk-service tests**

Replace the single oversized-order test with focused coverage for:

```rust
#[test]
fn rejects_zero_negative_and_invalid_quantities() { /* 0, -1, abc */ }

#[test]
fn rejects_non_positive_limit_price() { /* 0 and -1 */ }

#[test]
fn enforces_daily_limit_across_service_restart() {
    let path = test_path("restart");
    let now = shanghai_noon("2026-07-21");
    let request = market_order("1");

    {
        let service = service_with_limit(&path, 1);
        service.reserve_order(&request, None, now).unwrap();
    }
    let restarted = service_with_limit(&path, 1);
    assert!(restarted.reserve_order(&request, None, now).is_err());
    cleanup_test_files(&path);
}

#[test]
fn rotates_usage_on_configured_timezone_day_boundary() {
    let service = service_with_limit(&test_path("rotate"), 1);
    service.reserve_order(&market_order("1"), None, before_midnight()).unwrap();
    assert!(service.reserve_order(&market_order("1"), None, after_midnight()).is_ok());
}

#[test]
fn release_restores_quota_after_submission_failure() {
    let service = service_with_limit(&test_path("release"), 1);
    let reservation = service.reserve_order(&market_order("1"), None, now()).unwrap();
    service.release_reservation(&reservation, now()).unwrap();
    assert!(service.reserve_order(&market_order("1"), None, now()).is_ok());
}

#[test]
fn concurrent_reservations_cannot_overshoot_limit() {
    let service = Arc::new(service_with_limit(&test_path("concurrent"), 1));
    let successes = thread::scope(|scope| {
        // Start multiple workers behind a Barrier and count successful reservations.
    });
    assert_eq!(successes, 1);
}
```

Also cover fail-closed persistence behavior by constructing a store under a path whose parent is a regular file and asserting that reservation returns `AppError::Storage` before quota is granted.

**Step 2: Run the focused test and confirm failure**

```powershell
cargo test services::risk -- --nocapture
```

Expected: compilation fails because `reserve_order`, `release_reservation`, and the injectable store do not exist.

**Step 3: Carry timezone into `RiskConfig`**

Add:

```rust
pub struct RiskConfig {
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
    pub enabled: bool,
}
```

Default it to `DEFAULT_TRADING_DAY_TIMEZONE`, and copy `AppConfig.trading_day_timezone` in `From<&AppConfig>`.

**Step 4: Implement the reservation state machine**

Replace `AtomicU32` with:

```rust
#[derive(Debug, Clone)]
pub struct RiskReservation {
    trading_day: String,
    counted: bool,
}

struct RiskUsageState {
    usage: Option<RiskUsage>,
    load_error: Option<String>,
}

pub struct RiskService {
    config: RiskConfig,
    store: RiskUsageStore,
    usage: std::sync::Mutex<RiskUsageState>,
}
```

Constructors:

```rust
pub fn new(config: RiskConfig) -> Self;
pub(crate) fn with_store(config: RiskConfig, store: RiskUsageStore) -> Self;
```

`with_store()` attempts `store.load()`. A corrupt/unreadable ledger must not crash application startup: retain the error in `load_error`. When risk is enabled, `reserve_order()` returns a risk/storage error until persistence is usable; when risk is disabled it returns a non-counted reservation and does not touch the ledger.

Reservation sequence:

```rust
pub fn reserve_order(
    &self,
    request: &PlaceOrderRequest,
    reference_price: Option<&str>,
    now_ms: u64,
) -> AppResult<RiskReservation> {
    if !self.config.enabled {
        return Ok(RiskReservation::not_counted());
    }

    validate_qty_and_price(request, reference_price, &self.config)?;
    let timezone = resolve_trading_day_timezone(&self.config.trading_day_timezone);
    let day = trading_day_key(now_ms, &timezone);
    let mut state = self.usage.lock().map_err(poisoned_lock_error)?;
    fail_if_initial_load_failed(&state)?;

    let mut next = usage_for_day(&state, &day, &timezone, now_ms);
    if next.occupied_orders >= self.config.max_daily_orders {
        return Err(AppError::Risk(/* daily-limit message */));
    }
    next.occupied_orders += 1;
    next.updated_at_ms = now_ms;
    self.store.save(&next)?;
    state.usage = Some(next);
    Ok(RiskReservation::counted(day))
}
```

Validation must reject malformed, zero, and negative quantities instead of coercing them to zero. Limit orders must have a present, parseable, strictly positive price. Keep the existing maximum quantity and deviation checks.

Release sequence:

```rust
pub fn release_reservation(
    &self,
    reservation: &RiskReservation,
    now_ms: u64,
) -> AppResult<()> {
    if !reservation.counted {
        return Ok(());
    }
    let mut state = self.usage.lock().map_err(poisoned_lock_error)?;
    let Some(current) = state.usage.as_ref() else { return Ok(()); };
    if current.trading_day != reservation.trading_day {
        return Ok(());
    }
    let mut next = current.clone();
    next.occupied_orders = next.occupied_orders.saturating_sub(1);
    next.updated_at_ms = now_ms;
    self.store.save(&next)?;
    state.usage = Some(next);
    Ok(())
}
```

Both reserve and release serialize the clone/persist/commit sequence under the same mutex. This is the concurrency guarantee; do not split the limit check and increment across locks.

**Step 5: Run focused tests**

```powershell
cargo test services::risk -- --nocapture
```

Expected: all risk tests pass, including restart persistence and concurrent limit enforcement.

**Checkpoint commit message (do not commit automatically):** `fix(risk): enforce persistent timezone-aware daily quota`

---

### Task 3: Reserve before submission and release only on submission failure

**Files:**

- Modify: `src-tauri/src/services/trading.rs`
- Modify: `src-tauri/src/state.rs`

**Step 1: Add failing helper tests**

Add a module-private, closure-based seam so the risk behavior is testable without real HTTP:

```rust
#[tokio::test]
async fn submission_error_releases_reservation() {
    let risk = Arc::new(tokio::sync::RwLock::new(risk_with_limit(1)));
    let request = market_order("1");

    let result = execute_reserved_order(
        &risk,
        &request,
        None,
        now(),
        || async { Err(AppError::Api("rejected".into())) },
    ).await;

    assert!(result.is_err());
    assert!(risk.read().await.reserve_order(&request, None, now()).is_ok());
}

#[tokio::test]
async fn reservation_failure_prevents_submission() {
    let called = Arc::new(AtomicBool::new(false));
    // Pre-fill a limit=1 ledger, then execute again.
    let result = execute_reserved_order(/* ... */, || {
        called.store(true, Ordering::SeqCst);
        async { unreachable!() }
    }).await;
    assert!(result.is_err());
    assert!(!called.load(Ordering::SeqCst));
}
```

Use `AppError` variants that exist in `error.rs`; do not invent a new dependency or mock framework.

**Step 2: Run and confirm failure**

```powershell
cargo test services::trading -- --nocapture
```

Expected: compilation fails because `execute_reserved_order` does not exist.

**Step 3: Implement the test seam and wire production order placement**

Add `time: Arc<TimeService>` to `TradingService` and its constructor. In `state.rs`, pass the already-constructed `TimeService` when building `TradingService`.

Implement:

```rust
async fn execute_reserved_order<F, Fut>(
    risk: &Arc<tokio::sync::RwLock<RiskService>>,
    request: &PlaceOrderRequest,
    reference_price: Option<&str>,
    now_ms: u64,
    submit: F,
) -> AppResult<Order>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Order>>,
{
    let reservation = risk
        .read()
        .await
        .reserve_order(request, reference_price, now_ms)?;

    match submit().await {
        Ok(order) => Ok(order),
        Err(submit_error) => {
            if let Err(release_error) = risk
                .read()
                .await
                .release_reservation(&reservation, now_ms)
            {
                return Err(AppError::Internal(format!(
                    "order submission failed: {}; risk reservation release failed: {}",
                    submit_error.user_message(),
                    release_error.user_message(),
                )));
            }
            Err(submit_error)
        }
    }
}
```

Then `place_order()` calls it with `self.time.now_ms()` and `PrivateApi::create_order(&self.api, &request)`. A successful API response retains the occupied count. A deterministic rejection releases it. `Connection` and `Internal` errors cannot prove that the server did not accept the order, so they retain the reservation conservatively. A process crash after durable reservation remains counted by design.

**Step 4: Run focused and integration compilation tests**

```powershell
cargo test services::trading -- --nocapture
cargo test --all-targets
```

Expected: all tests compile and pass.

**Checkpoint commit message (do not commit automatically):** `fix(trading): roll back quota after failed submission`

---

### Task 4: Make environment reads side-effect free and probe the selected base URL

**Files:**

- Modify: `src-tauri/src/api/client.rs`
- Modify: `src-tauri/src/services/scheduler.rs`
- Modify: `src-tauri/src/commands/app.rs`
- Modify: `tests/frontend/appEnvironment.test.ts`

**Step 1: Add failing Rust tests for explicit probe targeting and cached reads**

In `api/client.rs`:

```rust
#[tokio::test]
async fn set_base_url_changes_public_request_target_without_credentials() {
    let client = ApiClient::new();
    client.set_base_url("https://sandbox.example.test/").await;
    assert_eq!(client.base_url().await, "https://sandbox.example.test");
}
```

In `commands/app.rs`, extract a small helper used by the command:

```rust
async fn cached_environment_status(
    status: &tokio::sync::RwLock<EnvironmentStatus>,
) -> EnvironmentStatus {
    status.read().await.clone()
}

#[tokio::test]
async fn cached_environment_read_returns_without_scheduler_work() {
    let status = RwLock::new(EnvironmentStatus::default());
    assert_eq!(cached_environment_status(&status).await, EnvironmentStatus::default());
}
```

If `EnvironmentStatus` lacks `PartialEq`, assert its fields instead of changing a public derive only for the test.

**Step 2: Run and confirm failure**

```powershell
cargo test api::client services::scheduler commands::app -- --nocapture
```

If Cargo accepts only one filter, run the three filters separately. Expected: failure because `set_base_url`/cached helper do not exist.

**Step 3: Implement exact-target probing**

Add:

```rust
pub async fn set_base_url(&self, base_url: &str) {
    *self.base_url.write().await = normalize_base_url(base_url);
}
```

In `scheduler.rs`, replace the duplicated method bodies with one shared free function used by both `SchedulerService::run_environment` and `SchedulerRefs::run_environment`:

```rust
async fn probe_environment(
    api: &Arc<ApiClient>,
    config: &Arc<RwLock<AppConfig>>,
    time: &Arc<TimeService>,
    emitter: &EventEmitter,
    cache: &Arc<RwLock<EnvironmentStatus>>,
) -> AppResult<()> {
    let account_id = normalize_account_id(&config.read().await.active_account_id);
    let credential = CredentialStore::load(&account_id)?;
    let selected_base_url = credential
        .as_ref()
        .map(|item| item.base_url.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    let probe_client = ApiClient::new();
    probe_client.set_base_url(&selected_base_url).await;
    let result = PublicApi::server_time(&probe_client).await;
    // Build status from selected_base_url and result, emit it, then update cache.
}
```

Prefer a small `ApiClient::for_base_url`/clone helper if `ApiClient::new` requires state that should be reused. The key invariant is that the client used by `PublicApi::server_time` has the same normalized base URL reported by `EnvironmentStatus`. Do not mutate the live connected client merely to probe an account.

Change `get_environment_status` to only clone `state.scheduler.environment_status()` (or the existing cached field). It must not invoke `run_task("environment")`.

**Step 4: Strengthen the frontend command-sequence test**

In `tests/frontend/appEnvironment.test.ts`, assert one scheduler trigger followed by one getter:

```ts
expect(tauriInvoke.mock.calls.map(([command]) => command)).toEqual([
  'scheduler_run_task',
  'get_environment_status',
])
expect(tauriInvoke).toHaveBeenCalledTimes(2)
```

The frontend flow remains: explicitly run the environment task, then read the cache.

**Step 5: Run focused tests**

```powershell
cargo test api::client -- --nocapture
cargo test commands::app -- --nocapture
.\node_modules\.bin\vitest.cmd run tests\frontend\appEnvironment.test.ts
```

Expected: all focused tests pass; a refresh performs one server-time probe total.

**Checkpoint commit message (do not commit automatically):** `fix(environment): probe selected endpoint once`

---

### Task 5: Allow instrument loading to retry after failure

**Files:**

- Modify: `src/stores/market.ts`
- Create: `tests/frontend/marketInstrumentsRetry.test.ts`

**Step 1: Write failing Pinia tests**

Mock `tauriInvoke` and isolate `localStorage` per test:

```ts
it('retries after the first request fails', async () => {
  tauriInvoke
    .mockRejectedValueOnce(new Error('offline'))
    .mockResolvedValueOnce([{ symbol: 'BTCUSDT' }])

  const store = useMarketStore()
  await store.loadInstruments(['ETHUSDT'])
  expect(store.symbols).toEqual(['ETHUSDT'])

  await store.loadInstruments(['ETHUSDT'])
  expect(tauriInvoke).toHaveBeenCalledTimes(2)
  expect(store.symbols).toEqual(['BTCUSDT'])
  expect(store.instrumentsError).toBeNull()
})

it('deduplicates only concurrent in-flight requests', async () => {
  const deferred = createDeferred<unknown>()
  tauriInvoke.mockReturnValueOnce(deferred.promise)
  const store = useMarketStore()

  const first = store.loadInstruments()
  const second = store.loadInstruments()
  expect(tauriInvoke).toHaveBeenCalledTimes(1)

  deferred.resolve([{ symbol: 'BTCUSDT' }])
  await Promise.all([first, second])
  await store.loadInstruments()
  expect(tauriInvoke).toHaveBeenCalledTimes(1)
})
```

**Step 2: Run and confirm failure**

```powershell
.\node_modules\.bin\vitest.cmd run tests\frontend\marketInstrumentsRetry.test.ts
```

Expected: the retry test fails because the rejected promise remains cached.

**Step 3: Implement in-flight versus loaded state**

In `market.ts`:

```ts
let instrumentsLoaded = false
let instrumentsFetchPromise: Promise<void> | null = null

async function loadInstruments(fallbackSymbols: string[] = []): Promise<void> {
  // Populate cache/fallback first as today.
  if (instrumentsLoaded) return
  if (instrumentsFetchPromise) return instrumentsFetchPromise

  instrumentsFetchPromise = (async () => {
    symbolsLoading.value = true
    try {
      const payload = await tauriInvoke<unknown>('fetch_instruments', {})
      const parsed = parseInstrumentSymbols(payload)
      if (parsed.length > 0) {
        symbols.value = parsed
        writeCachedSymbols(parsed)
        instrumentsLoaded = true
      }
      instrumentsRequest.setData(parsed)
    } catch {
      instrumentsRequest.setError('交易对加载失败')
      if (symbols.value.length === 0 && fallbackSymbols.length > 0) {
        symbols.value = [...fallbackSymbols]
      }
    } finally {
      instrumentsFetchPromise = null
      symbolsLoading.value = false
    }
  })()

  return instrumentsFetchPromise
}
```

An empty parsed response remains retryable. A non-empty successful response sets `instrumentsLoaded`. `useAsyncState.setData` clears the previous error.

**Step 4: Run focused tests**

```powershell
.\node_modules\.bin\vitest.cmd run tests\frontend\marketInstrumentsRetry.test.ts
```

Expected: retry and concurrent-deduplication tests pass.

**Checkpoint commit message (do not commit automatically):** `fix(market): retry instrument fetch after failure`

---

### Task 6: Prevent hidden order-center tabs from issuing private requests

**Files:**

- Modify: `src/composables/useOrderCenterRefresh.ts`
- Modify: `src/components/trading/order-center/TradeFillsTab.vue`
- Modify: `src/components/trading/order-center/ClosedPnlTab.vue`
- Create: `tests/frontend/orderCenterActivation.test.ts`

**Step 1: Write failing activation tests**

Mount a small harness for `useOrderCenterRefresh` and mount both record tabs with `active: false`. Install a fresh Pinia and set the connection status to connected before mount.

```ts
it('does not refresh private panels while inactive', async () => {
  mount(RefreshHarness, { props: { active: false }, global: { plugins: [pinia] } })
  await flushPromises()
  expect(refreshSyncTask).not.toHaveBeenCalled()
})

it('refreshes private panels once when activated', async () => {
  const wrapper = mount(RefreshHarness, { props: { active: false }, global: { plugins: [pinia] } })
  await wrapper.setProps({ active: true })
  await flushPromises()
  expect(refreshSyncTask).toHaveBeenCalledTimes(1)
})

it.each([
  [TradeFillsTab, 'fetch_trade_fills'],
  [ClosedPnlTab, 'fetch_closed_pnl'],
])('defers record requests until activation', async (component, command) => {
  const wrapper = mount(component, {
    props: { active: false },
    global: { plugins: [pinia], stubs: { TanstackDataTable: true } },
  })
  await flushPromises()
  expect(tauriInvoke).not.toHaveBeenCalledWith(command, expect.anything())

  await wrapper.setProps({ active: true })
  await flushPromises()
  expect(tauriInvoke).toHaveBeenCalledWith(command, expect.anything())
})
```

Add a separate assertion that transitioning the connection from disconnected to connected while `active=false` does not refresh. This catches the watcher path as well as `onMounted`.

**Step 2: Run and confirm failure**

```powershell
.\node_modules\.bin\vitest.cmd run tests\frontend\orderCenterActivation.test.ts
```

Expected: inactive-mount and hidden-connection tests fail because current hooks refresh unconditionally.

**Step 3: Gate every automatic refresh path**

In `useOrderCenterRefresh.ts`:

```ts
onMounted(() => {
  if (toValue(active)) void refresh()
})

watch(connected, (isConnected) => {
  if (isConnected && toValue(active)) void refresh()
})
```

Keep the existing active watcher so the first activation triggers refresh.

Apply the equivalent guard to both record tabs:

```ts
onMounted(() => {
  if (props.active) void refresh()
})

watch(connected, (isConnected) => {
  if (isConnected && props.active) void refresh()
})
```

Keep `v-show` in the parent so mounted tab state is preserved. Remove any dynamic parent `key` tied to `activeTab`, otherwise Vue still destroys and recreates the subtree. Do not switch to `v-if`.

**Step 4: Run focused tests**

```powershell
.\node_modules\.bin\vitest.cmd run tests\frontend\orderCenterActivation.test.ts
```

Expected: hidden tabs make zero private requests; activation makes exactly one request per activated path.

**Checkpoint commit message (do not commit automatically):** `perf(ui): defer hidden order tab requests`

---

### Task 7: Full validation and regression review

**Files:**

- Review all files changed in Tasks 1-6
- Update the design/plan only if implementation exposes a necessary deviation

**Step 1: Run frontend verification**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
.\node_modules\.bin\eslint.cmd .
.\node_modules\.bin\vitest.cmd run
.\node_modules\.bin\vue-tsc.cmd --noEmit
.\node_modules\.bin\vite.cmd build
```

Expected:

- ESLint has zero errors. Existing formatting warnings may remain only in untouched files.
- All Vitest tests pass.
- TypeScript strict checking passes.
- Production Vite build succeeds.

**Step 2: Run Rust verification**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri\src-tauri
rustfmt --edition 2021 --check src/api/client.rs src/commands/app.rs src/error.rs src/models/config.rs src/services/risk.rs src/services/scheduler.rs src/services/time.rs src/services/trading.rs src/state.rs src/storage/risk_usage.rs
cargo test --all-targets
cargo clippy --all-targets
```

Expected:

- Formatting check passes.
- All Rust tests pass.
- Clippy exits successfully under the repository's current CI policy. Existing unrelated warnings are not expanded into this patch.

**Step 3: Review the diff for scope and invariants**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
git status --short
git diff --check
git diff --stat
git diff -- src-tauri/src src tests docs/superpowers
```

Confirm:

- Only this repository changed.
- No generated build output, `.pnpm-store`, secrets, or credential files are present.
- Risk quota survives restart, rotates by configured timezone, and cannot overshoot concurrently within one application process. Cross-process locking/single-instance enforcement remains outside this plan.
- Deterministic submission failures release quota; successful, connection-ambiguous, and crash outcomes remain counted conservatively.
- Environment refresh performs one explicit probe against the same URL it reports.
- Instrument failures are retryable and simultaneous callers remain deduplicated.
- Hidden tabs are quiet until active.

**Step 4: Request code review before completion claim**

Apply `superpowers:requesting-code-review` to the final diff, resolve any high-confidence issues, rerun affected tests, then apply `superpowers:verification-before-completion` before reporting success.

**Suggested final commit sequence (only if the user later asks to commit):**

```text
feat(risk): add persistent daily usage ledger
fix(trading): roll back quota after failed submission
fix(environment): probe selected endpoint once
fix(market): retry instrument fetch after failure
perf(ui): defer hidden order tab requests
```
