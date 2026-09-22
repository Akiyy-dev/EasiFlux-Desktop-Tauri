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
(module 6105 bytes, manifest 9223 bytes)
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
threshold-strategy fixture: 17 behavior checks passed; module 6105 bytes; manifest 9223 bytes
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

The decoded module is 6,105 bytes (limit 8,192) and `manifest.json` is 9,223
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

JavaScript syntax checks also passed:

```text
node --check examples/plugins/threshold-strategy/build.mjs
node --check examples/plugins/threshold-strategy/verify.mjs
exit 0
```

## Native and frontend acceptance status

The checked-in manifest path and sizes were shared with the frontend implementer
for strict parser acceptance and with the backend implementer for injected-loop
acceptance. At the time this section was written, no result from either task had
been incorporated here.

In particular, this document does **not** yet claim that the native supervisor
has launched the guest, automatically placed and cancelled exactly once, or
verified journal/ack ordering. Those are backend injected-host acceptance
requirements. Broad Rust/frontend/CI, CodeQL, native smoke, production app,
real-account, installer, and release tests were not run by the example task.
