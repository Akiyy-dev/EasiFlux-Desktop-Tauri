# Plugin trading-management verification — 2026-09-29

Development verification uses synthetic fixtures, injected native hosts, temporary
journals and mocked IPC. No installed-app profile, credentials, authenticated API,
exchange order or live desktop application is used. A successful request receipt
does not establish fills or active TP/SL protection.

## Baseline and isolation

- Exact GitHub main: `8f81ed3832415269b879ff411eaa979e1254e832`.
- Branch: `plugin/trading-management` in the existing isolated
  `target/worktrees/plugin-update-preflight` checkout.
- Git HTTPS fetch could not connect. The GitHub connector supplied Git blobs,
  trees and signed commits; each imported object's SHA matched before advancing
  `origin/main` and creating the feature branch. No shallow boundary was changed.
- Original checkout `dev/trading-risk-order-recovery` and its pre-existing dirty
  files were preserved. Temporary hydration files were removed after verification.

Pre-implementation baseline (not evidence of new feature completion):

| Command | Result |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::strategy --lib --target-dir src-tauri/target` | 49 passed; 0 failed; existing Rust warnings |
| `node_modules/.bin/vitest.cmd run tests/frontend/pluginStrategyService.test.ts tests/frontend/pluginStrategyUi.test.ts` | 2 files / 33 tests passed |

The frontend baseline initially encountered Vite temporary-cache `EPERM` under
the linked dependency directory. The approved escalated retry passed without a
source change. Baseline source matches PR42; main's subsequent changes are release
metadata only.

## Native task evidence (before final integration)

Native implementation commits: `044e0eb` and `cfbf7f0`.

Focused commands used `cargo test --locked --manifest-path src-tauri/Cargo.toml
<filter> --lib --target-dir src-tauri/target -q`:

| Filter | Result | Snapshot |
| --- | --- | --- |
| `plugin::strategy` | 58 passed / 0 failed | Initial native implementation |
| `plugin::workflow::production` | 20 passed / 0 failed | Initial native implementation |
| `api::private::tests::` | 5 passed / 0 failed | Initial native implementation |
| `api::mapper::tests::place_order_body` | 1 passed / 0 failed | Initial native implementation |
| `services::order_submission` | 9 passed / 0 failed | Initial native implementation |
| `plugin::strategy::tests::supervisor::protected_owned` | 2 passed / 0 failed | Corrected protected-owned amendment |
| `plugin::strategy::tests::supervisor::unknown_protected_owned_amend` | 1 passed / 0 failed | Corrected protected-owned amendment |
| `plugin::strategy::tests::supervisor::amend_` | 2 passed / 0 failed | Corrected protected-owned amendment |
| `plugin::strategy::tests::supervisor::unknown_amend_survives` | 1 passed / 0 failed | Corrected protected-owned amendment |
| `plugin::strategy::tests::supervisor::stop_during_amend` | 1 passed / 0 failed | Corrected protected-owned amendment |
| `plugin::workflow::production::management::tests` | 6 passed / 0 failed | Corrected protected-owned amendment |

Fail-first correction evidence: matching protected normal Limit orders initially
failed with `plugin_strategy_invalid_output`; after lifting that exclusion, tests
exposed missing entry-price bounds against immutable TP/SL. Both were corrected.
Coverage includes raw protection identity, unexpected protection on an originally
unprotected order, bounds, restart without replay, and stop during preparation.
`git diff --check -- src-tauri/src` exited 0. Existing Rust dead-code warnings and
Git CRLF conversion warnings remain; the above is not a pristine-output claim.

These task-level runs are not a substitute for final integration checks. No live
exchange acceptance, desktop UI acceptance, fills or active protection is claimed.

Native independent review found three production-path gaps, fixed in `dd06597`
and approved by scoped re-review on 2026-09-30:

- Recognize only documented no-condition/unused-protection sentinels while keeping
  exact identity and alias checks. Fixtures derive from the public open-order and
  order-history responses linked in the design.
- Preserve explicitly empty Market price in ID-only acknowledgement and strict
  protected recovery. A parser -> durable journal -> supervisor receipt test and
  production acknowledgement test exercise the actual chain without fabricated
  observed price or status.
- Fetch the selected LastPrice/MarkPrice after awaited preparation and reservation,
  before final admission. Crossing or unavailable protection quotes reject locally
  with zero admission/mutation, released risk occupancy, and a Rejected journal.

All three were reproduced failing before their fixes. Final focused filters at
`dd06597` passed with zero failures: production management 9, production strategy
11, production root tests 6, Market receipt chain 1, trading strategy 5, selected
protection quote 1. Native whitespace check exited 0. No full suite was repeated
at this task gate. Inherited warnings cover unused API/response/model helpers,
storage atomic-file helpers and WebSocket methods/topics; unrelated cleanup was
not included. The reviewer found no new breakage in the fix.

## Frontend task evidence (before final integration)

Commit `57ba3b5` adds v7 declarations, explicit initially-unchecked grants, launch
and immutable-resume validation, correlated management receipts and Chinese
acknowledgement-only presentation through the installed plugin workbench.

The final affected-file Vitest invocation passed 9 files / 745 tests, zero failures:
`pluginService`, `pluginStrategyService`, `pluginStrategyUi`, `pluginStore`,
`pluginCommandWorkbench`, `pluginCard`, `pluginImportDialog`, `pluginManifestDiff`
and `pluginMarketplacePage` under `tests/frontend/*.test.ts`. `vue-tsc --noEmit`,
scoped ESLint on changed production/test files, and staged whitespace validation
each exited 0. Typecheck and ESLint emitted no output. Initial Vite linked-cache
EPERM required an approved retry; Git still reports LF/CRLF conversion warnings.

Fail-first checks covered v7 parser/access/resume rejection before implementation,
missing launcher permissions, unchecked dialog controls, strict receipt identities
and grant correlation, and version-specific Chinese permission descriptions.
These are task-level synthetic tests, not live exchange or desktop acceptance.
Independent frontend task review returned spec compliant and quality approved,
with no findings. Its cross-task native guarantees were covered by the separately
reviewed Task 1. The consolidated integration gate remains pending.

## Executable example task evidence (before final integration)

Task 3 adds the checked-in manifest v7 managed-entry package and readable WAT,
shared fixed-path single-strategy generator, CI verification steps and authoring
handoff. No runtime authority or trading behavior was changed in this task.

Fail-first evidence:

- Before any generated manifest existed, `node examples/plugins/managed-entry/verify.mjs`
  exited 1 with `AssertionError: managed-entry packaged manifest must exist`.
- Self-review added a behavioral regression for empty state outside initial
  start/sequence 1. The actual packaged module initially emitted placement and
  the `none/stop` assertion failed. The WAT now requires start, sequence 1 and
  null prior receipt; regenerating and rerunning passed.
- An initial Rust test assertion used Option for the nonoptional position index
  and failed compilation. The test-only type error was corrected; it is not
  counted as behavioral RED.

Final focused evidence (commands from the isolated checkout; cargo is
`C:/Users/ROG/.cargo/bin/cargo.exe` on this Windows host):

| Command | Result |
| --- | --- |
| `node examples/plugins/managed-entry/verify.mjs` | 13 behavior groups passed; 0 failures |
| `node examples/plugins/managed-entry/build.mjs --check` | Source/package parity; module 4740 bytes, manifest 7342 bytes |
| `node examples/plugins/threshold-strategy/build.mjs --check` | Preserved v6 parity; module 5934 bytes, manifest 8995 bytes; manifest Git diff empty |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml packaged_ --lib --target-dir src-tauri/target -q` | 2 passed, 0 failed, 1287 filtered out |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::strategy::tests::supervisor::protected_owned --lib --target-dir src-tauri/target -q` | 2 passed, 0 failed, 1287 filtered out |
| Both Node wrappers with `--arbitrary-path` and `--check elsewhere` | Each exits 2 with usage; no arbitrary path allowed |
| Managed-entry Rust generator `--check elsewhere`; threshold Rust generator `--arbitrary-path` | Each exits 1 with extra-argument/usage rejection |
| `git diff --check` | Exit 0 |

The packaged guest has two data segments, no imports or bulk-memory instructions,
and stays below existing 8192-byte module/16-KiB manifest limits. Input/state remain
bounded at 4096 bytes. Node covers input dependence of both entry/amendment,
matching kind/sequence/identity, missing/malformed/foreign/duplicate visibility,
rejected/unknown receipts, nonincreasing total quantity, filled-quantity bounds,
exact decimal comparison, one amendment, completion, and reordered persisted state.

The actual packaged Wasmi/supervisor test asserts one protected Buy Limit entry
(BTCUSDT, qty 0.001, price 50000, GTC canonicalized to GoodTillCancel, positionIdx 1,
reduceOnly false, TP 55000 / SL 45000, LastPrice), no amendment while invisible,
one amendment of native orderId owned-1 to 50010 / total qty 0.001, two actions,
cumulative debit 0.002, matching accepted amendment receipt, Completed, no later
replay/cancellation, and unchanged original protection metadata. Its companion
v6 packaged threshold test remains green.

Inherited Rust unused-code/linker warnings and Git LF/CRLF notices remain; no
pristine-output claim is made. The generator extraction did not touch the
separate account-workflow generator or add dependencies. Tests used injected
hosts and temp storage only. Grants remain explicit/unchecked; acknowledgement
does not prove active protection, and stop does not cancel orders/close positions.

Task review, consolidated broad frontend/Rust/example checks, whole-branch review,
exact-head CI and integration remain controller-owned and pending. No real account,
installed profile, desktop launch or exchange acceptance is claimed.
