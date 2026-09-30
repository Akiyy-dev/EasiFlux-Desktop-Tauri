# Managed-entry strategy (manifest v7)

This executable teaching fixture requests one protected **Buy Limit** entry,
waits for its matching native acknowledgement and exact visible owned order,
requests one price amendment without increasing quantity, then stops after the
matching amendment acknowledgement. It is not a trading recommendation.

Input (all six fields required; JSON key order does not matter):

```json
{"symbol":"BTCUSDT","qty":"0.001","entryPrice":"50000","amendPrice":"50010","takeProfit":"55000","stopLoss":"45000"}
```

The example always uses Buy, Limit, GTC, positionIdx 1, reduceOnly false and
LastPrice triggers, with both TP and SL. The native contract also permits Sell,
MarkPrice and one nullable protection leg; this example deliberately does not
expose those options. Prices/quantity are positive bounded decimal strings.
Both entry and amendment prices must remain strictly between SL and TP.
The amendment price must differ from the entry/current visible price.

Lifecycle:

1. Empty state at initial start/sequence 1 requests the protected entry and stores
   its callback sequence. Empty state at a later callback stops.
2. Only a matching `placeProtectedOrder` receipt with accepted status supplies the
   native exchange `orderId` and client `submissionId`. The guest persists both.
3. Wait for exactly one visible active Buy Limit row with that order ID, client
   ID and symbol. Missing, foreign, terminal or duplicate rows never authorize an
   amendment. The requested total qty must not exceed visible qty and must exceed
   filled qty. The native host independently verifies immutable protected identity.
4. Persist `amendRequested` with the new callback sequence and request exactly one
   amendment. Matching accepted amendment acknowledgement stops; missing/stale
   receipts wait without resubmission. Rejected/unknown outcomes stop in the guest.

The native supervisor normally intercepts unknown mutations as
`recoveryRequired` before another callback. Resolve them by read-only native
reconciliation, never by replaying an action. Accepted is request acknowledgement,
not a fill, final amended state, active TP/SL, or guaranteed protection. Stopping
does not cancel the order or close a position.

## Authority and limits

Manifest v7 alone declares `trade.protect` and `trade.amend`. Protection requires
`trade.place` + `market.read`; amendment additionally requires `orders.read`.
The manifest also declares `account.read` and `strategy.run`. Import and enable
grant nothing: capabilities start unchecked and require explicit automatic-trading
authorization. Catalog transport remains v3 and strategy IPC remains schema 1.

For this input, one place plus one amend consumes two actions and cumulative
quantity **0.002**, not 0.001. Each amendment debits its full new total quantity,
including rejected/unknown dispatches; cancellation does not refund budgets.
The existing mandatory per-run limits remain unchanged (interval 5000–60000 ms,
lifetime 60–86400 s, 1–1000 actions, positive maxOrderQty/maxTotalQty with the former
no greater than the latter). Native risk checks also apply.

No position-level TP/SL, leverage, margin/mode changes, multi-symbol trading,
arbitrary network/WASI, credentials, automatic restart or real-account acceptance
is included.

## Reproducible offline verification

From the repository root, with Node and the locked Rust toolchain available:

```sh
node examples/plugins/managed-entry/build.mjs --write
node examples/plugins/managed-entry/build.mjs --check
node examples/plugins/managed-entry/verify.mjs
cargo test --locked --manifest-path src-tauri/Cargo.toml packaged_managed_entry --lib --target-dir src-tauri/target
```

`build.mjs` permits only `--check` (default) or `--write` and invokes the pinned
fixed-path Rust generator; no user-supplied file paths or dependencies are added.
The threshold v6 generator shares this small helper and keeps identical bytes.

`strategy.wat` is an import-free MVP module: bounded structural field lookup,
32-level nesting ceiling, allowlisted copied strings, exact decimal comparison,
no mutable globals or bulk memory, at most 32 data segments. Input/state are
bounded at 4096 bytes, module at 8192, manifest/output at 16 KiB, and memory at
16 pages. Invalid selected data traps or waits/stops without trading. This is a
small example parser for native sanitized JSON, not a general-purpose JSON SDK.
The verifier instantiates checked-in bytes; Rust exercises those same bytes with
the real Wasmi preflight/fuel and supervisor using injected hosts/temp journals.

See the [runtime contract](../../../docs/plugin-strategy-runtime.md),
[design and checked public API references](../../../docs/superpowers/specs/2026-09-29-plugin-trading-management-design.md#primary-api-references-checked-2026-09-29),
and [verification evidence and remaining gates](../../../docs/superpowers/verification/2026-09-29-plugin-trading-management.md).
No app, installed profile, authenticated API or real exchange order was used.
