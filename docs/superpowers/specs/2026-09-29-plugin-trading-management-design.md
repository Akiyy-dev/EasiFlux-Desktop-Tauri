# Plugin trading management: owned amendment and protected entry

## Intent and scope

The user wants EasiFlux to host useful autonomous trading robots, not merely
display plugins. They explicitly delegated research, design, implementation,
review, and suitable PR integration without routine questions. This architectural
increment implements two missing order-management operations, not complete account
takeover. No real account, credential, exchange mutation, or installed-app profile
is used during development or verification.

Base: remote main `8f81ed3832415269b879ff411eaa979e1254e832`; work in the existing
isolated checkout on `plugin/trading-management`. Preserve the original checkout.

## Approach

Three options were considered: expose raw exchange commands (insufficient native
authorization/recovery); implement every position/account endpoint at once (too
many distinct recovery and risk contracts); extend the existing supervised order
pipeline (selected). Keep the existing one-action callback, one-symbol authority,
durable intent, immutable policy, final native admission guard, and explicit resume.

This increment adds owned normal limit-order amendment and native TP/SL attached
to a new opening order. It excludes position-level create/replace TP/SL, leverage,
margin/mode changes, multi-symbol operation, arbitrary REST, transfers, withdrawals,
automatic restart, and handling another strategy's orders.

## Version and permission contract

- Manifest v7 uses the existing `sandbox.strategy` / `strategy-json-v1` ABI.
- v1-v6 retain their previous capability sets and semantics. v7 alone may declare
  `trade.amend` and `trade.protect`; import/update/enable never grants either.
- `trade.amend` requires `trade.place`, `orders.read`, and `market.read`.
- `trade.protect` requires `trade.place` and `market.read`.
- Both manifest declarations and each start/resume grant enforce dependencies.
- Existing catalog transport version 3 and strategy IPC schema 1 remain unchanged.
  Strict parsers add only the explicitly enumerated new capabilities/receipt kinds.
- Existing caps remain: interval 5000-60000 ms, lifetime 60-86400 seconds, 1-1000
  actions, mandatory positive maxOrderQty/maxTotalQty, maxOrderQty <= maxTotalQty.

## New guest actions

Existing `none`, `stop`, `placeOrder`, and `cancelOrder` remain unchanged.

```json
{"kind":"amendOrder","order":{"symbol":"BTCUSDT","orderId":"owned-id","price":"50010","qty":"0.001"}}
```

Both new price and total order quantity are required positive bounded decimal
strings. The target must be an owned, currently visible, ordinary Limit order in
New/PartiallyFilled state, with no prior cancellation request. The exact client
order identity must match this run's owned submission identity. Reject duplicate
or conflicting target rows, another symbol, no-op updates, quantity increases,
and qty <= already-filled quantity. New quantity must also satisfy maxOrderQty.
Direction, position index, reduceOnly, timeInForce, conditional type, and protection
are not editable through this operation.

Each amendment consumes one action and its full requested quantity from the
conservative cumulative quantity budget, even on rejection/unknown. This intentionally
overcounts possible volume, but cannot bypass limits through repeated amendments.
The existing native risk validator/reservation is applied to the reconstructed
canonical target with new price/qty; global risk being disabled does not disable
per-run limits. The production adapter obtains authoritative immutable target
metadata instead of trusting guest-provided side/reduceOnly values. It must check
fresh target data and a fresh risk quote before final admission. The exchange can
still fill/cancel the order concurrently; never emulate amendment with cancel/place.
Persist each newly owned order's original canonical placement as optional native
ownership metadata. Legacy records without it remain readable but cannot amend.
Use that immutable identity to check live raw targets and later reconciliation;
requested price/qty may evolve without changing original direction, reduceOnly,
position index, client ID, or attached protection. Do not derive these from guest
input or infer them from a missing raw field.

```json
{"kind":"placeProtectedOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false},"protection":{"takeProfit":"55000","stopLoss":"45000","triggerBy":"LastPrice"}}
```

`protection` is a strict object: both nullable price fields are required; at least
one is non-null and every non-null value is a positive bounded decimal string.
`triggerBy` is exactly LastPrice or MarkPrice and maps to each specified leg's
exchange trigger selector. Opening orders only: reduceOnly must be false; a
reduce-only run cannot use this action. Relative to the fresh selected trigger
quote, long TP > reference > SL, short TP < reference < SL. For limit entries the
same directional relation must also hold relative to entry price. Reject equality,
missing/invalid quote, duplicate fields and unknown fields. The host validates
again after awaited preparation before the final admission hand-off.

One protected placement consumes the same budgets and UUID client submission
identity as an ordinary placement; it is one exchange create-order request, not
an unprotected order followed by a second protection request.

## Native adapter, receipts, durability and recovery

Use existing EasiCoin endpoints; do not add dependencies or arbitrary HTTP access.
Native request representation carries optional protection through canonicalization,
the order-submission journal, serialization, API payload mapping and identity
checks. Ordinary/v5 requests serialize as before when protection is absent.
Keep guest workflow v5 DTO unchanged. Unknown optional native protection fields
must not be silently accepted as a valid protected canonical request.

Receipt kinds add `amendOrder` and `placeProtectedOrder`. Protected placement has
a submissionId like placeOrder; amendment has an orderId and no submissionId.
Accepted means exchange request acknowledgement (or explicitly reconciled desired
state), not filled, final amendment, active TP/SL, or guaranteed protection.
UI/docs must state this clearly. A matching acknowledgement must be structurally
valid and identify the exact order; HTTP success, empty data and wrong/conflicting
IDs are unknown, never fabricated acceptance.

Pending intent and conservative debit precede mutation dispatch. Persist receipts
and ownership before acknowledging a placement's submission journal. Preserve the
existing synchronous final admission check after all asynchronous preparation;
revocation before hand-off sends zero mutations, and already-admitted calls drain.
Never automatically retry unknown mutations. Unknown receipts require a durable
matching action, sequence, submission/order identity and sufficient budget debit.
Read old strategy documents without new optional authority; old plugins cannot gain
new operations during restart or resume.

Amendment reconciliation is read-only by exact exchange ID and owned client ID.
It resolves only when a strict fresh order observation proves the requested
price and total quantity, with unchanged immutable identity; missing, conflicting,
or old values stay unresolved. This proves desired state, not causation. Do not
reuse cancellation's terminal-status rule for amendment. A normal matching live
amend acknowledgement need not contain final price/qty, but cannot be described
as final state. Add a narrow typed acknowledgement instead of pretending raw data
is a fully observed order where needed.

Protected placement reconciliation by submission ID validates raw requested TP/SL
and trigger fields as well as original order identity before resolving a previously
unknown submission. Never infer protection from the normalized Order DTO (it lacks
these fields). A known matching live create-order acknowledgement confirms receipt
of the canonical protected request, not activation of its protection. Strict journal
canonical comparison prevents resolving a protected request as an ordinary one.

## UI and authoring

Recognize v7 across catalog/import/update/diff/launch flows. Show explicit Chinese
permission descriptions for amendment and attached protection, unchecked initially.
Enforce dependency selection, including during resume. The monitor displays the
new receipt kinds without claiming fills/protection activation. No new implicit
start, account switching, or privilege inheritance.

Provide a v7 example with readable WAT and reproducible checked-in manifest. It
uses a protected opening order, then amends its exact acknowledged visible limit
order, then stops. A synthetic execution verifier and real native Wasmi/supervisor
acceptance must exercise the packaged bytes. Keep the example under existing
8192-byte module/16-KiB manifest limits and retain the v6 threshold example.

## Verification and handoff

Use focused fail-first behavioral tests for the new decisions; consolidate broad
tests at final integration rather than repeating full suites after each edit.
Cover unauthorized/version mismatch, duplicate fields/IDs, quantity increase and
filled races, stale/stop during preparation, actual payloads, malformed ack,
unknown/restart reconciliation, canonical protected identity and old v6 behavior.
Frontend tests exercise strict parser, unchecked grants/dependencies, new receipt
labels and immutable resume. Tests use injected hosts/temp journals/mocked IPC only.

Perform independent task reviews and a final whole-branch review, then CI before
merging a suitable PR under the user's standing authorization. Report remaining
live/test-environment acceptance and unsupported position/account operations;
do not call this full trading takeover or production acceptance.

## Primary API references checked 2026-09-29

- [Create order](https://www.easicoin.io/api-doc/contract/orderHttp/order-create)
- [Replace order](https://www.easicoin.io/api-doc/contract/orderHttp/order-replace)
- [Create position TP/SL](https://www.easicoin.io/api-doc/contract/positionHttp/set-tpsl)
- [Position list](https://www.easicoin.io/api-doc/contract/positionHttp/list)

Only public documentation was read; no authenticated API request was made.
