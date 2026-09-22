# Threshold-once automatic strategy example

This importable manifest v6 package is an **execution demo**, not investment
advice and not evidence of a profitable strategy. Its import-free WebAssembly
guest reads the selected symbol's last price, saved guest state, the last native
receipt, and the currently visible active orders. It then:

1. waits while `lastPrice < threshold`;
2. requests the configured limit order once when the threshold is reached;
3. uses an accepted placement receipt to remember the native-owned order ID;
4. requests cancellation once only when that exact order is visible as active;
5. requests that the run stop on the following callback.

An accepted receipt means that the exchange accepted the request. It does **not**
mean the order filled, made a profit, or was later cancelled. An absent order in
a partial snapshot proves nothing, so the guest waits rather than guessing.

## Verify or rebuild

From the repository root, with the locked Rust dependencies already available:

```powershell
node examples/plugins/threshold-strategy/build.mjs --check
node examples/plugins/threshold-strategy/verify.mjs
node examples/plugins/threshold-strategy/build.mjs --write
```

`--check` proves that the canonical padded Base64 in `manifest.json` was compiled
from `strategy.wat`. The fixed-path generator accepts no caller-selected source
path and uses the repository's pinned `wat` dependency. `verify.mjs` instantiates
the packaged bytes with no imports and supplies only synthetic context, state,
receipts, and snapshots. It performs no account, keychain, network, or trade I/O.

The checked-in module is 6,105 decoded bytes and the manifest is 9,223 bytes,
within the 8,192-byte module and 16 KiB manifest limits.

## Configure the compact example input

This small WAT is intentionally not a general JSON parser. It accepts the exact
compact key order shown by `defaultInput`, with no extra fields or escapes:

```json
{"threshold":"100","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"100","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}
```

Threshold, quantity, and price are canonical non-negative decimal strings;
threshold, quantity, and price must be positive. Symbol is 1–32 uppercase ASCII
letters/digits. This example supports limit orders with `GTC`, `IOC`, or `FOK`.
The opening/reduce-only pairing must remain valid: Buy/1 or Sell/2 opens;
Sell/1 or Buy/2 is reduce-only. The host independently parses, normalizes, and
enforces every action and all run limits. Unsupported formatting or fields trap
the guest and the host fails the run closed.

The host may reserialize saved `state` objects with their keys in a different
order. The guest accepts both exact known layouts for its two order-owning states
(`phase` first or `orderId` first); it does not treat key order as authority and
still rejects unknown state fields or shapes.

## Try it deliberately

1. Read `strategy.wat` and import `manifest.json` in a build that supports v6.
   Import and preview do not execute the guest; the plugin remains disabled.
2. Enable the plugin. Enabling still does not start a strategy or grant access.
3. Connect the intended account/environment and open the strategy launcher.
   Select only the declared capabilities needed for this run: `account.read`,
   `orders.read`, `market.read`, `trade.place`, `trade.cancel`, and `strategy.run`.
4. Select the symbol and edit the compact input so its order symbol matches.
   Check threshold, side, type, quantity, price, time-in-force, position index,
   and reduce-only together.
5. Configure every mandatory native limit: `intervalMs` (5,000–60,000),
   `maxOrderQty`, `maxTotalQty`, `maxActions` (1–1,000), `maxRunSeconds`
   (60–86,400), and `reduceOnly`. Quantity limits use the exchange order
   quantity unit, not dollars. `maxTotalQty` is conservative cumulative submitted
   quantity; rejection, unknown outcome, or cancellation does not refund it.
6. Check the explicit autonomous real-trading acknowledgement and click the
   real automatic-trading start control once. There is no per-order prompt.
7. Monitor status, remaining time/action/quantity budgets, guest message, and
   last receipt. Closing the panel does not stop the run.
8. Pause or stop when needed. Stop prevents new admission but does not cancel
   outstanding exchange orders or close positions. Inspect and manage them
   separately.
9. If status is `recoveryRequired`, use reconciliation. Reconciliation queries
   the original identity; it does not replay an action or restart the run.
10. Resume only with a fresh explicit authorization. Resume retains the original
    immutable policy, counters, submitted quantity, symbol, input, and guest state.

Only one run may be active per account. At most 32 run records are retained; the
runtime does not automatically prune ownership or recovery history. At capacity,
a new start fails visibly. Do not delete the strategy state file to bypass this
safety boundary.

The worker exists only while the desktop process is alive and the computer is
awake. Sleep, shutdown, stale data, a long scheduling gap, lost authority, plugin
disable/reload, or credential replacement pauses or faults admission. Restart never
restores trading authority automatically and requires explicit resume.

See [the complete strategy runtime contract](../../../docs/plugin-strategy-runtime.md).
