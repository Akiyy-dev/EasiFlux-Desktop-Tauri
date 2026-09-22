# Native-supervised plugin strategies (manifest v6)

Manifest v6 adds `sandbox.strategy`: an import-free WebAssembly guest makes one
bounded decision per callback while a native supervisor owns scheduling, fresh
data, authorization, policy enforcement, durable intent/receipt state, order
ownership, and recovery. A strategy can place or cancel ordinary orders without
a per-order prompt **only after one explicit real automatic-trading start**.

Import, preview, enable, access inspection, page rendering, reconnect, and app
restart never start or resume a run. This runtime is an execution facility, not
investment advice, a profitability claim, a hosted service, or an HFT engine.

## Manifest and capabilities

The contribution parameters are exactly:

```json
{
  "runtime": "wasm-v1",
  "abi": "strategy-json-v1",
  "moduleBase64": "<canonical padded Base64>",
  "defaultInput": "{}"
}
```

A v6 manifest contains at least one `sandbox.strategy` command and requests
`account.read` plus `strategy.run`. Its unique requested capabilities may also
use the seven v5 names: `balances.read`, `positions.read`, `orders.read`,
`market.read`, `trade.place`, and `trade.cancel`. Cancellation additionally needs
`orders.read`. V6 can mix show-info, fixed-page, and numeric-compute commands, but
cannot contain a v5 `sandbox.accountWorkflow`. V1–v5 cannot request `strategy.run`.

The decoded module limit is 8,192 bytes and the whole manifest limit is 16 KiB.
Import/enable grants nothing, and changing plugin content invalidates captured
authority. The guest never receives API credentials, a private installation ID,
raw provider responses, filesystem paths, host handles, or arbitrary network I/O.

## Guest ABI

The module has no imports, WASI, or start function and exports:

```text
memory
alloc(length: i32) -> i32
run(context_ptr: i32, context_len: i32, input_ptr: i32, input_len: i32) -> i64
```

The packed result uses the unsigned high 32 bits for the output pointer and low
32 bits for its length. Existing sandbox limits remain 1 MiB memory, 2,000,000
fuel, and a two-second callback deadline. Input JSON is an object up to 4,096
UTF-8 bytes; context is at most 65,536 bytes; output is at most 16,384 bytes; the
serialized guest `state` object is at most 4,096 bytes.

The context is exactly:

```text
{schemaVersion:1, runId, sequence, event, snapshot, state, lastReceipt}
```

`sequence` is a canonical u64 decimal string and `event` is `start`, `timer`, or
`update`. `snapshot` is the sanitized granted-data DTO documented for v5,
including capture timestamps and partial flags. Each callback gets a new
authoritative read. `lastReceipt` is null or the latest sanitized native receipt:

```text
{sequence, kind:"placeOrder"|"cancelOrder",
 status:"accepted"|"rejected"|"unknown",
 submissionId:string|null, orderId:string|null, errorCode:string|null}
```

Accepted means the request was accepted; it does not mean filled, cancelled,
profitable, or terminal. Missing data in a partial snapshot never proves an order
or position disappeared.

Output is exactly `{state:object, action:Action, message:string}`. Message is
plain text up to 2,000 UTF-8 bytes. A callback requests at most one action:

```json
{"kind":"none"}
```

```json
{"kind":"stop"}
```

```json
{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}
```

```json
{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"owned-exchange-id"}}
```

The host validates the strict union and all trading invariants independently.
Only the selected symbol can be traded. Cancellation is allowed only for an order
this run demonstrably placed and that is currently visible as cancellable.

## Explicit start and immutable limits

The launcher displays the current account/environment, all declared capabilities
initially unchecked, selected symbol, JSON parameters, mandatory policy, and an
initially unchecked autonomous real-trading acknowledgement. A start needs a
fresh one-use native ticket and exactly:

```json
{
  "intervalMs": 5000,
  "maxOrderQty": "0.001",
  "maxTotalQty": "0.01",
  "maxActions": 20,
  "maxRunSeconds": 3600,
  "reduceOnly": false
}
```

- `intervalMs`: integer 5,000–60,000.
- `maxRunSeconds`: integer 60–86,400.
- `maxActions`: integer 1–1,000.
- Quantity limits: positive Decimal strings up to 64 bytes, with
  `maxOrderQty <= maxTotalQty`.
- `reduceOnly: true` forces every placement to reduce-only.

These limits are immutable and remain enforced when global risk is disabled.
Quantity is the exchange order quantity unit, not a dollar/notional amount.
`maxTotalQty` is conservative cumulative submitted quantity: rejected and unknown
submissions count, and cancellation does not refund it. These caps are not maximum
position, notional, loss, or profit guarantees; global trading risk also applies.

There is one active run per account, one callback/action in flight per run, at
most four workers globally, and at most 32 retained run records. Records are not
automatically pruned because they carry ownership and recovery history. At
capacity a new start fails visibly. Do not delete the state file to make room or
bypass unresolved actions.

## Scheduling, pause, stop, and restart

Timers are anchored to completed passes, skip missed ticks, and coalesce native
wakeups. Fresh snapshots are read for every pass. A read error, read taking more
than 30 seconds, lost authority, invalid guest, or scheduling gap greater than
`max(30 seconds, 3 * intervalMs)` pauses or faults admission rather than using
stale data. Reconnect does not restore authority automatically.

Pause/stop synchronously prevents new admission and signals guest cancellation.
If a network mutation was already admitted, the owned worker records its receipt
before draining; the UI may therefore show `stopping`. Stop does not cancel
exchange orders, close positions, or liquidate. Emergency stop remains available
after plugin removal or account disconnection. Never report stop success after an
unknown IPC/network result without refreshing authoritative status.

The worker runs only while the desktop process is alive and the computer is
awake. Closing the strategy panel does not stop it. Sleep, shutdown, or app exit
can interrupt scheduling. On restart, formerly active runs load paused without
live authority. No action is replayed and no strategy restarts automatically.

## Receipts, recovery, and resume

Before HTTP dispatch, native code debits action/quantity budgets and durably
records a pending canonical action. It then persists the sanitized receipt,
ownership, next guest state, and counters before another callback. An accepted
placement is acknowledged in the order journal only after durable strategy state
agrees. Unknown outcomes are never retried automatically.

`recoveryRequired` means the original action needs read-only reconciliation by
its exact identity. Reconciliation does not replay an action or restart a run;
not-found remains unresolved. Resume requires a fresh explicit authorization for
the same plugin content, account/environment, symbol, input, capabilities, and
unchanged policy. It retains counters, submitted quantity, state, ownership, and
expiry; renewal is not a budget reset.

## Authoring and example

[Threshold-once strategy example](../examples/plugins/threshold-strategy/README.md)
contains readable bounded WAT, a fixed-path reproducible generator, and a Node
verifier that instantiates the packaged bytes with synthetic inputs. The example
inspects price, guest state, native receipt, and visible owned orders; it does not
return one canned action.

Out of scope for v6 are guest-origin arbitrary networking/filesystem access,
credentials, withdrawals, transfers, conditional orders, TP/SL, multi-account
strategy concurrency, automatic restart authorization, historical indicator SDKs,
hosted marketplace delivery, and installer/release publication.
