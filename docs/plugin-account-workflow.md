# Plugin account workflows (manifest v5)

Manifest v5 adds `sandbox.accountWorkflow`: an import-free WebAssembly guest can
transform a host-captured, explicitly granted account snapshot plus bounded user JSON
into plain text or a proposed place/cancel action. The guest never receives API keys,
private session authority, provider clients, filesystem, network, clock, randomness,
or a host-call import.

This is not unattended trading. Import, enable, grant, Run, preview, and proposal
creation never mutate an account. Every place or cancel proposal requires a separate
host-rendered confirmation for that exact account and canonical request.

## Manifest and capabilities

The contribution parameters are exactly:

```json
{
  "runtime": "wasm-v1",
  "abi": "account-json-v1",
  "moduleBase64": "<canonical padded Base64>",
  "defaultInput": "{}"
}
```

A v5 manifest must include at least one workflow and request `account.read`.
`requestedCapabilities` is a unique subset of:

- `account.read`
- `balances.read`
- `positions.read`
- `orders.read`
- `market.read`
- `trade.place`
- `trade.cancel`

The list declares what the plugin may ask for; it grants nothing. A connected user
selects a subset for the current application session. Nonempty grants must include
`account.read`. Revocation and account, connection, content, catalog, or session changes
invalidate pending work. Old manifest versions cannot request these capabilities or use
the action.

Plugin grants also cannot elevate the exchange-side permissions of the connected API
key. Reads require a key that the exchange already permits to read, and confirmed trade
actions require its trading permission; a host rejection remains authoritative when the
key lacks either permission. Configure the key outside the plugin according to the
official [API quick start](https://www.easicoin.io/api-doc/zh-CN/common/QuickStart), and never
put the key or secret in a manifest or guest module.

## Guest ABI

The module must have no imports and export:

```text
memory
alloc(length: i32) -> i32
run(context_ptr: i32, context_len: i32, input_ptr: i32, input_len: i32) -> i64
```

The host writes separate UTF-8 JSON buffers returned by `alloc`. `run` returns the
output pointer in the unsigned high 32 bits and output length in the low 32 bits. The
context limit is 65,536 bytes, user input must be a JSON object no larger than 4,096
bytes, and output is limited to 16,384 bytes. One decoded module is limited to 8,192
bytes and the entire manifest to 16 KiB. The host also applies a bounded memory, fuel,
and deadline budget and validates every allocation and range.

The snapshot contains only the currently granted sections. Treat all arrays as partial
point-in-time data: warnings and capture timestamps shown by the host are part of the
review surface, not proof that the exchange state is unchanged.

Some stable guest DTO fields are deliberately derived from authoritative upstream
fields because the public API does not return them directly:

- For balances, `frozen` is decimal `position_margin + order_margin`, and `total` is
  `equity`. See the official [wallet balance response](https://www.easicoin.io/api-doc/contract/accountHttp/get-wallet-list).
- The orders section is **ordinary active orders** fetched with `order_filter=Normal`;
  conditional and TP/SL orders are outside v5. `PendingCancel` may be represented as
  `Unknown` and cannot be selected for a cancellation proposal. The upstream response
  has cumulative execution quantity/value but no `avg_price`, so `avgPrice` is decimal
  `cum_exec_value / cum_exec_qty` when authoritative cumulative quantity is positive;
  it is zero only when that quantity is authoritatively zero. See the official
  [active-orders response](https://www.easicoin.io/api-doc/contract/orderHttp/open-order-list)
  and [enum definitions](https://www.easicoin.io/api-doc/contract/enumsDefinitions/enums-definitions).

These derivations preserve decimal strings and fail closed on malformed/missing source
values; they are not guest calculations. The guest output is a strict closed union:

```json
{"kind":"display","text":"Available balance: 12.5"}
```

```json
{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}
```

```json
{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"exchange-order-id"}}
```

Unknown fields, malformed/non-object input, ungranted data dependencies, stale identity,
and invalid proposal values fail closed. Guest strings are rendered as plain text, never
HTML. The host canonicalizes and revalidates proposals; author-provided JSON is not
authority.

For `placeOrder`, `positionIdx` is only `1` (long side) or `2` (short side). Opening
orders use Buy/1 or Sell/2; reduce-only orders use Sell/1 or Buy/2. Public
`timeInForce` accepts `GTC`, `IOC`, or `FOK`; the production host maps these to the
exchange's longer wire names. The example is an opening Buy and therefore uses
`positionIdx: 1`.

## Run and confirmation lifecycle

1. The host resolves an enabled published contribution and displays the connected
   account/environment and its session grants.
2. Run captures the authorized sections under the current account lifecycle, executes
   the guest, and validates its strict output. Run does not mutate the account.
3. A place/cancel result receives a short-lived host-owned confirmation token. The UI
   displays the exact immutable account and canonical action. The guest never sees the
   token or private authority.
4. A separate explicit confirmation consumes the token once before dispatch through
   the production risk, idempotency, and recovery path. The confirmation call accepts
   the token only; it cannot replace the captured order.
5. Accepted, rejected, and unknown are distinct terminal displays. Unknown is not
   success and never permits retry of the same token. Inspect the trading/recovery UI
   instead of resubmitting blindly.

Placement receives a generated submission identity. A cancellation can name only an
order captured in the authorized open-order snapshot. Pending confirmation is bounded,
expires quickly, and is invalidated by revocation or relevant lifecycle changes.

## Example

[Account workflow safety example](../examples/plugins/account-workflow/README.md)
contains readable WAT, a reproducible manifest, and a synthetic-data verifier. Its
place proposal is user-parameterized and its cancellation derives a captured order ID.
The example is instructional: inspect and edit every proposed value before confirming.

## Deliberate exclusions

The ABI has no background task, timer, auto-confirm, unattended strategy loop, custom
network access, arbitrary exchange endpoint, credential access, or direct provider SDK.
Supporting unattended strategies would require a separate policy, lifecycle, risk, and
recovery design; v5 deliberately does not provide it.
