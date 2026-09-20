# Plugin account and trading workflows

## Outcome and scope

An enabled local plugin can obtain explicitly granted current-account balances, positions, open orders and a fresh symbol quote, execute its own import-free Wasm against that data, and propose a real order or cancellation. The trusted application displays the exact proposal and account, then submits through the existing production trading/risk/recovery pipeline only after a separate explicit confirmation. This is an executable account workflow, not navigation to the trading page.

The user has delegated implementation and suitable PR integration. Development never opens real credentials/profiles or sends real orders. Automated validation uses an injected host and temporary storage. This release does not add unattended strategies, background schedules, transfers, withdrawal, arbitrary network access, raw IPC, cancel-all, leverage changes, or guest access to credentials.

## Choices

- Use manifest v5 and an import-free JSON Wasm ABI, extending the existing bounded executor. General JavaScript/WASI/raw Tauri bridges are unnecessary authority.
- Grants are explicit, independently selectable, memory/session-only, and bound to plugin content, runtime epoch, account session and private credential authority. Enabling/importing never grants capabilities. Restart, account/session/credential change, plugin lifecycle mutation, or explicit revocation requires renewed authorization.
- Trading permission allows preparing proposals, not bypassing confirmation. Every order/cancel has a host-owned single-use token with a 60-second monotonic expiry. The confirmation IPC accepts only that token.
- Host admission is serialized against plugin lifecycle/grant mutation, then against account mutation. Recheck all authority after acquiring both guards. Revocation cannot undo an operation already admitted/transmitted; the UI must say so.
- No automatic retry after a mutation timeout or unknown outcome. Existing durable order submission recovery remains authoritative for placement. Cancellation ambiguity is reported as unknown and requires checking the trading page.

## Binding constraints

- Preserve v1-v4 behavior, exact canonical fingerprints and the catalog transport schema (3).
- Manifest/file limit remains 16 KiB; each canonical base64 Wasm module remains at most 8192 decoded bytes.
- Runtime stays import-free/no-start/no-WASI/no network, 1 MiB memory, 2,000,000 total fuel, 10,000-fuel slices and a 2-second cooperative deadline. Share the existing process-wide actual-worker-owned slot with v4; lifecycle invalidates late results.
- JSON ABI input context <= 65,536 UTF-8 bytes, user input <= 4096 UTF-8 bytes (JSON object), guest output <= 16,384 UTF-8 bytes. Output is a strict closed object union. Reject duplicate/unknown fields in authority-bearing DTOs, arrays in place of objects, malformed UTF-8, noncanonical counters, forged identities and out-of-bounds guest pointers.
- Decimal financial values remain strings. Never derive account totals with f64. No API key, secret, credential digest, raw exchange payload, diagnostics, local path or arbitrary API URL is exposed to the guest.
- Fail closed on unavailable/stale account data; section reads are not an atomic exchange snapshot. Timestamps describe host retrieval, not an invented exchange timestamp. Lists are bounded at 100 entries and marked partial because current endpoints do not establish complete pagination.
- Orders always pass invariant validation even when configurable risk is disabled, then use existing TradingService and durable submit_once. Plugin code cannot select its own orderLinkId.

## Manifest and Wasm contract

Manifest v5 contributions may contain the previous three actions plus `sandbox.accountWorkflow`. V4 must still exclude the new action. A v5 manifest must contain at least one workflow and request `account.read`. `requestedCapabilities` is a unique closed list drawn from:

`account.read`, `balances.read`, `positions.read`, `orders.read`, `market.read`, `trade.place`, `trade.cancel`.

Workflow params are exactly:

```json
{"runtime":"wasm-v1","abi":"account-json-v1","moduleBase64":"<canonical module>","defaultInput":"{}"}
```

`defaultInput` is a JSON object encoded as text, at most 4096 UTF-8 bytes. Capability lists in the catalog are requests, never inferred grants; session grants are returned by a separate access IPC.

Exports: `memory`, `alloc(length: i32) -> i32`, `run(context_ptr: i32, context_len: i32, input_ptr: i32, input_len: i32) -> i64`. The unsigned high 32 bits of `run` are output pointer, low 32 bits output byte length. Host writes separate UTF-8 context and input buffers, validates both allocations and result range with checked arithmetic, and applies the shared total budget to alloc and run. Guest result remains data; it cannot call host functions.

## IPC and wire contract (camelCase)

All counters/timestamps below are canonical nonnegative decimal strings; IDs are validated using existing plugin rules. `AuthorityRequest` has exactly `{pluginId, contributionId, expectedCatalogGeneration, expectedRevision}`. `Account` is `{accountId, sessionEpoch, environment}`; environment is a sanitized origin/display label, never the raw configured URL. Internal credential identity is NOT serialized.

1. `get_plugin_workflow_access({request: AuthorityRequest})` returns `Access`.
2. `set_plugin_workflow_grants({request: {...AuthorityRequest, expectedAccountId, expectedSessionEpoch, expectedGrantRevision, capabilities}})` returns `Access`. Subsets (including empty) are supported; nonempty sets must contain `account.read`. Grant updates invalidate all prepared tokens and in-flight results for that plugin. Deny unknown/unrequested/duplicate capabilities and stale account/grant revisions.
3. `run_plugin_workflow({request: {...AuthorityRequest, requestId, expectedAccountId, expectedSessionEpoch, expectedGrantRevision, symbol, inputJson}})` returns `WorkflowResult`.
4. `confirm_plugin_workflow({token})` returns `TradeReceipt`. The backend owns and revalidates the stored request; no caller-supplied order can be confirmed.
5. `cancel_plugin_compute({requestId})` also cancels a workflow's bounded Wasm computation (not a previously admitted trade).

`Access` is exactly `{schemaVersion:1, pluginId, contributionId, catalogGeneration, revision, account:Account, requestedCapabilities:Capability[], grantedCapabilities:Capability[], grantRevision}`. Access is available only for a published enabled workflow and a connected account. Native rechecks accountId, epoch, private authority and endpoint under AccountLifecycleCoordinator; frontend checks correlation but is not the authority.

`WorkflowResult` is exactly `{schemaVersion:1, requestId, pluginId, contributionId, catalogGeneration, revision, account:Account, grantRevision, snapshot:Snapshot, output:Output, confirmation:Confirmation|null}`.

`Snapshot` is exactly `{schemaVersion:1, account:Account, capturedAtMs, symbol, grantedCapabilities:Capability[], balances:Section<Balance>|null, positions:Section<Position>|null, orders:Section<Order>|null, market:MarketSection|null}`. Unauthorized sections are null and must not be queried. `Section<T>` is `{items:T[], fetchedAtMs, partial:boolean}`. Use the existing sanitized Balance, Position and Order DTO fields; select/copy only these fields. `MarketSection` is `{ticker:{symbol,lastPrice,bidPrice,askPrice,markPrice}, fetchedAtMs}`. Symbol is 1..32 ASCII uppercase letters/digits, and every position/order/quote must match the requested symbol. Keep decimal strings validated and bounded; no raw ticker diagnostic fields. `capturedAtMs` is completion time, each fetchedAtMs is recorded immediately after that read. Do not label data as currently fresh after it ages.

`Output` is one of these exact shapes:

```json
{"kind":"display","text":"text, at most 2000 UTF-8 bytes"}
{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}
{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"exchange-order-id"}}
```

Place: positive non-exponent Decimal strings (max 64 bytes), canonicalized before preview; side Buy/Sell; type Market/Limit; Limit requires positive price and GTC/IOC/FOK; Market requires null price and IOC; positionIdx is 1/2; reduceOnly boolean. Opening Buy/Sell uses index 1/2 respectively; reduce-only Sell/Buy uses index 1/2 respectively. Reject inconsistent side/index/reduction combinations. Symbol must equal the snapshot symbol. Unknown order fields (including orderLinkId) reject. Cancel: requires both `trade.cancel` and `orders.read`, a nonblank bounded orderId (<=128 bytes) actually present in the captured open-order section for that symbol, with New/PartiallyFilled status. Place requires `trade.place`. Neither mutation happens during run.

### Official production API mapping

Checked against the public [wallet balance](https://www.easicoin.io/api-doc/contract/accountHttp/get-wallet-list), [active orders](https://www.easicoin.io/api-doc/contract/orderHttp/open-order-list), [ticker](https://www.easicoin.io/api-doc/contract/marketHttp/symbol-ticker), [positions](https://www.easicoin.io/api-doc/contract/positionHttp/list), and [create order](https://www.easicoin.io/api-doc/contract/orderHttp/order-create) documentation; no authenticated API call was made. Do not assume legacy forgiving DTO parsers match actual response fields.

- Balance: map `coin`, `available_balance` and `equity`. If an explicit frozen field is absent, derive `frozen` as checked Decimal `position_margin + order_margin` (occupied/reserved margin); require both source values. This is not an invented zero or a cross-asset sum.
- Open orders: request `order_filter=Normal` (ordinary active orders, not conditional/TPSL). Accept explicit validated average price when provided. Otherwise use checked Decimal `cum_exec_value / cum_exec_qty` when filled quantity is positive; normalize to the host Decimal precision. An authoritative zero fill quantity has average-price sentinel `"0"`; missing/malformed fill information is not zero. Document this derived field. The documented PendingCancel state maps honestly to Unknown and is not cancellable; a transitional row must not make unrelated valid orders unreadable.
- Preserve public proposal TIF GTC/IOC/FOK; the production adapter maps to the documented exchange spellings GoodTillCancel/ImmediateOrCancel/FillOrKill before the existing submission pipeline. Position indexes are explicitly 1-long/2-short; do not send the legacy UI's undocumented default 0 from plugins.
- Tests use synthetic values in the documented envelopes and reject missing required inputs instead of using personal account data or raw captured exchange responses.

`Confirmation` is exactly `{token, expiresAtMs, submissionId:string|null}`. Native stores immutable canonical output, captured content/counters/runtime epoch, account and private authority, grant revision, monotonic deadline and a generated UUID submissionId for placement. Limit pending confirmations to 32 with expired entries removed; no silent unbounded growth. ExpiresAtMs is for display only. Consume exactly once before external mutation; repeated/expired/stale tokens cannot send.

`TradeReceipt` is exactly `{schemaVersion:1, token, account:Account, action:"placeOrder"|"cancelOrder", status:"accepted"|"rejected"|"unknown", submissionId:string|null, order:Order|null, errorCode:string|null}`. Accepted requires a meaningful exchange order identity. Fixed workflow errors use prefix `plugin_workflow_` with safe messages; never propagate raw exchange/guest errors. Unknown is not success and must not enable retry of the same token. A dropped IPC must not release admission or cancel a dispatched mutation midway; let an owned native task finish and retain existing durable placement recovery.

## UI

Workflow commands are available in both plugin cards and the command workbench. A host-rendered inline dialog loads Access without querying private sections; lists requested capabilities individually with readable explanations and all grants initially unchecked; shows account/environment, grant/revoke actions and session-only lifetime. The user chooses a symbol and edits plugin input as bounded JSON (provide clear example/default). Run displays timestamps, partial warnings and authorized account data plus the plugin's plain-text result. Never render guest HTML.

For order/cancel, show an immutable confirmation summary containing plugin identity, account/environment, exact canonical side/type/qty/price/TIF/positionIdx/reduceOnly or cancellation ID, expiry and submission identity. The explicit action label must say real order/cancel; no implicit submit on running or granting. During confirmation block duplicate clicks; afterward display accepted/rejected/unknown distinctly and guide unknown outcomes to the trading page. Close/context change/account event invalidates local views and late responses; backend remains authoritative. Import/update/removal/marketplace copy must accurately explain v5 permissions without claiming old plugins can trade.

## Acceptance

Focused tests prove manifest backward compatibility, JSON ABI real execution and hostile output rejection, no unauthorized section fetch, grant/revoke/account/content freshness, no mutation before confirmation, exact one-shot order and cancellation dispatch, disabled-risk invariant enforcement, and unknown-result no retry. Vue tests exercise the real dialog/service/store routes. Add an importable v5 example whose guest reads account data and whose order/cancel command outputs are accepted by the broker. Isolated native smoke proves actual Vue -> Tauri ACL -> broker -> injected host order/cancel flow without loading AppState or real credentials. Run typecheck/build and final CI once rather than repeatedly running every suite locally.
