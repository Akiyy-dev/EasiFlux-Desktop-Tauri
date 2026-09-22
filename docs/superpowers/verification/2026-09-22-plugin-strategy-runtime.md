# Plugin strategy runtime verification — 2026-09-22

This evidence is synthetic and checkout-local. No production `AppState`, user
profile, keychain, credential, exchange endpoint, network request, order, or
desktop application was used.

## Executable example evidence

TDD RED, before the example artifact existed:

```text
node examples/plugins/threshold-strategy/verify.mjs
Error: ENOENT ... examples\plugins\threshold-strategy\manifest.json
exit 1
```

The verifier was already loading and instantiating packaged Wasm rather than
searching source text. The failure was the intended missing-artifact boundary.

Development failures were retained rather than hidden:

- The first generator run rejected unsupported WAT opcode `i32.max_u` at line
  249. The integer maximum was rewritten using unsigned comparison and `select`.
- The first packaged execution trapped on the initial callback because the
  empty state was accidentally compared with the input terminator `}}`; a
  dedicated `{}` token fixed the state parser.
- A mutation-review test for null/rejected placement receipts then failed on a
  trailing NUL. The guest had compared null with the wrong static token and two
  fixed-output lengths were overstated. Dedicated executable cases caught and
  verified the corrected receipt branches.

Final source parity:

```text
src-tauri\target\debug\examples\build_threshold_strategy_manifest.exe --check
verified ...\examples\plugins\threshold-strategy\manifest.json
(module 5934 bytes, manifest 8995 bytes)
exit 0
```

The final WAT-only refresh used the already-built fixed-path generator executable
so it did not contend with the backend implementer's active Cargo compile. Earlier,
the public `node examples/plugins/threshold-strategy/build.mjs --check` wrapper
also exited 0 against the prior generated WAT revision; its JavaScript syntax was
rechecked after the final refresh.

Final executable behavior:

```text
node examples/plugins/threshold-strategy/verify.mjs
threshold-strategy fixture: 17 behavior checks passed; module 5934 bytes; manifest 8995 bytes; data segments 15; bulk memory false
exit 0
```

The verifier creates a real `WebAssembly.Module`, asserts it has no imports,
instantiates it, allocates actual context/input buffers, calls the four-argument
`run` export, unpacks pointer/length, and parses the returned JSON. Hand-derived
cases cover below threshold, threshold crossing with the configured order,
threshold parameter dependence, null/rejected/accepted placement receipts with
no duplicate placement, invisible/wrong owned order, exact visible owned-order
cancellation, later stop state, malformed input, unrelated action-field
injection, input over the guest's 4,096-byte bound, and both known persisted
state key orders across chained callbacks.

The decoded module is 5,934 bytes (limit 8,192) and `manifest.json` is 8,995
bytes (limit 16,384). The generator uses the locked `wat` dependency, accepts
only `--check`/`--write`, and reads the fixed example path.

## Persisted state key-order integration fix

Actual injected native-loop execution parsed guest state into
`serde_json::Value` and reserialized it as `{"orderId":...,"phase":...}`.
The original WAT accepted only `{"phase":...,"orderId":...}` and trapped after
the accepted placement callback. JSON object key order is not an authority or ABI
guarantee.

Before changing the WAT, the Node verifier was extended to recursively normalize
every persisted object key between real callback outputs. The next callback then
reproduced `RuntimeError: unreachable` at the normalized awaiting state. The WAT
now accepts both exact known key orders for `awaitingOrder` and
`cancelRequested`, while continuing to reject unknown fields and state shapes.
The 17-case GREEN above covers both orders through
accepted -> awaiting -> cancel -> cancelRequested -> stop.

## Native sandbox feature-subset integration fix

The next real Wasmi attempt rejected the otherwise Node-valid artifact before
execution: it had 51 data segments while native preflight permits at most 32,
and it used `memory.copy` while the native engine explicitly disables bulk
memory. The host sandbox was not relaxed.

The verifier now parses binary Wasm sections rather than searching WAT text. Its
RED reported `native sandbox shape: 51 data segments, bulk memory true`. The WAT
coalesces readable token tables into two documented segments and replaces
`memory.copy` with a bounded MVP `i32.load8_u`/`i32.store8` loop. Final artifact
shape is 15 data segments and no bulk-memory opcode, alongside the 17 behavior
checks above.

JavaScript syntax checks also passed:

```text
node --check examples/plugins/threshold-strategy/build.mjs
node --check examples/plugins/threshold-strategy/verify.mjs
exit 0
```

## Native packaged-loop acceptance

The backend implementer ran the checked-in packaged guest through the real
Wasmi sandbox and owned native supervisor loop:

```text
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::strategy::tests::supervisor::packaged_threshold_guest --lib --target-dir src-tauri/target
1 passed; 0 failed; 1237 filtered out
exit 0 (20.64 s compile, 0.18 s test; 37 baseline warnings)
```

The native assertions observed zero placements below threshold, one placement
on the crossing, the receipt-owned-state transition with zero early cancels,
one cancel only when the owned order became visible, and completed stop state.
Final totals were one placement, one cancellation, and one acknowledgement.
This is the decisive compatibility check for the native feature subset; the
Node engine alone is not used to claim native compatibility.

The stable manifest was also shared with the frontend implementer for strict
parser acceptance. Broad Rust/frontend/CI, CodeQL, native smoke, production app,
real-account, installer, and release tests were not run by the example task.

## Integrated native and frontend evidence

The native implementation's final focused runs use injected hosts, temporary
strategy/submission journals and the real Wasmi engine. All Cargo commands use
`--locked --manifest-path src-tauri/Cargo.toml --target-dir src-tauri/target`:

| Command/filter | Result |
| --- | --- |
| `cargo test ... plugin::strategy --lib` | 40 passed, 0 failed |
| `cargo test ... plugin:: --lib` | 426 passed, 0 failed, 2 ignored subprocess helpers |
| `cargo test ... capability_tests --lib` | 6 passed |
| `cargo test ... events::emitter::tests::native_strategy_wake --lib` | 1 passed |
| `cargo test ... services::order_submission --lib` | 9 passed |
| `cargo test ... storage::safe_plugin_document --lib` | 21 passed |
| `cargo check ... --features plugin-smoke --bin plugin-smoke` | Exit 0; compile only |

The production adapter's focused tests also passed (16 tests, including existing
v5 coverage). The two ignored plugin cases are subprocess entry points exercised
by their parent crash tests, not skipped strategy behavior. Test builds retain
37 existing warnings; the smoke feature check reports 62 dead-code warnings.

Fail-first native tests caught and verified fixes for stopping during the final
authority read, stopping during durable publication, lock-blocked shutdown
drain, frozen/rolled-back wall clocks renewing expiry, completed runs becoming
resumable, and malformed or contradictory durable candidates. Pending intent
and quantity/action debit precede dispatch; receipt and ownership precede
submission acknowledgement. Unknown actions are never automatically replayed.

Frontend verification used mocked Tauri IPC only:

- Initial focused integration: 11 Vitest files, 625 tests passed.
- Independent-review fix round: two focused Vitest files, 33 tests passed.
  Previously failing cases cover emergency stop during access/start/reconcile,
  stale refresh cleanup, required capabilities, fixed lifetime and monotonic
  resume counters.
- `vue-tsc --noEmit` and scoped ESLint exited 0 after those fixes.
- Final `vite build` exited 0 (4,775 modules). Vite retains its advisory warning
  about the existing large application chunk; no installer was built.
- The example verifier now rejects impossible context/receipt chronology; the
  corrected fixture still passes all 17 actual execution cases and source parity.

Independent native and whole-branch review, broad cross-platform CI and CodeQL
are separate merge gates; local focused results alone do not claim those gates
passed. Native-window acceptance, real-account testing and release remain outside
this verification.
