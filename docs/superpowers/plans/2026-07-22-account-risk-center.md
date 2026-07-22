# Account and Risk Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a production-safe single-active-account center with credential lifecycle, read-only assets, and configurable persistent daily-risk status, while preserving the existing trading protocol and application-global risk quota.

**Architecture:** Keep secrets in the Rust/Keyring boundary and expose only sanitized account profiles. Serialize account mutations in a Rust lifecycle coordinator, preflight switches before disconnecting, and roll back both persisted configuration and the former connection on failure. Normalize funding balances in the Rust mapper. Expose risk configuration and a read-only ledger snapshot through dedicated commands, then compose three Vue account sections from small Pinia stores and shared form components.

**Tech Stack:** Rust stable, Tokio, Tauri 2, serde, keyring, rust_decimal, chrono-tz, Vue 3, TypeScript strict, Pinia, Naive UI, Vitest, Vue Test Utils.

## Global Constraints

- Modify only `G:\EasiFlux\EasiFlux-Desktop-Tauri`; do not edit the legacy Desktop or Python SDK repositories.
- Keep REST, WebSocket, HMAC, credential, account-switch, and risk rules in Rust. Vue coordinates commands and presentation only.
- Keep exactly one active account and one active connection. Do not introduce per-account API clients, concurrent account sessions, or per-account risk ledgers.
- Keep `risk_usage.toml` application-global. Switching, adding, editing, or deleting an account must never reset or rewrite its usage.
- Never serialize an API key, API secret, Keyring item name, or raw Keyring error into a frontend response.
- Preserve the current command names `save_credentials`, `has_credentials`, and `test_connection` for startup/settings compatibility.
- Funding balances are read-only. Do not add transfer, withdrawal, or transfer-history actions to the new page.
- Keep new modules/components focused and target fewer than 200 lines; extract a small child component or pure utility before allowing a new file to grow materially beyond that limit.
- Follow test-driven development for every task: add a focused failing test, run it and observe the intended failure, implement the smallest complete behavior, then rerun the focused and regression tests.
- Use `apply_patch` for source edits. Preserve unrelated worktree changes if any appear.
- Each implementation task ends with exactly one Conventional Commit containing its tests. Do not mix later tasks into an earlier commit.

## Commit Map

1. `feat(account): add account profile lifecycle`
2. `feat(account): add read-only asset overview`
3. `feat(risk): expose configurable daily risk status`
4. `feat(ui): integrate account and risk center navigation`

---

### Task 1: Add the account profile lifecycle

**Files:**

- Create: `src-tauri/src/services/account_profiles.rs`
- Create: `src-tauri/src/commands/account_profiles.rs`
- Modify: `src-tauri/src/models/account.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Modify: `src-tauri/src/commands/app.rs`
- Modify: `src-tauri/src/commands/connection.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types/models.ts`
- Create: `src/stores/accountProfiles.ts`
- Create: `src/services/accountSessionService.ts`
- Create: `src/utils/credentials.ts`
- Modify: `src/stores/account.ts`
- Modify: `src/stores/order.ts`
- Modify: `src/stores/position.ts`
- Modify: `src/stores/privatePanels.ts`
- Create: `src/components/account/CredentialEditor.vue`
- Create: `src/components/account/AccountProfilesPanel.vue`
- Create: `tests/frontend/accountProfiles.test.ts`
- Create: `tests/frontend/credentials.test.ts`

- [ ] **Step 1: Add failing Rust model and profile-list tests**

Add camelCase-safe models to `models/account.rs` and write tests in the new service module before defining them:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialState {
    Present,
    Missing,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub account_id: String,
    pub label: String,
    pub base_url: String,
    pub credential_state: CredentialState,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSwitchResult {
    pub active_account_id: String,
    pub connected: bool,
}
```

The tests must prove all of these cases:

```rust
#[test]
fn normalizes_deduplicates_and_prepends_missing_active_account() {
    let ids = normalize_account_ids(
        &[" backup ".into(), "".into(), "backup".into()],
        " primary ",
    );
    assert_eq!(ids, vec!["primary", "backup"]);
}

#[test]
fn profile_list_is_sanitized_and_isolates_keyring_failures() {
    let repository = FakeCredentialRepository::from([
        ("primary", FakeCredential::present("Main", "https://api.easicoin.io")),
        ("backup", FakeCredential::missing()),
        ("broken", FakeCredential::unavailable()),
    ]);
    let profiles = build_account_profiles(
        &["primary".into(), "backup".into(), "broken".into()],
        "primary",
        &repository,
    );

    assert_eq!(profiles[0].credential_state, CredentialState::Present);
    assert_eq!(profiles[1].credential_state, CredentialState::Missing);
    assert_eq!(profiles[2].credential_state, CredentialState::Unavailable);
    assert_eq!(serde_json::to_string(&profiles).unwrap().contains("apiSecret"), false);
    assert_eq!(serde_json::to_string(&profiles).unwrap().contains("apiKey"), false);
}
```

- [ ] **Step 2: Run the focused Rust tests and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri\src-tauri
cargo test services::account_profiles -- --nocapture
```

Expected: compilation fails because the account-profile models, repository seam, and normalization helpers do not exist.

- [ ] **Step 3: Implement sanitized profile listing**

Create a narrow repository seam so unit tests never touch the host Keyring:

```rust
pub(crate) trait CredentialRepository: Send + Sync {
    fn load(&self, account_id: &str) -> AppResult<Option<ApiCredential>>;
    fn save(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()>;
    fn delete(&self, account_id: &str) -> AppResult<()>;
}

pub(crate) struct KeyringCredentialRepository;
```

`KeyringCredentialRepository` delegates to `CredentialStore`. `normalize_account_ids` must trim every configured ID, drop empty configured entries, preserve first occurrence order, normalize an empty active ID to `default`, and prepend the normalized active account only when it is absent. User-entered account IDs still use the existing empty-to-`default` normalization. `build_account_profiles` must map each repository result as follows:

- valid `Some(credential)` -> `present`, with its normalized label/base URL;
- `None` -> `missing`, with label equal to account ID and the default base URL;
- invalid stored credential or any repository error -> `unavailable`, with no error details and no secret-derived fields.

Add `list_account_profiles` to `commands/account_profiles.rs`, export/register it, and keep list reads independent so one unavailable account cannot hide other profiles.

- [ ] **Step 4: Add failing lifecycle transaction tests**

Define a private generic async port in `services/account_profiles.rs` and drive it with a deterministic fake:

```rust
#[allow(async_fn_in_trait)]
pub(crate) trait AccountLifecyclePort: Send + Sync {
    async fn read_config(&self) -> AppConfig;
    async fn replace_runtime_config(&self, config: AppConfig);
    fn persist_config(&self, config: &AppConfig) -> AppResult<()>;
    fn load_credential(&self, account_id: &str) -> AppResult<Option<ApiCredential>>;
    fn save_credential(&self, account_id: &str, credential: &ApiCredential) -> AppResult<()>;
    fn delete_credential(&self, account_id: &str) -> AppResult<()>;
    async fn connection_status(&self) -> ConnectionStatus;
    async fn preflight(&self, credential: &ApiCredential) -> AppResult<()>;
    async fn disconnect(&self);
    async fn connect(&self, account_id: &str, realtime: bool, credential: ApiCredential)
        -> AppResult<()>;
}
```

Write tests for this observable sequence:

- switching to the active account is idempotent and performs no preflight, persistence, disconnect, or reconnect;
- a missing/unavailable target or failed preflight leaves both configuration and connection untouched;
- a disconnected source persists/selects the target but does not connect it;
- a connected source disconnects, persists/selects the target, and reconnects it;
- target connection failure restores the former persisted/in-memory config and attempts to reconnect the former credential;
- a rollback failure returns one combined error containing both the primary and rollback failure context;
- two simultaneous switch/delete calls never enter lifecycle effects concurrently;
- concurrent public connect/disconnect/config writes cannot interleave with a switch transaction;
- deleting the active account and deleting the only account are rejected;
- delete persistence failure restores the removed credential and leaves the runtime account list unchanged;
- saving a new account requires a complete key/secret pair, an existing account with both fields blank preserves the stored pair, and either partial pair is rejected;
- a successful account switch leaves an already-occupied global `RiskService` quota unchanged.

Use event names in the fake such as `preflight:backup`, `disconnect`, `persist:backup`, and `connect:backup`; assert the complete vector rather than only call counts.

- [ ] **Step 5: Run the lifecycle tests and confirm RED**

```powershell
cargo test services::account_profiles -- --nocapture
```

Expected: profile-list tests pass, while switch/delete/save transaction tests fail because the coordinator and command adapters are not implemented.

- [ ] **Step 6: Implement the serialized lifecycle coordinator and commands**

Add one coordinator to application state:

```rust
pub struct AccountLifecycleCoordinator {
    mutation: tokio::sync::Mutex<()>,
}

impl AccountLifecycleCoordinator {
    pub fn new() -> Self {
        Self { mutation: tokio::sync::Mutex::new(()) }
    }

    pub async fn mutation_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.mutation.lock().await
    }
}
```

Store it as `Arc<AccountLifecycleCoordinator>` in `AppState`. `save_credentials`, `switch_account`, and `delete_account` must all hold this same guard from their first read until the mutation has either committed or rolled back. The public `connect`, `disconnect`, `save_config`, and `save_window_size` commands must also use the guard around connection/config mutation so they cannot interleave with a switch or overwrite a freshly persisted active account. Release the guard before scheduling non-transactional refresh work.

Extract the current temporary-client check from `commands/connection.rs` into a reusable `preflight_credential` function. It must perform, in order:

1. server-time sync;
2. public `BTCUSDT` ticker request;
3. private balances request.

`test_connection` and account switching must both call that function.

Implement `switch_account(account_id, start_realtime)` in this exact order:

1. normalize target ID and return immediately if it is already active;
2. verify target membership and load a valid target credential;
3. preflight the target before any disconnect or config write;
4. record former config, former connection state, and former credential for rollback;
5. disconnect the former session;
6. persist a cloned config with the new active account, then replace the in-memory config;
7. reconnect only if the former state was `Connected`, using `start_realtime.unwrap_or(former_config.use_websocket)`;
8. on any post-disconnect failure, restore persisted config first, then runtime config, then make a best-effort former-account reconnect;
9. return `AccountSwitchResult` only after the primary transaction succeeds.

The coordinator must never mutate `RiskService` or `RiskUsageStore`.

Implement deletion as a recoverable transaction: load the former credential, delete it, persist the reduced cloned account list, then replace runtime config. If persistence fails, restore the former credential and return an error; do not expose a half-deleted profile.

Refactor `save_credentials` to delegate to the same generic service transaction and use clone -> persist -> runtime replacement. Preserve the former Keyring record for rollback, reject partial key/secret input, and delete a newly-created Keyring record if the account-list config write fails. A Keyring read error must block overwrite; it represents `unavailable`, not a missing record.

Register these commands in `lib.rs`:

```rust
list_account_profiles,
switch_account,
delete_account,
```

Extend the existing `scheduler_run_task` bridge with a special `bootstrap` task that calls `SchedulerService::bootstrap_connection()`. The frontend invokes this only after it has cleared the former account state, which prevents a new-account bootstrap event from being erased by a late frontend reset.

- [ ] **Step 7: Run Rust lifecycle tests and targeted formatting**

```powershell
cargo test services::account_profiles -- --nocapture
cargo test commands::config -- --nocapture
rustfmt --edition 2021 --check src/models/account.rs src/services/account_profiles.rs src/commands/account_profiles.rs src/commands/config.rs src/commands/connection.rs src/commands/app.rs src/state.rs src/lib.rs
```

Expected: all focused tests pass and targeted Rust files are formatted.

- [ ] **Step 8: Add failing frontend credential and store tests**

Add TypeScript contracts matching Rust exactly:

```ts
export type CredentialState = 'present' | 'missing' | 'unavailable'

export interface AccountProfile {
  accountId: string
  label: string
  baseUrl: string
  credentialState: CredentialState
  active: boolean
}

export interface AccountSwitchResult {
  activeAccountId: string
  connected: boolean
}
```

In `credentials.test.ts`, cover create/edit rules with a pure validator:

```ts
expect(validateCredentialDraft({ mode: 'create', apiKey: '', apiSecret: '' })).toBeTruthy()
expect(validateCredentialDraft({ mode: 'edit', apiKey: '', apiSecret: '' })).toBeNull()
expect(validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: '' })).toBeTruthy()
expect(validateCredentialDraft({ mode: 'edit', apiKey: 'new', apiSecret: 'secret' })).toBeNull()
```

In `accountProfiles.test.ts`, mock `tauriInvoke` and assert:

- refresh invokes `list_account_profiles` and never stores key/secret fields;
- save invokes `save_credentials`, exposes a save-specific loading/error state, and refreshes profiles only after success;
- successful switch clears account summary, balances, daily PnL, orders, positions, and private-panel snapshot only after `switch_account` resolves;
- failed switch leaves every former private store value intact;
- `switching` remains true while the invoke promise is pending;
- delete refreshes the list only after backend success and keeps the profile after backend failure.

- [ ] **Step 9: Run the frontend tests and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
pnpm test -- tests/frontend/credentials.test.ts tests/frontend/accountProfiles.test.ts
```

Expected: tests fail because the validator, profile store, and private-state reset APIs do not exist.

- [ ] **Step 10: Implement frontend lifecycle state and cleanup**

Add explicit reset functions:

- `account.clearAccountData()` resets summary, balances, account request, and daily-PnL request;
- `order.clearOrders()` resets open/history arrays and both requests;
- `position.clearPositions()` resets positions and its request;
- `privatePanels.clearPrivatePanels()` resets its shared async state.

`accountSessionService.clearAccountBoundState()` calls all four. It must not clear public market, watchlist, ticker, depth, or kline stores.

`useAccountProfilesStore` owns separate list/save/switch/delete async states plus one `mutating` computed flag. The save method delegates to the existing `save_credentials` command and refreshes the sanitized list only after success. Its switch flow is:

```ts
const result = await tauriInvoke<AccountSwitchResult>('switch_account', {
  accountId,
  startRealtime: null,
})
clearAccountBoundState()
connectionStore.setStatus(result.connected ? 'connected' : 'disconnected')
await Promise.allSettled([
  configStore.fetchConfig(),
  refreshProfiles(),
  connectionStore.refreshStatus(),
  result.connected
    ? tauriInvoke('scheduler_run_task', { task: 'bootstrap', force: true })
    : Promise.resolve(),
])
return result
```

Do not clear stores in a `finally` block. On backend failure, retain former data and expose the backend error through the switch request state. Post-commit refresh/bootstrap failures are independent section errors and must not relabel an already-committed account switch as failed.

- [ ] **Step 11: Implement and test the shared credential editor and profile panel**

`CredentialEditor.vue` must:

- accept `mode`, `accountId`, `initialLabel`, and `initialBaseUrl` props;
- keep key and secret inputs blank on every open;
- enable account ID input only in create mode;
- apply `validateCredentialDraft` before calling `accountProfilesStore.saveCredentials`, which invokes `save_credentials`;
- require key and secret together when either is entered;
- allow both blank in edit mode to preserve stored credentials;
- expose a test-connection action only when both current fields are present;
- emit only the normalized account ID after save, never the secret.

`AccountProfilesPanel.vue` must refresh on first activation, list sanitized profiles, show credential state, and open the editor for add/edit. Switch and delete buttons must be disabled while `mutating` is true. Delete uses a confirmation `AppDialog` whose body includes the exact account ID. Do not optimistically remove rows.

For an `unavailable` profile, disable edit, switch, and delete because the backend cannot safely distinguish or restore its Keyring record. A `missing` profile may be edited only by submitting a complete new pair.

Mount the panel with Pinia and stubs in `accountProfiles.test.ts`; assert that rendered text never contains the mock key or secret and that delete requires the second confirmation click.

- [ ] **Step 12: Verify and commit Task 1**

```powershell
pnpm test -- tests/frontend/credentials.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/account.test.ts tests/frontend/connection.test.ts
pnpm lint
pnpm build
cd src-tauri
cargo test services::account_profiles -- --nocapture
cargo test
cargo clippy
cd ..
git diff --check
git status --short
git add src-tauri/src/models/account.rs src-tauri/src/services/account_profiles.rs src-tauri/src/services/mod.rs src-tauri/src/commands/account_profiles.rs src-tauri/src/commands/mod.rs src-tauri/src/commands/config.rs src-tauri/src/commands/connection.rs src-tauri/src/commands/app.rs src-tauri/src/state.rs src-tauri/src/lib.rs src/types/models.ts src/stores/accountProfiles.ts src/services/accountSessionService.ts src/utils/credentials.ts src/stores/account.ts src/stores/order.ts src/stores/position.ts src/stores/privatePanels.ts src/components/account/CredentialEditor.vue src/components/account/AccountProfilesPanel.vue tests/frontend/accountProfiles.test.ts tests/frontend/credentials.test.ts
git commit -m "feat(account): add account profile lifecycle"
```

Expected: all checks pass; the commit contains only account-profile lifecycle code and its tests.

---

### Task 2: Add the read-only asset overview

**Files:**

- Modify: `src-tauri/src/models/account.rs`
- Modify: `src-tauri/src/api/mapper.rs`
- Modify: `src-tauri/src/api/diagnostic.rs`
- Modify: `src-tauri/src/commands/account.rs`
- Modify: `src/types/models.ts`
- Modify: `src/stores/account.ts`
- Create: `src/components/account/AccountAssetsPanel.vue`
- Create: `tests/frontend/accountAssets.test.ts`

- [ ] **Step 1: Add failing funding-balance mapper tests**

Add this model:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingBalance {
    pub asset: String,
    pub available: String,
    pub frozen: String,
    pub total: String,
}
```

Add mapper tests covering array/list/rows envelopes, every approved alias, numeric values, empty lists, and invalid rows. A representative assertion is:

```rust
#[test]
fn parses_funding_balance_aliases_and_skips_rows_without_asset() {
    let payload = serde_json::json!({
        "data": { "rows": [
            { "coin": "USDT", "availableBalance": "8", "frozenBalance": "2", "totalBalance": "10" },
            { "currency": "BTC", "available": 1.25, "locked": 0.25, "walletBalance": 1.5 },
            { "available": "99" }
        ]}
    });
    let balances = parse_funding_balances(&payload);
    assert_eq!(balances.len(), 2);
    assert_eq!(balances[0].asset, "USDT");
    assert_eq!(balances[1].total, "1.5");
}
```

- [ ] **Step 2: Run the mapper test and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri\src-tauri
cargo test parse_funding_balance -- --nocapture
```

Expected: compilation fails because `FundingBalance` and `parse_funding_balances` do not exist.

- [ ] **Step 3: Implement the normalized funding command**

Use `extract_list_with_meta` and `get_str`. The only accepted aliases are:

```rust
const FUNDING_ASSET_KEYS: &[&str] = &["coin", "currency", "asset"];
const FUNDING_AVAILABLE_KEYS: &[&str] = &["availableBalance", "available", "available_balance"];
const FUNDING_FROZEN_KEYS: &[&str] = &["frozenBalance", "frozen", "locked"];
const FUNDING_TOTAL_KEYS: &[&str] = &["totalBalance", "walletBalance", "balance", "total"];
```

Skip rows without a non-empty asset or without any recognized balance field. Default an individually missing available/frozen field to `0`; if total is absent, derive it only when available and frozen both parse as decimals, otherwise skip the row. Keep values as strings.

Update `warn_if_raw_parsed_mismatch` so it logs whenever `raw_count != parsed_count`, including partial filtering, without logging an empty successful envelope. Change `fetch_funding_balances` from `AppResult<Value>` to `AppResult<Vec<FundingBalance>>`, parse the private API payload, emit mismatch diagnostics, and return the normalized vector.

- [ ] **Step 4: Add failing frontend asset-state tests**

Add `FundingBalance` to TypeScript and extend `useAccountStore` with an independent funding request. Test that:

- `refreshFundingBalances` invokes `fetch_funding_balances` and stores normalized rows;
- a funding request failure preserves existing contract summary/balances and exposes only the funding error;
- `clearAccountData` also clears funding balances;
- a disconnected `AccountAssetsPanel` does not start private invokes;
- activating/refreshing the connected panel requests account, positions, daily PnL, and funding through `Promise.allSettled`, so one rejection does not suppress other sections.

- [ ] **Step 5: Run the frontend asset test and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
pnpm test -- tests/frontend/accountAssets.test.ts
```

Expected: tests fail because the funding model/store API and asset panel do not exist.

- [ ] **Step 6: Implement the asset store and panel**

The panel must render four independently stateful sections:

1. contract equity and per-asset available/frozen/total;
2. non-zero positions with aggregate unrealized PnL;
3. daily realized PnL from the existing daily-PnL snapshot;
4. funding-account balances.

Show connection status, each section's last-update time, and one manual refresh button. When disconnected, show `连接账户后刷新` and do not invoke private commands. On refresh, use `Promise.allSettled`; never clear a successful section because a different section failed. Do not add any mutation button.

- [ ] **Step 7: Verify and commit Task 2**

```powershell
pnpm test -- tests/frontend/accountAssets.test.ts tests/frontend/account.test.ts tests/frontend/dashboardAssets.test.ts
pnpm lint
pnpm build
cd src-tauri
cargo test parse_funding_balance -- --nocapture
cargo test
cargo clippy
rustfmt --edition 2021 --check src/models/account.rs src/api/mapper.rs src/api/diagnostic.rs src/commands/account.rs
cd ..
git diff --check
git status --short
git add src-tauri/src/models/account.rs src-tauri/src/api/mapper.rs src-tauri/src/api/diagnostic.rs src-tauri/src/commands/account.rs src/types/models.ts src/stores/account.ts src/components/account/AccountAssetsPanel.vue tests/frontend/accountAssets.test.ts
git commit -m "feat(account): add read-only asset overview"
```

Expected: all checks pass; no transfer-related UI or command changes are included.

---

### Task 3: Expose configurable daily-risk status

**Files:**

- Create: `src-tauri/src/models/risk.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Modify: `src-tauri/src/models/config.rs`
- Modify: `src-tauri/src/services/risk.rs`
- Modify: `src-tauri/src/storage/config.rs`
- Create: `src-tauri/src/commands/risk.rs`
- Modify: `src-tauri/src/commands/config.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/types/models.ts`
- Create: `src/utils/risk.ts`
- Create: `src/stores/risk.ts`
- Create: `src/components/account/RiskControlPanel.vue`
- Create: `tests/frontend/risk.test.ts`

- [ ] **Step 1: Add failing Rust risk status and validation tests**

Create these external models:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLedgerState {
    Disabled,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskStatus {
    pub enabled: bool,
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
    pub ledger_state: RiskLedgerState,
    pub trading_day: String,
    pub occupied_orders: Option<u32>,
    pub remaining_orders: Option<u32>,
    pub updated_at_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRiskConfigRequest {
    pub enabled: bool,
    pub max_order_qty: String,
    pub max_price_deviation_pct: String,
    pub max_daily_orders: u32,
    pub trading_day_timezone: String,
}
```

Extend `services/risk.rs` tests to prove:

- same-day/same-timezone snapshot reports current occupied and saturated remaining values;
- stale day or timezone reports zero usage without changing the ledger file;
- disabled status does not load or lock an unreadable ledger and returns null usage;
- an unreadable enabled ledger returns `unavailable`, null usage, and a generic scrubbed error without a path;
- enabling after disabled reloads the ledger, preserving fail-closed order behavior;
- validation rejects zero/negative/invalid quantity, negative/invalid deviation, zero daily limit, and invalid IANA timezone;
- validation accepts zero deviation and valid `Asia/Shanghai`, `UTC`, and `America/New_York` zones.

- [ ] **Step 2: Run focused risk tests and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri\src-tauri
cargo test services::risk -- --nocapture
```

Expected: new tests fail because risk status, strict validation, and disabled-ledger behavior are absent.

- [ ] **Step 3: Implement read-only snapshots and strict validation**

Add:

```rust
pub fn validate_risk_config(config: &RiskConfig) -> AppResult<()>;
pub fn status(&self, now_ms: u64) -> RiskStatus;
```

Validation uses `Decimal::from_str` and `is_valid_iana_timezone`; update commands must not call the fallback timezone resolver. `status` computes the current trading-day key but never saves the ledger. For stale usage, return `occupied_orders = Some(0)`, `remaining_orders = Some(max_daily_orders)`, and `updated_at_ms = None`. For current usage, compute remaining with `saturating_sub`.

Change service initialization/update behavior so a disabled configuration does not read the ledger. On a disabled -> enabled transition, attempt one reload and record any internal load error. Do not surface the internal error string; return a fixed user-safe message such as `风险用量账本不可用，请检查本地存储权限或文件格式`.

- [ ] **Step 4: Add failing command persistence tests**

Add `ConfigStore::with_path(path)` for tests. In `commands/risk.rs`, factor the state mutation into a testable helper and prove:

- a valid request writes a complete cloned `AppConfig` before changing runtime config/service state;
- a blocked/unwritable config path returns an error and leaves both runtime `AppConfig` and `RiskService` unchanged;
- `save_config` calls the same `validate_risk_config` helper and cannot bypass invalid risk values;
- updating risk timezone triggers the existing daily-PnL refresh after the successful commit;
- no risk update writes or deletes `risk_usage.toml`.

- [ ] **Step 5: Implement and register risk commands**

Add:

```rust
#[tauri::command]
pub async fn get_risk_status(state: State<'_, AppState>) -> AppResult<RiskStatus>;

#[tauri::command]
pub async fn update_risk_config(
    state: State<'_, AppState>,
    request: UpdateRiskConfigRequest,
) -> AppResult<RiskStatus>;
```

`update_risk_config` must acquire the shared lifecycle/config mutation guard, trim decimal/timezone fields, validate a `RiskConfig`, clone the full current `AppConfig`, replace only the five risk fields, persist it, then replace in-memory config and update `RiskService`. Return a fresh status using `state.time.now_ms()`. If persistence fails, return before either runtime write. Release the guard before triggering the daily-PnL refresh.

Register `get_risk_status` and `update_risk_config` in `lib.rs`. Refactor `save_config` to run the same strict validator before persistence.

- [ ] **Step 6: Run Rust risk/command tests**

```powershell
cargo test services::risk -- --nocapture
cargo test commands::risk -- --nocapture
cargo test commands::config -- --nocapture
```

Expected: all focused tests pass, including persistence-failure atomicity and application-global usage continuity.

- [ ] **Step 7: Add failing frontend risk tests**

Mirror the Rust types exactly:

```ts
export type RiskLedgerState = 'disabled' | 'ready' | 'unavailable'

export interface RiskStatus {
  enabled: boolean
  maxOrderQty: string
  maxPriceDeviationPct: string
  maxDailyOrders: number
  tradingDayTimezone: string
  ledgerState: RiskLedgerState
  tradingDay: string
  occupiedOrders: number | null
  remainingOrders: number | null
  updatedAtMs: number | null
  error: string | null
}

export interface UpdateRiskConfigRequest {
  enabled: boolean
  maxOrderQty: string
  maxPriceDeviationPct: string
  maxDailyOrders: number
  tradingDayTimezone: string
}
```

Test the pure frontend validator, store, and panel:

- invalid decimal, non-positive quantity, negative deviation, non-positive limit, and blank timezone are rejected before invoke;
- refresh invokes `get_risk_status`;
- save invokes `update_risk_config` with trimmed strings and adopts the returned snapshot;
- save failure preserves draft input and the last successful snapshot;
- `unavailable` renders a warning and `--` usage, while `disabled` renders disabled status rather than zero usage;
- successful save and manual refresh both replace the displayed snapshot;
- mounting an inactive risk panel performs no request.

- [ ] **Step 8: Run frontend risk tests and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
pnpm test -- tests/frontend/risk.test.ts
```

Expected: tests fail because the risk validator, store, and panel do not exist.

- [ ] **Step 9: Implement the risk store and panel**

`useRiskStore` keeps `status`, read request state, and update request state separate. `RiskControlPanel` owns the editable draft so a failed save cannot overwrite user input. Refresh only when the section becomes active, after a successful save, or on manual refresh; do not add polling or a Tauri event.

Use an explicit timezone select containing at least `Asia/Shanghai`, `UTC`, `America/New_York`, and `Europe/London`, while the backend remains authoritative for validation. Show the configured trading day, occupied/remaining quota, updated time, and ledger health.

- [ ] **Step 10: Verify and commit Task 3**

```powershell
pnpm test -- tests/frontend/risk.test.ts
pnpm lint
pnpm build
cd src-tauri
cargo test services::risk -- --nocapture
cargo test commands::risk -- --nocapture
cargo test
cargo clippy
rustfmt --edition 2021 --check src/models/risk.rs src/models/config.rs src/services/risk.rs src/storage/config.rs src/commands/risk.rs src/commands/config.rs src/lib.rs
cd ..
git diff --check
git status --short
git add src-tauri/src/models/risk.rs src-tauri/src/models/mod.rs src-tauri/src/models/config.rs src-tauri/src/services/risk.rs src-tauri/src/storage/config.rs src-tauri/src/commands/risk.rs src-tauri/src/commands/config.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/types/models.ts src/utils/risk.ts src/stores/risk.ts src/components/account/RiskControlPanel.vue tests/frontend/risk.test.ts
git commit -m "feat(risk): expose configurable daily risk status"
```

Expected: all checks pass; the existing reservation/release tests still prove strict daily quota persistence across restarts.

---

### Task 4: Integrate account-center navigation and shared settings UI

**Files:**

- Create: `src/types/navigation.ts`
- Create: `src/components/account/AccountCenterPage.vue`
- Modify: `src/components/layout/NavigationRail.vue`
- Modify: `src/components/layout/AppShell.vue`
- Modify: `src/components/layout/Sidebar.vue`
- Modify: `src/components/dashboard/DashboardPage.vue`
- Modify: `src/components/dashboard/types.ts`
- Modify: `src/components/settings/SettingsDialog.vue`
- Modify: `src/components/trading/OrderPanel.vue`
- Create: `tests/frontend/accountNavigation.test.ts`
- Create: `tests/frontend/settingsCredentialEditor.test.ts`

- [ ] **Step 1: Add failing navigation and integration tests**

Move `NavKey` out of `NavigationRail.vue` and define structured targets in `src/types/navigation.ts`:

```ts
export type AccountSection = 'api' | 'assets' | 'risk'

export type NavKey =
  | 'home'
  | 'trading'
  | 'charts'
  | 'news'
  | 'account'
  | 'plugins'
  | 'settings'

export interface NavigationTarget {
  page: NavKey
  section?: AccountSection
}
```

Test these user-visible behaviors:

- selecting the account rail for the first time renders the API section;
- selecting sidebar `assets` or `risk` emits the section to `AppShell` and changes the mounted panel;
- navigating away and back to account preserves the last account section for the current process;
- dashboard `查看资产` navigates directly to `{ page: 'account', section: 'assets' }`;
- settings still opens from the gear action and mounts the same `CredentialEditor` used by the account API section;
- `OrderPanel` submit is disabled while `accountProfilesStore.switching` is true, even if connection/order validation would otherwise allow submission.

- [ ] **Step 2: Run integration tests and confirm RED**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
pnpm test -- tests/frontend/accountNavigation.test.ts tests/frontend/settingsCredentialEditor.test.ts
```

Expected: tests fail because navigation is page-only, sidebar selection is local, and settings duplicates credential fields.

- [ ] **Step 3: Implement structured account navigation**

`AppShell` owns:

```ts
const activePage = ref<NavKey>('home')
const activeAccountSection = ref<AccountSection>('api')

function navigateTo(target: NavKey | NavigationTarget): void {
  const normalized = typeof target === 'string' ? { page: target } : target
  activePage.value = normalized.page
  if (normalized.page === 'account' && normalized.section) {
    activeAccountSection.value = normalized.section
  }
}
```

Update `NavigationRail`, `AppShell`, `Sidebar`, and dashboard components to import `NavKey` from `types/navigation.ts`. Make `Sidebar` controlled with `activeSection` and a `selectSection` event; remove its local `activeSecondary`. For non-account secondary navigation, it may still receive a string controlled by `AppShell`, but account keys must be typed as `AccountSection`.

`AccountCenterPage` is a lightweight switch that keeps panels mounted only when useful and passes `active` to asset/risk panels so hidden sections do not request data. The account placeholder in `AppShell` is fully replaced; placeholders for charts/news/plugins remain unchanged.

- [ ] **Step 4: Route dashboard assets and reuse the credential editor in settings**

Change the dashboard navigation emit to `NavKey | NavigationTarget`; map `assets` directly to:

```ts
emit('navigate', { page: 'account', section: 'assets' })
```

Refactor `SettingsDialog` to mount `CredentialEditor` for the normalized active account. Load its sanitized label/base URL from `useAccountProfilesStore`; never prefill key or secret. Keep general WebSocket and ticker-poll settings in the dialog, but remove risk configuration fields now owned by `RiskControlPanel`. After the editor reports a successful credential save, persist general settings and connect through the stored Keyring credential.

- [ ] **Step 5: Block trading submission during account switching**

Read the profile store in `OrderPanel` and extend the final guard:

```ts
const canSubmit = computed(
  () => connectionStore.connected
    && !accountProfilesStore.switching
    && !submitting.value
    && !validationMessage.value,
)
```

Also check `switching` at the start of `submit()` and show a warning, so programmatic calls cannot bypass the disabled button.

- [ ] **Step 6: Run frontend integration and regression tests**

```powershell
pnpm test -- tests/frontend/accountNavigation.test.ts tests/frontend/settingsCredentialEditor.test.ts tests/frontend/accountProfiles.test.ts tests/frontend/accountAssets.test.ts tests/frontend/risk.test.ts tests/frontend/orderForm.test.ts
pnpm lint
pnpm build
```

Expected: navigation, shared editor, switch guard, strict TypeScript, and production build all pass.

- [ ] **Step 7: Run the full repository verification**

```powershell
cd G:\EasiFlux\EasiFlux-Desktop-Tauri
pnpm test
pnpm lint
pnpm build
cd src-tauri
cargo test
cargo clippy
cd ..
git diff --check
git status --short
```

Expected: the complete frontend and Rust suites pass, lint/build pass, and only Task 4 files remain unstaged.

- [ ] **Step 8: Perform manual Tauri smoke checks**

Run:

```powershell
pnpm tauri dev
```

Verify in the desktop window:

1. account/API lists sanitized profiles and supports add/edit;
2. failed target preflight leaves the current connected account and visible private data unchanged;
3. successful switch clears former private data, reconnects only when appropriate, and repopulates snapshots;
4. delete dialog names the exact inactive account and cannot delete active/only accounts;
5. assets show contract/funding failures independently and contain no transfer action;
6. risk usage survives account switching and process restart, and timezone/day rendering matches config;
7. settings and account API page use identical blank-key/secret edit rules;
8. dashboard `查看资产` opens the assets section directly.

Stop the development process after the checks. If WebView2 or environment setup blocks this smoke test, record the exact blocker and do not claim the manual check passed.

- [ ] **Step 9: Commit Task 4**

```powershell
git add src/types/navigation.ts src/components/account/AccountCenterPage.vue src/components/layout/NavigationRail.vue src/components/layout/AppShell.vue src/components/layout/Sidebar.vue src/components/dashboard/DashboardPage.vue src/components/dashboard/types.ts src/components/settings/SettingsDialog.vue src/components/trading/OrderPanel.vue tests/frontend/accountNavigation.test.ts tests/frontend/settingsCredentialEditor.test.ts
git commit -m "feat(ui): integrate account and risk center navigation"
git status --short --branch
git log -4 --oneline
```

Expected: the branch is clean and the latest four implementation commits match the commit map in order.

---

## Final Acceptance Checklist

- [ ] No frontend command or event returns API key/secret material.
- [ ] Account mutation operations share one Rust serialization guard.
- [ ] Switch preflight occurs before disconnect; failed connection attempts restore config and former connection where possible.
- [ ] Only successful switches clear frontend private state.
- [ ] Account switching does not modify the global risk usage ledger.
- [ ] Funding balances use the approved aliases and isolate section failures.
- [ ] Risk status is read-only, disabled mode avoids ledger reads, and invalid config cannot enter through `save_config`.
- [ ] Hidden account sections do not poll or request private data.
- [ ] Settings and account API management share `CredentialEditor`.
- [ ] Each completed feature has exactly one implementation commit with its own tests.
- [ ] `pnpm test`, `pnpm lint`, `pnpm build`, `cargo test`, and `cargo clippy` pass before completion is reported.
