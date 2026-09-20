# Executable local computation plugins

## Purpose and decision

The user explicitly prioritizes a usable executable-plugin release over further marketplace administration, and authorizes autonomous implementation and suitable PR merges. Deliver one complete input → guest computation → result workflow. Keep the unrelated root trading worktree untouched.

Use wasmi (pinned, no default features except the deliberately selected standard-library support), not JavaScript in the main WebView or native DLLs. A no-import WebAssembly interpreter provides a small versioned ABI and explicit resource accounting. Rhai is a credible simpler author language but introduces another language/builtin surface; QuickJS adds a larger scripting/module bridge. Supporting multiple engines is out of scope.

## User experience

Import the supplied SMA example JSON using the existing preview. Preview explicitly identifies executable local code and its limitations. Import remains disabled by default. Enable it, open its computation command from the existing plugin command UI, paste a numeric series, choose the numeric parameter, click Run, see the scalar result and provenance. `1,2,3,4,5` with period `3` returns `4`, computed inside the supplied guest module, not a host SMA implementation. Cancel, failure, stale context, and resource exhaustion have clear states. No import, enable, render, search, or navigation action executes guest code automatically.

Input and results are memory-only. They are never logged or persisted. No production app/account/keychain/AppData access is used for development validation.

## Manifest and ABI

Keep v1–v3 byte/fingerprint behavior compatible. Manifest schema v4 permits the previous commands plus `sandbox.computeSeries`. Its exact params are:

```json
{"runtime":"wasm-v1","abi":"series-f64-v1","moduleBase64":"<canonical padded standard Base64>","parameter":{"label":"Period","default":3,"min":1,"max":4096}}
```

Parameter metadata uses integer values (safe exact integers, bounded to ±1000000; min ≤ default ≤ max); the runtime parameter supplied by the user is a finite number within that range, allowing fractional parameters for other algorithms. Labels use the existing nonblank 80-byte text rule. Unknown/duplicate keys, mixed action/params, noncanonical Base64, invalid headers, empty or decoded modules over 8192 bytes are rejected. Whole manifest stays at the existing 16 KiB limit, requestedCapabilities stays empty, and contributions remain 1–16. Imports/start/unsupported ABI are rejected before running. Module bytes stay in the contribution so existing canonical fingerprints bind enablement to code and metadata, without changing the managed manifest+receipt file shape.

Fixed exports: `memory`, `alloc(bytes: i32) -> i32`, `run(ptr: i32, count: i32, parameter: f64) -> f64`. No caller-selected export names. Host accepts 1–4096 finite f64 values; writes little-endian bytes only after checked pointer/length bounds; accepts only a finite scalar result. A fresh instance/store per run prevents hidden persistent guest state. Module comparison identifies runtime/ABI/parameter/code changes without rendering the Base64 blob.

## Authority and lifecycle

Add main-WebView-only execute/cancel IPC. Execute receives plugin/contribution ID, a bounded opaque request ID, expected catalogGeneration/revision, input numbers, and parameter — never module bytes, paths, export names, or capabilities. Rust resolves current registry content and requires available state, enabled content-bound identity, v4 compute contribution, and no pending/unreconciled lifecycle authority. Capture immutable module+identity, release registry locks, run in blocking worker, then revalidate identity/enabled/generation/revision before returning. Errors expose fixed codes, never guest traps/bytes/paths or user input.

One process-wide running slot; concurrent run fails busy. Cancellation matches the active request ID and sets a flag; cancellation of a stale ID must not affect a newer run. An owned worker retains the slot until actual exit even if the IPC future is dropped. Disable/removal/reload/publication must cancel or invalidate current execution; stale output must never publish. Frontend additionally keys pending work to current context and discards late output, including when navigating away/unmounting. No retries of execution.

## Sandbox limits

No imports at all, no WASI, no network/files/OS/Tauri/account/credential/order bridge. Reject start functions, keep only needed Wasm proposals, eager validation/translation with structural limits. Linear memory ≤1 MiB, one memory, no tables if practical (otherwise strictly bounded), bounded functions/globals/stack/recursion. Total fuel 2000000; cooperative fuel slices 10000; deadline 2 seconds checked between slices, sharing budget across allocation and run. Slot released only after worker exit. Check cancellation between module preparation and invocation as well. Compilation is bounded by bytes/structure, not claimed to be interruptible or hard real-time. Store limits are not an entire-process memory guarantee; this is interpreter isolation, not a separate OS sandbox process.

## Acceptance and non-goals

Required evidence: actual SMA guest gives 4; invalid module/import/start/ABI/pointer/nonfinite input/output rejected; loop reaches budget and cancellation exits; stale/disabled/changed identities cannot execute or return results; one slot is enforced; old manifests still work; strict frontend parser and all UI action branches understand v4; main-only ACL registrations agree in production and isolated smoke. Focused tests plus one final CI run, not repeated repository-wide tests.

Deliver author-readable WAT source, reproducible build/check script, ready-to-import manifest, usage steps, and a real isolated native smoke route if the environment permits. Do not claim native GUI verification when only unit/integration tests were run.

This version supports EasiFlux-ABI local numerical algorithms on explicitly supplied data. It does not run arbitrary third-party plugins, JS/npm packages, WASI applications, background jobs, arbitrary plugin UI, live market subscriptions, or trades. Signing/marketplace publishing/automatic updates remain later work.

## References

- https://docs.rs/wasmi/2.0.0/wasmi/struct.Config.html
- https://docs.rs/wasmi/2.0.0/wasmi/enum.TypedResumableCall.html
- https://docs.rs/wasmi/2.0.0/wasmi/struct.StoreLimitsBuilder.html
