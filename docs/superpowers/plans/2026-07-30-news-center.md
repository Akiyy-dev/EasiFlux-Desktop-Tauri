# News Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build PRD-09 as an app-wide, Rust-owned news synchronizer backed by the fixed TG-forwarder public API, a durable SQLite cache, and a Jin10-inspired Vue timeline that intentionally omits source identity and media type.

**Architecture:** Rust owns build-time source/default-token configuration, optional Keyring token override, defensive HTTP polling, source-bound SQLite state, lifecycle, retry policy, commands, and events. Vue/Pinia only reads local snapshots, coordinates unread/scroll state, opens safe links through Tauri, and renders a focused timeline while polling continues outside the page lifecycle. The initial historical import is transactionally hidden until complete and becomes the seen baseline.

**Tech Stack:** Tauri 2, Rust stable, Tokio, reqwest, rusqlite (bundled), keyring 3 native persistent backends, SHA-256, Vue 3, TypeScript strict, Pinia, Naive UI, Vitest, Vue Test Utils.

## Global Constraints

- Work only in the `news/createnewscenter` linked worktree and preserve unrelated changes.
- `origin/main`, local `main`, and the branch base were verified at `953f633bd058ff87b71cc593843567b44c0598f0` before implementation.
- The only upstream source is the built-in TG-forwarder endpoint `GET /api/public/v1/messages`; no source selector or endpoint editor appears in the UI.
- Release builds require `EASIFLUX_NEWS_API_BASE_URL`, `EASIFLUX_NEWS_SOURCE_EPOCH`, and `EASIFLUX_NEWS_API_TOKEN`; the URL must be HTTPS without credentials, query, or fragment, the epoch must match `[A-Za-z0-9._-]{1,64}`, and the token must form a valid raw Bearer token. Debug/test may omit all three and may additionally use HTTP only for loopback hosts.
- The source fingerprint is SHA-256 of normalized base URL, one NUL byte, and source epoch. A mismatch pauses news sync and never merges or resets the old cursor automatically.
- The default API token is supplied as a GitHub Repository Secret and embedded at compile time. GitHub protects the build input, not the distributed binary; use only a least-privilege credential approved for client distribution.
- A valid Keyring value at service `easiflux_desktop_tauri_news`, entry `news_api_token`, overrides the embedded default. Missing/unavailable/malformed Keyring data falls back to the embedded value. `set` and guarded `delete` remain optional future override operations and are never called automatically.
- HTTP uses a 5-second connect timeout, 15-second request timeout, disabled redirects, an 8 MiB response-body cap, and a 256 KiB per-message text cap.
- API validation requires positive signed-64-bit delivery IDs, IDs greater than the request cursor, unique IDs in a page, at most the requested limit, RFC3339 timestamps with an explicit zone, and exact cursor semantics.
- SQLite is the local source of truth for UI reads. Each page inserts messages, advances the cursor, updates initial-sync state, and updates seen baseline in one transaction.
- The first historical import is hidden until caught up; retained partial data resumes after restart; completed historical import is marked seen and does not create a large unread count.
- History is retained indefinitely. List page size is 50; older history is explicitly loaded in pages of 50; unread is counted from rows and capped in Rust to `100`, whose presentation is `99+`.
- Polling is app-wide: caught-up cadence is about 3 seconds; `has_more` continues immediately; transient backoff is 3/6/12/24/48/60 seconds and honors a larger `Retry-After` up to 60 seconds.
- Invalid active credentials, deployment/source/default-token configuration, contract violations, and storage errors pause only news and keep completed cached history readable.
- Shutdown requests the poller to stop and waits no longer than 20 seconds.
- UI content is local time with seconds plus text only. It must not show source username, source/chat/message IDs, or media type. Blank text renders `该消息暂无可展示的文本内容`.
- Only `http:` and `https:` links may be opened with the system browser. Never use `v-html`.
- The news page retains TopBar and NavigationRail, hides Sidebar, uses one centered column up to 880px, a fixed time rail, a 24px latest threshold, and reduced-motion-safe fades no longer than 200ms.
- No PRD-13 system notifications are added.
- Production files should remain focused and target fewer than 200 lines; split modules by responsibility when that target is exceeded.
- Every behavioral task follows strict RED -> observed expected failure -> minimal implementation -> GREEN.
- Use `apply_patch` for source edits.
- The user has not authorized `git add`, `git commit`, `git push`, merge, or release publication. This plan contains no commit step; all changes remain uncommitted for the final unified review.

## File Responsibility Map

### Build, deployment, and credentials

- `src-tauri/build_support/news_build_config.rs`: pure validation and normalization for release source variables.
- `src-tauri/build.rs`: validates release source configuration and exports normalized compile-time values.
- `src-tauri/src/storage/news_token.rs`: redacted token value, token-store trait, and native Keyring adapter.
- `src-tauri/src/news_provision.rs`: independently testable provisioning command parser and runner.
- `src-tauri/src/bin/easiflux-news-provision.rs`: minimal same-user utility entrypoint.
- `.github/workflows/tauri-build-reusable.yml`: compiles and uploads the per-platform provisioning utility alongside app artifacts.
- `.github/workflows/release.yml` and `.github/workflows/release-please.yml`: pass named repository secrets for host/token and the protected source-epoch variable to the reusable build.
- `docs/news-deployment.md`: operator contract for embedded credentials, their extraction boundary, and optional same-user Keyring override set/status/delete.

### Rust news domain

- `src-tauri/src/models/news.rs`: external DTOs, status enum/snapshot, page/read/event contracts, and validated upstream item/page models.
- `src-tauri/src/api/news_client.rs`: bounded defensive TG-forwarder client and classified fetch errors.
- `src-tauri/src/storage/news_database.rs`: schema migration, source binding, transactional page commit, list/count/mark-seen queries.
- `src-tauri/src/services/news/backoff.rs`: deterministic transient retry policy and Retry-After handling.
- `src-tauri/src/services/news/poller.rs`: cancellation-aware fetch/commit state machine.
- `src-tauri/src/services/news.rs`: public service facade, status watch, app-wide start/stop/recheck/retry, and command reads.
- `src-tauri/src/commands/news.rs`: five thin Tauri command adapters.
- `src-tauri/src/events/emitter.rs`: typed news committed/status events with observable emit failures.
- `src-tauri/src/state.rs` and `src-tauri/src/lib.rs`: degraded-safe assembly, command registration, startup, and bounded shutdown.

### Frontend

- `src/types/news.ts`: camelCase command/event/status contracts.
- `src/services/newsService.ts`: five typed Tauri command wrappers.
- `src/utils/newsLinks.ts`: safe text segmentation and scheme-checked system opening.
- `src/stores/news.ts`: app-wide status/unread state, page lifecycle, generation-safe refresh, pagination, and seen advancement.
- `src/composables/useNewsRuntimeHost.ts`: register events before the initial status snapshot.
- `src/components/news/NewsStatusBar.vue`: all sync/degraded states and exact recovery controls.
- `src/components/news/NewMessagesBanner.vue`: pending-new affordance with polite live announcement.
- `src/components/news/NewsTimeline.vue`: local-time grouped timeline, safe links, placeholder text, and load-more control.
- `src/components/news/NewsCenterPage.vue`: scroll threshold, viewport preservation, enter/leave coordination, and focused page layout.
- `src/components/layout/AppShell.vue` and `src/components/layout/NavigationRail.vue`: page routing, Sidebar removal, persistent runtime host, and unread badge.

---

### Task 1: Lock build-time source configuration

**Files:**
- Create: `src-tauri/build_support/news_build_config.rs`
- Create: `src-tauri/tests/news_build_config.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `validate_news_build_config(profile, base_url, source_epoch, api_token) -> Result<Option<NewsBuildConfig>, NewsBuildConfigError>` and compile-time `EASIFLUX_NEWS_API_BASE_URL` / `EASIFLUX_NEWS_SOURCE_EPOCH` / `EASIFLUX_NEWS_API_TOKEN` values.
- Consumes later: `NewsService::from_app` reads values using `option_env!` and computes the source fingerprint.

- [ ] **Step 1: Write failing pure validation tests**

Add table-driven tests proving debug permits both values to be absent, debug accepts HTTP only for the three loopback host spellings, release rejects absent/HTTP/credential/query/fragment URLs and invalid epochs, and release normalizes a trailing slash without echoing the rejected input in `Display`.

```rust
#[test]
fn release_accepts_only_a_safe_fixed_source() {
    let config = validate_news_build_config(
        "release",
        Some("https://news.example.invalid/root/"),
        Some("prod-v1"),
    ).unwrap().unwrap();
    assert_eq!(config.base_url.as_str(), "https://news.example.invalid/root/");
    assert_eq!(config.source_epoch, "prod-v1");
}
```

- [ ] **Step 2: Run the focused integration test and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test news_build_config`

Expected: FAIL because `build_support/news_build_config.rs` and its exported types do not exist.

- [ ] **Step 3: Implement the pure validator and wire the build script**

Add `url = "2"` as a build dependency. In `build.rs`, print `cargo:rerun-if-env-changed` for the base URL, source epoch, and API token names; invoke the pure validator using `PROFILE`; and let `option_env!` read the compilation-step environment directly. Never emit host or token values through Cargo instructions. Panic with stable category-only text for invalid/partial configuration, while permitting an entirely unconfigured debug/test build.

- [ ] **Step 4: Run focused tests GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test news_build_config`

Expected: all build-config cases PASS.

### Task 2: Add persistent redacted news credentials and provisioning utility

**Files:**
- Create: `src-tauri/src/storage/news_token.rs`
- Create: `src-tauri/src/news_provision.rs`
- Create: `src-tauri/src/bin/easiflux-news-provision.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Create: `docs/news-deployment.md`

**Interfaces:**
- Produces: `NewsApiToken`, `NewsTokenStore`, `KeyringNewsTokenStore`, `ProvisionAction`, and `run_provision(args, input, output, store) -> ExitCode`.
- Fixed identifiers: `NEWS_KEYRING_SERVICE` and `NEWS_KEYRING_ENTRY`.
- Consumes later: the poller receives `Arc<dyn NewsTokenStore>`; no frontend interface can write this token.

- [ ] **Step 1: Write failing token-store and utility tests**

Cover redacted `Debug`, `NoEntry -> None`, set/load/delete against one injected mock `keyring::Entry`, production builder persistence `UntilDelete`, bounded input, trailing CR/LF trimming, empty/control-character/header-invalid rejection, exact delete selectors, status exit codes, and output that never contains the fake token or its prefix.

```rust
#[test]
fn token_debug_is_always_redacted() {
    let token = NewsApiToken::parse("tgf_secret_value\n").unwrap();
    assert_eq!(format!("{token:?}"), "NewsApiToken(<redacted>)");
}
```

- [ ] **Step 2: Run credential-focused tests and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::news_token::tests`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --bin easiflux-news-provision`

Expected: FAIL because the news token store and utility do not exist.

- [ ] **Step 3: Implement native persistence and the narrow CLI**

Enable keyring features `apple-native`, `windows-native`, `linux-native-sync-persistent`, and `crypto-rust`; add `zeroize` and `rpassword`. Explicitly declare desktop and provisioning binaries and set Tauri `mainBinaryName` to `easiflux-desktop`. The binary accepts only `set`, `status`, and guarded `delete --service easiflux_desktop_tauri_news --entry news_api_token`; terminal set uses no-echo input and piped set uses a bounded reader.

- [ ] **Step 4: Document same-user deployment and verify GREEN**

Document that the utility must run as the same non-elevated desktop user, Token comes only from stdin/no-echo input, and real Keyring mutation is excluded from CI. Run the two focused commands from Step 2 and require PASS.

### Task 3: Define stable news contracts and a defensive TG-forwarder client

**Files:**
- Create: `src-tauri/src/models/news.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Create: `src-tauri/src/api/news_client.rs`
- Modify: `src-tauri/src/api/mod.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `NewsMessageDto`, `NewsPage`, `NewsUnreadSnapshot`, `NewsStatusKind`, `NewsStatusSnapshot`, `NewsMessagesCommittedEvent`, `ValidatedNewsPage`, `NewsPageFetcher`, `TgForwarderNewsClient`, and `NewsFetchError`.
- `NewsPageFetcher::fetch_after(&self, token: &NewsApiToken, cursor: i64, limit: usize) -> Result<ValidatedNewsPage, NewsFetchError>`.
- JSON output uses camelCase and decimal-string delivery IDs.

- [ ] **Step 1: Write failing serialization and client contract tests**

Use an isolated local mock server. Cover exact GET path/query/Bearer, required `{ ok: true, data }` wrapper with ignored `meta`/unknown fields, multi-item success, empty-page cursor rule, unsorted-valid page normalization, duplicate/nonpositive/not-after-cursor IDs, fractional/string/overflow IDs, page over limit, malformed/non-zoned timestamps and millisecond overflow, null/missing/non-string text, whitespace normalization, unknown-field tolerance, text over 256 KiB, body over 8 MiB, `ok=false`, 2xx non-JSON, disabled redirect, 401, 422, 408/429/5xx, Retry-After seconds/date, connect/request timeout, and error formatting that omits the fake token and body.

```rust
#[tokio::test]
async fn rejects_a_cursor_that_is_not_the_page_maximum() {
    let response = r#"{"ok":true,"data":{"items":[{"delivery_id":11,"created_at":"2026-07-30T08:00:00Z","text":"x"}],"next_cursor":12,"has_more":true},"meta":{}}"#;
    let error = fetch_fixture(10, response).await.unwrap_err();
    assert_eq!(error.kind(), NewsFetchErrorKind::Contract);
}
```

- [ ] **Step 2: Run client tests and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml api::news_client::tests`

Expected: FAIL because the contracts and client are absent.

- [ ] **Step 3: Implement bounded streaming and exact validation**

Add reqwest `stream`, use a client with 5-second connect and 15-second total timeout and `Policy::none()`, stream chunks into a bounded buffer, deserialize the required wrapper into raw wire types, accept JSON integer IDs only, parse them into positive `i64`, parse `DateTime::parse_from_rfc3339` and checked UTC milliseconds, normalize whitespace-only text to `""`, sort by delivery ID, and enforce that nonempty `next_cursor` equals the maximum ID while an empty page has the request cursor and `has_more=false`.

- [ ] **Step 4: Run model/client tests GREEN**

Run the Task 3 focused test command and require PASS with no real network or Keyring access.

### Task 4: Build the source-bound transactional SQLite cache

**Files:**
- Create: `src-tauri/src/storage/news_database.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `NewsDatabase::open`, `status_snapshot`, `commit_page`, `list_messages(before_id, limit)`, `mark_seen`, `prepare_source`, and `NewsStorageError`.
- `commit_page` returns `NewsCommitOutcome { inserted_count, newest_delivery_id, unread_count, initial_sync_complete }`.
- `prepare_source` distinguishes fresh, matching, and mismatched fingerprints without deleting data or resetting cursor.

- [ ] **Step 1: Write failing temporary-database tests**

Cover schema/user-version creation, WAL/busy-timeout/foreign-key/synchronous pragmas, source fingerprint binding, mismatch refusal, idempotent inserts, atomic rollback on invalid page, restart resume of partial initial sync, first-sync final transaction setting `last_seen_delivery_id = COALESCE(MAX(delivery_id),0)`, row-count unread semantics for non-contiguous IDs capped to 100, descending latest 50, exclusive older cursor with 49/50/51 and limit+1 `hasMore`, stable decimal-string DTOs, UTC timestamp roundtrip, local receipt timestamp storage, mark-seen monotonicity/clamping to stored latest, and completed cache visibility.

```rust
#[test]
fn first_sync_completion_becomes_the_seen_baseline_atomically() {
    let db = temporary_database();
    db.prepare_source(FINGERPRINT).unwrap();
    db.commit_page(page(&[2, 9], false)).unwrap();
    let snapshot = db.status_snapshot().unwrap();
    assert!(snapshot.initial_sync_complete);
    assert_eq!(snapshot.unread_count, 0);
    assert_eq!(snapshot.latest_delivery_id.as_deref(), Some("9"));
}
```

- [ ] **Step 2: Run storage tests and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml storage::news_database::tests`

Expected: FAIL because the database module does not exist.

- [ ] **Step 3: Implement schema v1 and transactions**

Add bundled rusqlite. Create the exact schema from the design: `news_messages(delivery_id, created_at_ms, text, received_at_ms)`, the `(created_at_ms DESC, delivery_id DESC)` index, and singleton `news_sync_state(singleton_id, cursor, last_seen_delivery_id, initial_sync_complete, source_fingerprint)` with all CHECK constraints and `PRAGMA user_version=1`. Keep the connection behind a short synchronous mutex, perform each page mutation in one immediate transaction, and expose operations so the asynchronous service executes every blocking database call through `spawn_blocking`.

- [ ] **Step 4: Run storage tests GREEN**

Run the Task 4 focused command and require all database cases PASS.

### Task 5: Implement deterministic retry and the cancellation-aware poller

**Files:**
- Create: `src-tauri/src/services/news/backoff.rs`
- Create: `src-tauri/src/services/news/poller.rs`
- Create: `src-tauri/src/services/news.rs`
- Modify: `src-tauri/src/services/mod.rs`

**Interfaces:**
- Produces: `NewsService::new`, `start`, `stop_and_join(Duration)`, `status`, `list_messages`, `mark_seen`, `recheck_credentials`, and `retry_sync`.
- Injects `Arc<dyn NewsPageFetcher>`, `Arc<dyn NewsTokenStore>`, `Arc<NewsDatabase>`, and `Arc<dyn NewsEventSink>` for tests.
- One owned background task and cancellation channel; `retry_sync`/`recheck_credentials` signal wake-up rather than spawning duplicates.

- [ ] **Step 1: Write failing paused/live/retry state-machine tests with paused Tokio time**

Cover unconfigured debug source -> `deploymentMisconfigured`; token-store absence/failure states at the generic service boundary; initial sync hidden and immediate `limit=100` pagination; caught-up 3-second sleep; 3/6/12/24/48/60 backoff reset after success; Retry-After max/cap; 401 -> `credentialInvalid`; 422, permanent 4xx, and contract error -> `contractError`; 408/429/5xx/network/timeout -> `retrying`; DB error -> `storageError`; manual recheck/retry wake-up; event emission after commit only; emit failure does not roll back; one poller under repeated start; operation without trading credentials/connection/page mount; blocking database work routed through `spawn_blocking`; and stop completes within a test timeout. Task 12 adds the production runtime's embedded-token fallback contract.

```rust
#[tokio::test(start_paused = true)]
async fn transient_failures_back_off_and_success_resets_the_sequence() {
    let rig = PollerRig::with_results([timeout(), timeout(), page(false), timeout()]);
    rig.start().await;
    rig.advance_and_assert_requests(Duration::from_secs(3), 2).await;
    rig.advance_and_assert_requests(Duration::from_secs(6), 3).await;
    rig.advance_and_assert_requests(Duration::from_secs(3), 4).await;
}
```

- [ ] **Step 2: Run service tests and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml services::news`

Expected: FAIL because retry/poller/service modules are absent.

- [ ] **Step 3: Implement status transitions, wakeups, and bounded cancellation**

Use `tokio::select!` over cancellation, explicit wake, and sleep. Classify errors without secret/body details. Publish status snapshots after every material transition, continue immediately while `has_more`, and expose completed cache even in paused states. `stop_and_join` signals cancellation and wraps the join in the supplied timeout; status becomes `stopped`.

- [ ] **Step 4: Run service tests GREEN**

Run the Task 5 command and require PASS under paused time without wall-clock sleeps.

### Task 6: Assemble Tauri state, commands, events, startup, and shutdown

**Files:**
- Create: `src-tauri/src/commands/news.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/events/emitter.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Commands: `get_news_status()`, `list_news_messages(beforeDeliveryId?: string, limit?: number)`, `mark_news_seen(throughDeliveryId: string)`, `recheck_news_credentials()`, `retry_news_sync()`.
- Events: `news://messages-committed` and `news://status-changed`.
- State always contains a `NewsService`; path/config/storage failures create a paused degraded service rather than failing Tauri setup.

- [ ] **Step 1: Write failing command/lifecycle/event tests**

Cover decimal ID parsing and limit `1..50`, camelCase serialization, exact command delegation, news-only error mapping, typed event names/payloads, setup surviving an invalid/unavailable news data path through an injected constructor, listener-safe startup, and a helper that requests news shutdown with a 20-second bound before final chart flush.

- [ ] **Step 2: Run focused integration tests and observe RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::news`

Run: `cargo test --manifest-path src-tauri/Cargo.toml events::emitter::tests`

Expected: FAIL because the commands/events/state wiring are absent.

- [ ] **Step 3: Implement thin command adapters and degraded-safe assembly**

Resolve `app.path().app_local_data_dir()/news/news.sqlite3`, compute SHA-256 over normalized URL + NUL + epoch, build Keyring/client/database adapters, manage state, register all five commands, start one poller after state management, and stop it on `RunEvent::Exit`. Never use Scheduler or connection state for news polling.

- [ ] **Step 4: Run focused and full Rust tests GREEN**

Run the Step 2 commands, then `cargo test --manifest-path src-tauri/Cargo.toml`; require all existing and news tests PASS.

### Task 7: Add typed frontend service, safe link parser, and app-wide store

**Files:**
- Create: `src/types/news.ts`
- Create: `src/services/newsService.ts`
- Create: `src/utils/newsLinks.ts`
- Create: `src/stores/news.ts`
- Create: `src/composables/useNewsRuntimeHost.ts`
- Create: `tests/frontend/newsLinks.test.ts`
- Create: `tests/frontend/newsStore.test.ts`

**Interfaces:**
- Produces TypeScript equivalents of every Task 3 DTO and five typed service calls.
- Store actions: `initialize`, `enterPage`, `leavePage`, `setAtLatest`, `handleMessagesCommitted`, `handleStatusChanged`, `refreshLatest`, `showLatest`, `loadMore`, `markLatestSeen`, `recheckCredentials`, `retrySync`.
- The runtime host registers both events before `initialize` and remains mounted with AppShell.

- [ ] **Step 1: Write failing service/link/store tests**

Cover exact invoke command/argument names, `http/https` segmentation with trailing punctuation excluded, dangerous schemes remaining text, a second URL validation before `openUrl`, event-before-snapshot reconciliation, page unopened not fetching messages, initial hidden cache, generation preventing stale refresh overwrite, single-flight committed refresh, BigInt ID sort/dedup, top refresh versus pending banner, load-more exclusive cursor and dedup, `99+` presentation input via raw count, and mark-seen adopting the returned snapshot.

```ts
it('does not replace a newer refresh with an older response', async () => {
  const first = deferred<NewsPage>()
  const second = deferred<NewsPage>()
  listNewsMessages.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
  const one = store.refreshLatest()
  const two = store.showLatest()
  second.resolve(page(['12']))
  first.resolve(page(['11']))
  await Promise.all([one, two])
  expect(store.messages.map(item => item.deliveryId)).toEqual(['12'])
})
```

- [ ] **Step 2: Run focused Vitest and observe RED**

Run: `.\node_modules\.bin\vitest.CMD run tests/frontend/newsLinks.test.ts tests/frontend/newsStore.test.ts`

Expected: FAIL because frontend news contracts, service, parser, store, and host do not exist.

- [ ] **Step 3: Implement strict typed contracts and concurrency-safe store**

Keep all IDs as decimal strings and compare with `BigInt`; never convert to `number`. Use a monotonically increasing latest-page generation and one queued refresh promise. Page entry loads only when initial sync is complete. While away from latest, committed events increase `pendingNewCount` and preserve messages; `showLatest` reloads, resets the banner, and lets the page mark the rendered newest row seen.

- [ ] **Step 4: Run focused frontend tests GREEN**

Run the Step 2 command and require PASS.

### Task 8: Build accessible status, banner, and timeline components

**Files:**
- Create: `src/components/news/NewsStatusBar.vue`
- Create: `src/components/news/NewMessagesBanner.vue`
- Create: `src/components/news/NewsTimeline.vue`
- Create: `tests/frontend/newsStatusBar.test.ts`
- Create: `tests/frontend/newsTimeline.test.ts`

**Interfaces:**
- `NewsStatusBar` consumes `status` and `hasCachedMessages`, emits `recheckCredentials` or `retrySync` only for recoverable states.
- `NewMessagesBanner` consumes `count`, emits `showLatest`, and uses `aria-live="polite"`.
- `NewsTimeline` consumes `items`, `hasMore`, `loadingMore`, `loadMoreError`, emits `loadMore`, and calls `openNewsLink` for link segments.

- [ ] **Step 1: Write failing component tests**

Assert all ten status kinds and their correct button/no-button behavior, cached-history visibility copy, `有 N 条新消息`, local date separators, `HH:mm:ss`, newest-first rendering, exact blank-text placeholder, safe link buttons, manual `加载更早消息`, end-of-history state, and absence of source username/ID/media labels.

- [ ] **Step 2: Run focused component tests and observe RED**

Run: `.\node_modules\.bin\vitest.CMD run tests/frontend/newsStatusBar.test.ts tests/frontend/newsTimeline.test.ts`

Expected: FAIL because components do not exist.

- [ ] **Step 3: Implement focused visual components**

Use existing CSS tokens, a mono time rail, restrained separators, semantic buttons, visible focus rings, link text without HTML injection, at most a 200ms background fade for newly inserted rows, and `prefers-reduced-motion: reduce` to disable that fade.

- [ ] **Step 4: Run component tests GREEN**

Run the Step 2 command and require PASS.

### Task 9: Integrate the page, scroll semantics, navigation badge, and persistent runtime

**Files:**
- Create: `src/components/news/NewsCenterPage.vue`
- Modify: `src/components/layout/AppShell.vue`
- Modify: `src/components/layout/NavigationRail.vue`
- Create: `tests/frontend/newsCenterPage.test.ts`
- Create: `tests/frontend/newsNavigation.test.ts`
- Modify: `tests/frontend/accountNavigation.test.ts`
- Modify: `tests/frontend/chartWorkspacePage.test.ts`

**Interfaces:**
- News page has no source selector and uses the singleton store.
- Navigation receives `newsUnreadCount: number`; it hides zero, shows `1..99`, and displays `99+` for 100 or more with a dynamic accessible label.
- AppShell mounts `useNewsRuntimeHost` regardless of active page and hides Sidebar for `charts` and `news` only.

- [ ] **Step 1: Write failing page/navigation regression tests**

Mock scroll metrics and assert the 24px latest threshold, committed-item viewport preservation when scrolled down, banner click reload/scroll-to-zero/seen advancement only after the newest row renders, no premature seen marking, page enter/leave, one load-more request while busy, 880px focused column, retained TopBar/NavigationRail, hidden Sidebar, persistent listener initialization before visiting news, badge 0/1/99/100, and unchanged trading/account/chart navigation.

- [ ] **Step 2: Run focused page/navigation tests and observe RED**

Run: `.\node_modules\.bin\vitest.CMD run tests/frontend/newsCenterPage.test.ts tests/frontend/newsNavigation.test.ts tests/frontend/accountNavigation.test.ts tests/frontend/chartWorkspacePage.test.ts`

Expected: FAIL because the page and navigation integration are absent and old Sidebar expectations cover charts only.

- [ ] **Step 3: Implement page orchestration and AppShell integration**

Measure the scroll container before/after a refresh and restore `scrollTop += newScrollHeight - oldScrollHeight` when away from latest. Use `nextTick` before marking the latest rendered ID as seen. Replace the news placeholder in AppShell, retain placeholders only for plugins/settings, and keep runtime listeners active for the full shell lifetime.

- [ ] **Step 4: Run focused navigation tests GREEN**

Run the Step 2 command and require PASS.

### Task 10: Wire release builds and provisioning assets

**Files:**
- Modify: `.github/workflows/tauri-build-reusable.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/release-please.yml`
- Modify: `docs/news-deployment.md`
- Create: `tests/frontend/newsReleaseWorkflow.test.ts`

**Interfaces:**
- Reusable workflow named secrets: `news_api_base_url` and `news_api_token`; input: `news_source_epoch`.
- Caller values: `${{ secrets.EASIFLUX_NEWS_API_BASE_URL }}`, `${{ secrets.EASIFLUX_NEWS_API_TOKEN }}`, and `${{ vars.EASIFLUX_NEWS_SOURCE_EPOCH }}`.
- Release asset: `easiflux-news-provision-<host-triple>[.exe]` plus SHA-256; the tool never prints or returns a token.

- [ ] **Step 1: Add failing static workflow assertions**

Create `tests/frontend/newsReleaseWorkflow.test.ts` using `node:fs/promises` to load all three workflow YAML files. Assert the required named secrets and epoch input are passed only through app/utility compilation environments, no secret is interpolated into a run script, Linux installs `libdbus-1-dev`, and utility assets are host-triple named, checksummed, and uploaded without `--clobber`.

- [ ] **Step 2: Run the focused workflow test and observe RED**

Run: `.\node_modules\.bin\vitest.CMD run tests/frontend/newsReleaseWorkflow.test.ts`

Expected: FAIL because the reusable inputs and provisioning asset steps are missing.

- [ ] **Step 3: Implement reusable inputs and per-platform utility upload**

Pass host/token as named repository secrets and the epoch as a protected variable from both callers. Set them only on compilation steps, build the utility in release mode, derive the host triple, copy the executable to the deterministic asset name, generate SHA-256 using platform-appropriate tooling, and upload both files to the same tag without `--clobber`. Add `libdbus-1-dev` to Linux dependencies required by the optional persistent Keyring override.

- [ ] **Step 4: Run workflow test GREEN**

Run: `.\node_modules\.bin\vitest.CMD run tests/frontend/newsReleaseWorkflow.test.ts`

Expected: PASS.

### Task 11: Full verification, production builds, and unified review

**Files:**
- Verify all changed files; edit only to fix findings.

**Interfaces:**
- Produces an uncommitted, review-ready `news/createnewscenter` branch with reproducible verification evidence.

- [ ] **Step 1: Run frontend quality gates**

Run:

```powershell
pnpm lint
pnpm test
pnpm build
```

Expected: zero failures; baseline was 52 files / 289 tests before PRD-09.

- [ ] **Step 2: Run Rust quality gates**

Run:

```powershell
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo tree --manifest-path src-tauri/Cargo.toml -e features -i keyring
```

Expected: tests and clippy pass; the keyring tree shows native persistent backend features rather than the process-only mock default.

- [ ] **Step 3: Build both release binaries with a safe fixed source**

Run in one PowerShell process:

```powershell
$env:EASIFLUX_NEWS_API_BASE_URL='https://news.example.invalid'
$env:EASIFLUX_NEWS_SOURCE_EPOCH='verification-v1'
$env:EASIFLUX_NEWS_API_TOKEN='verification-token'
cargo build --release --manifest-path src-tauri/Cargo.toml --bin easiflux-news-provision
pnpm tauri build
```

Expected: provisioning utility and all configured Tauri bundles build successfully with a fake compile-time token; the token does not appear in build output.

### Task 12: Embed the release token and retain an optional Keyring override

**Files:**
- Modify: `src-tauri/build_support/news_build_config.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/tests/news_build_config.rs`
- Modify: `src-tauri/src/storage/news_token.rs`
- Modify: `src-tauri/src/storage/news_token_tests.rs`
- Modify: `src-tauri/src/news_runtime.rs`
- Modify: `src-tauri/src/news_runtime_tests.rs`
- Modify: all three release workflows and `tests/frontend/newsReleaseWorkflow.test.ts`
- Modify: this plan, the design specification, and `docs/news-deployment.md`

**Behavior:**
- Release builds require a valid embedded token alongside the fixed host and epoch.
- A valid Keyring token overrides the embedded token; no entry, Keyring failure, or malformed stored data falls back to the embedded value.
- The app never launches `easiflux-news-provision`; the retained tool can explicitly set/delete the future override.
- Build scripts and workflow run blocks never print or interpolate host/token values.

- [x] Write and observe failing build-config, runtime fallback, and workflow contract tests.
- [x] Implement the minimum build/runtime/workflow changes and make focused tests green.
- [x] Update operator/design documentation, run full frontend/Rust verification, perform a secret-leak scan, and independently review the delta.

- [x] **Step 4: Run repository hygiene checks**

Run:

```powershell
git diff --check
git status --short --branch
```

Expected: no whitespace errors; only intentional PRD-09 content differs. The second-stage delta remains uncommitted on top of baseline commit `13f8876`.

- [x] **Step 5: Perform one independent whole-branch review and fix findings in one wave**

Compare implementation against every acceptance criterion in `docs/superpowers/specs/2026-07-30-news-center-design.md`, inspect secret handling/source binding/poller concurrency/scroll and unread semantics first, repair all confirmed findings with focused RED/GREEN tests, and rerun every affected gate plus the final full test/build set.
