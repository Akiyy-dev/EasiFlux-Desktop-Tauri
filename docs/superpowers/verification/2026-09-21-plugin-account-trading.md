# Plugin account trading verification

Work date: 2026-09-21 (Asia/Shanghai). This is a local verification snapshot for
the `plugin/account-trading` branch. It is not a packaged-release claim and does
not claim that any real exchange account, credential, order placement, or
cancellation was exercised.

## Scope and safety boundary

Manifest v5 can run an import-free `account-json-v1` guest against a bounded,
explicitly granted account snapshot. Run only creates a display result or a
proposal. Each place/cancel mutation requires a separate token-only user
confirmation, and session grants are revocable and bound to plugin content,
runtime epoch, account, environment, and a private installed-session authority.

The native smoke uses `WorkflowHostState(Arc<dyn WorkflowHost>)` with fixed
synthetic values. It does not create `AppState`, load profiles, open a keychain,
construct a production API client, or make a network request. The guest has no
imports and never receives the private authority, API key, Secret, filesystem,
network, clock, or randomness. The smoke still probes that
`list_account_profiles` is forbidden from the local main WebView.

## Backend evidence

Task 1's latest focused backend evidence, all against injected/in-memory fixtures:

| Scope | Result |
| --- | --- |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib plugin:: -- --test-threads=1` | 374 passed, 0 failed, 2 ignored |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml --lib capability_tests -- --test-threads=1` | 6 passed, 0 failed |
| Earlier workflow-focused run | 26 passed, 0 failed, including private installed-session rotation |

The post-review cancellation acknowledgment provenance fix then passed its two
focused adapter tests plus one workflow receipt test. Production now requires an
actual matching exchange acknowledgment; an accepted cancel receipt carries an
`Unknown` order status rather than claiming terminal cancellation. The strict
service logs request acceptance but does not publish the incomplete acknowledgment
as an order event or append it to history/analytics. This preserves existing
order-center data until authoritative WebSocket/refresh observations arrive.
The legacy cancellation path and public host/IPC wire contract are unchanged.

An earlier plugin run reported 365 passed, 1 failed, 2 ignored because the
pre-existing Windows removal crash-checkpoint fixture hit OS error 5 during a
native rename. The final plugin scope and the exact removal-test retry both
passed. This work did not root-fix that intermittent Windows filesystem failure.
Backend tests were partly written alongside implementation; this record does not
claim strict test-first development for every new broker function.

## Frontend evidence

Task 2's final pre-review focused workflow run passed 14 tests. Its earlier
15-file touched-plugin regression passed 770 tests, and `vue-tsc --noEmit` plus
scoped ESLint completed without diagnostics. After review corrections for
capability/proposal correlation, leading-zero decimals, environment length,
pre-dispatch outcome copy, and timestamps, the focused workflow/import group
passed 500/500; typecheck and scoped ESLint also passed.

Task 3's smoke driver failed first because the retained v4 removal result left
the subsequent v5 remove control disabled. After explicitly dismissing that
result, the focused driver passed:

```text
tests/frontend/pluginSmokeSelfTest.test.ts
Test Files 1 passed (1)
Tests 4 passed (4)
```

The assertions cover the legacy v4 flow, v5 no-grant access, explicit five-item
grant, two Runs with no mutation, token-only place/cancel confirmations, two
receipts, revocation blocking a future Run, disable/reload, forbidden account
IPC, and removal.

The final integrated production Vite build transformed 4,767 modules and exited
0 in 6.32 seconds. It retained the existing advisory that the main minified
chunk is larger than 500 kB; this is not a build failure.

## Example and smoke-host evidence

`node examples/plugins/account-workflow/verify.mjs` passed five checks against
the actual Base64 bytes in the checked-in manifest. The balance display changes
from `12.5` to `8` with the granted context; place output preserves the complete
user JSON; cancel output selects the captured synthetic order ID. The manifest
is at most 16 KiB, each decoded module is at most 8,192 bytes, and all modules
have zero imports.

The source generator initially failed on missing output and then exposed two WAT
stack-shape errors (`not enough arguments on stack for call` and a non-empty
fallthrough stack). Correcting the branch pointer and result-valued `if` produced
a reproducible `--write` output and a passing parity check.

Focused native Rust checks before the one UI acceptance attempt:

| Scope | Result |
| --- | --- |
| `plugin::smoke::tests` with `plugin-smoke` | 7 passed |
| `plugin_smoke::tests` with `plugin-smoke` | 10 passed |
| Synthetic host counter test | 1 passed |
| Dedicated `plugin-smoke` binary build in `src-tauri/target` | Passed |

These checks execute the packaged v5 WAT with the production interpreter and
prove that the injected host counts exactly one place and one cancel operation.
They do not invoke production account infrastructure.

## Isolated native acceptance

The first native acceptance attempt used the standard installed Vite loader, an
explicit worktree root/config, hidden owned helper PID, and the dedicated
`plugin-smoke` binary. Vite reported ready in 435 ms at
`http://127.0.0.1:1430/`, but the native process reached its 90-second hard
deadline and exited 1 before the WebView self-test imported either fixture. It
did not produce `report.json`; the isolated `plugins/local`, `import-staging`,
and `removal-staging` directories remained empty. This is a failed native
attempt and is not omitted from the record.

Retained evidence is under:

- `target/native-run-d38cc528-9e06-45d7-83f4-646dd6e06bcc/vite.stdout.log`
- `target/native-run-d38cc528-9e06-45d7-83f4-646dd6e06bcc/vite.stderr.log`
- `target/native-run-d38cc528-9e06-45d7-83f4-646dd6e06bcc/plugin-smoke-0ff207d4-f202-4291-b866-7b250674ddb2/`

The stderr log is empty. The first sandboxed helper attempt also remains under
`target/native-run-ea509e23-b095-4849-b304-98a915f143f3/`; it failed with
`EPERM` before the server started and did not launch the native binary. Only the
owned helper PID 79384 was force-stopped after the bounded native attempt, and
port 1430 no longer had a listener.

The controller then added three static native startup milestones plus page
started/finished diagnostics (no URL or private value logging), rebuilt the
dedicated binary, and ran a controlled diagnostic against the already-warm Vite
server. That process exited 0. Its retained report has `passed: true`,
`uiSuccess: true`, `detailValid: true`, `nativeError: null`, source unchanged,
all managed/staging/ownership directories empty, the disabled decision retained,
and exactly `placeMutations: 1` plus `cancelMutations: 1`:

`target/native-diagnosis-a5be939f-713f-4571-b625-082ec0fbce99/plugin-smoke-09bf7535-a145-4849-a4ee-bdf5c00b1232/report.json`

The native stderr log in that parent records host construction, WebView
construction, event-loop start, page `Started`, page `Finished`, report exit 0,
then the same post-exit Chromium class-unregistration error 1412 seen in earlier
smoke work. The initial timeout's root cause is not proven or fixed; the passing
warm-server diagnostic shows the integrated synthetic flow can complete, not
that startup is flake-free.

## Limits and remaining gates

The first PR CI frontend run passed 1,428 tests and failed one security allowlist
expectation that still enumerated the previous nine IPCs. The expectation now
enumerates the intended thirteen commands explicitly, while retaining all
forbidden filesystem/dialog/shell/remote authority assertions and main-WebView
restriction. The focused four-test security file passed after this correction;
the final updated-head CI result remains the merge gate.

- Native UI acceptance passed on the controlled diagnostic run, but the earlier
  startup timeout remains unexplained and must not be hidden.
- No production profile, account, key, private endpoint, live HTTP, or real
  place/cancel request was used. Passing mock acceptance would still not prove
  live exchange execution.
- The connected exchange API key must independently have server-side read/trade
  permission; plugin grants cannot elevate it.
- Conditional/TP-SL orders and unattended/background strategies are outside
  this phase. `PendingCancel` may be shown as `Unknown` and is not cancellable.
- Accepted cancel means the exchange request was acknowledged, not that the
  order is terminally cancelled. Unknown outcomes must be reconciled rather
  than blindly retried.
- Final whole-branch review and CI remain required before merge.
