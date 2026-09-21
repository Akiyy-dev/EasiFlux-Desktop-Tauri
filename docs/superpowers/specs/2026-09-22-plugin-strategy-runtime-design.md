# Native-supervised automatic strategy plugins

## Intent and authority

EasiFlux is a trading framework. A plugin implements strategy decisions, not a
sequence of user-confirmed order suggestions. The user configures and explicitly
starts a strategy once; it can then place and cancel ordinary orders automatically
within native-enforced authority. The user approved this architecture and delegated
design, implementation, review and suitable PR integration without intermediate
questions. This permission covers software development, NOT using real credentials,
running a real strategy, or spending real funds during verification.

Baseline: main `f1c3ec48a9df88f7f06c9b9630088dc2067acfe5` (PR #41). Work happens on
`plugin/strategy-runtime` in the existing isolated linked worktree. Root checkout
changes are unrelated and remain untouched. Existing CI and CodeQL are green.

## Chosen architecture and alternatives

Add manifest v6 `sandbox.strategy`, with native-owned background callbacks and a
separate `strategy-json-v1` ABI. Reuse the bounded import-free Wasm executor and
production workflow host. Keep v1-v5 behavior, hashes and v5 per-order confirmation
unchanged. Never simulate unattended trading by programmatically confirming v5 tokens.

A timer around v5 would reuse an unsuitable authorization model. An arbitrary native
or JavaScript plugin process would expand filesystem/network/credential authority and
does not fit this first version. Native supervision with short isolated callbacks is
the chosen extension: the guest never owns a long-running thread or exchange client.

The first version runs while the desktop process is alive and the computer is awake;
closing its panel does not stop it. It is not a Windows service, cloud runner, HFT
engine, or guarantee of work during sleep/shutdown. Restart requires explicit resume.

## Closed manifest and guest contract

- v6 contributions remain `kind: "command"`; new `actionId: "sandbox.strategy"`.
- Parameters are exactly `{runtime:"wasm-v1", abi:"strategy-json-v1",
  moduleBase64:string, defaultInput:string}`. The JSON input must be an object.
- v6 requires at least one strategy contribution and `account.read` plus `strategy.run`.
- Allowed capabilities are the seven v5 names plus `strategy.run`. Only v6 may request
  `strategy.run`. v5 parsing/grants reject it. All requested names are unique.
- v6 can include showInfo/openPage/compute commands but not accountWorkflow commands;
  v5 remains the explicit-confirmation format. No ambiguous untagged params parsing:
  discriminate the action before decoding or validate ABI during deserialization.
- Manifest limit remains 16 KiB, decoded module 8,192 bytes. Existing sandbox limits
  remain 1 MiB memory, 2,000,000 fuel, 2-second callback deadline, no imports/WASI/start.
- Exports remain `memory`, `alloc(i32)->i32`, `run(i32,i32,i32,i32)->i64` and the
  existing pointer/length encoding. Input JSON <=4,096 bytes; context <=65,536 bytes;
  guest output <=16,384 bytes. State is an object <=4,096 serialized UTF-8 bytes.

Context is `{schemaVersion:1, runId, sequence, event, snapshot, state, lastReceipt}`.
`sequence` is a canonical u64 decimal string; `event` is `start`, `timer`, or `update`.
`snapshot` uses the v5 sanitized granted-data DTO, including timestamps and partial
flags. `lastReceipt` is null or the receipt below. It reports acceptance, not fill.
Raw API responses, credentials/private session identity, paths, or host handles never
enter the guest. Native wakeups cause a new authoritative read, not payload forwarding.

Output is exactly `{state: object, action: Action, message: string}` (message <=2,000
UTF-8 bytes, displayed as plain text). `Action` is one of:

```json
{"kind":"none"}
{"kind":"stop"}
{"kind":"placeOrder","order":{"symbol":"BTCUSDT","side":"Buy","orderType":"Limit","qty":"0.001","price":"50000","timeInForce":"GTC","positionIdx":1,"reduceOnly":false}}
{"kind":"cancelOrder","order":{"symbol":"BTCUSDT","orderId":"owned-exchange-id"}}
```

One callback may request at most one mutation. Placement invariants match v5, including
Buy/1 and Sell/2 for opening, Sell/1 and Buy/2 for reduce-only, market price=null and
IOC, and GTC/IOC/FOK for limit. Quantity and price use checked Decimal strings.

## Explicit start authority and limits

Import, enable, access inspection, page rendering, and reconnect never start a run.
The user explicitly selects capabilities and confirms that this run can trade without
per-order prompts. Nonempty selection requires account.read and strategy.run.
Selected capabilities must be declared by the captured manifest. Cancelling additionally
requires orders.read; missing capabilities fail closed. The guest cannot amend grants.

`get_plugin_strategy_access` accepts the existing `AuthorityRequest` and returns a
short-lived one-use native ticket bound to plugin identity/content, catalog/revision,
runtime epoch, current account/session/environment and private installation UUID. A
credential replacement must invalidate the ticket even when session epoch is unchanged.
Ticket TTL is 60 seconds; at most 32 tickets, pruned on issuance. Tickets never persist.

Required start policy (exact camelCase wire fields):

```json
{"intervalMs":5000,"maxOrderQty":"0.001","maxTotalQty":"0.01","maxActions":20,"maxRunSeconds":3600,"reduceOnly":false}
```

intervalMs is integer 5,000..60,000; maxRunSeconds integer 60..86,400; maxActions
integer 1..1,000; quantities are positive bounded Decimal strings (<=64 bytes), and
maxOrderQty <= maxTotalQty. reduceOnly forces every placement to be reduce-only when
true. Policy is immutable for a run. All constraints hold even when global risk is off.
Quantity means the exchange order quantity unit, NOT a dollar amount. maxTotalQty is
conservative cumulative submitted quantity, including rejected/unknown submissions;
cancellation does not refund it. These are submission caps, not maximum-position,
notional, loss, or profit guarantees. Existing global trading risk applies in addition.
Only the selected symbol can be traded. Counter overflow or exhaustion ends admission.

One active run per account, one in-flight callback/action per run, max four workers
globally. Retain at most 32 run records; capacity fails visibly rather than deleting
recovery/ownership history. Explicit new starts cannot bypass unresolved earlier
strategy actions for the same account ID/environment, including after key rotation.

## Native supervisor and stop semantics

Native Tokio workers own the schedule. Timers are anchored to completed passes, skip
missed ticks (never catch up in a burst), and coalesce host-only Notify wakeups. Native
market/private updates may mark work dirty; callbacks still obey intervalMs. The
first callback is `start`; resume uses `update`. Fresh snapshots are fetched for each
pass. Do not hold plugin/account gates while sleeping or executing guest code.

Lock ordering follows plugin operation gate -> account lifecycle -> trading/risk.
Capture and then recheck plugin content, enabled status, epoch, native authority,
run generation, limits and cancellation immediately before every mutation. Reuse the
global compute slot; actual workers retain leases until they exit. Busy admission may
skip a pass, never run a second guest concurrently. A data read error, excessive read
duration (>30 seconds), authority change, invalid guest, or missed scheduling gap
>max(30 seconds, 3*intervalMs) pauses/faults admission rather than using stale data.
Reconnect does not restore trading authority automatically.

Pause/stop intent synchronously revokes run admission and signals guest cancellation
before waiting for any gate. A dispatched network future is never aborted as a way to
stop a run: an owned worker accounts for its receipt first. `stopping`/pause-pending
remains visible until it drains. Stop does not imply that exchange orders were cancelled
or positions closed. No implicit cancellation, liquidation or restart is authorized.
Emergency stop applies to every run even if account is disconnected or plugin removed.
The UI warns that outstanding exchange orders remain and displays the last receipt.
App exit requests revoke admission before scheduler shutdown. Drain owned workers within
the existing bounded shutdown path where possible; forceful termination leaves durable
pending intents requiring recovery and must never enable startup replay.

States on the wire: `running`, `paused`, `stopping`, `stopped`, `recoveryRequired`,
`faulted`, `completed`. A persistent pending intent or persistence/receipt/ack failure
must not be hidden by stopped. Resume is an explicit new authorization ticket for the
same content/account/symbol and unchanged policy; counters and state never reset.
Expired runs cannot resume; authorization renewal is not an implicit budget reset.

## Durability, ownership, and reconciliation

Persist run configuration, content fingerprint, durable native account scope, sequence,
guest state, counts, owned order IDs, last receipt, and any pending canonical action.
Persist no API secret, API key, private installation UUID or executable authority.
Use a new native-only injectable StrategyStore and a dedicated app-owned fixed path;
tests use workspace-owned temporary paths. No arbitrary path comes from IPC or guest.

The file snapshot is bounded to 16 MiB with monotonic revision. Atomic writes fsync the
new candidate before publishing. Load examines main/temp/backup, validates every
present candidate, and chooses the highest revision, never silently downgrading to an
older state. Any malformed candidate, contradictory same revision, symlink/reparse
target, invalid record, or ambiguous intent makes strategy trading unavailable. Do not
reuse config_persistence::load's first-valid fallback semantics for trading authority.
Successful saves may retain a validated lower-revision backup. Storage failure revokes
admission and cannot be bypassed by a new run. Serialize persistence across workers.

Before HTTP, debit action/quantity budget and persist a pending action with host UUID
submission ID and the proposed next guest state. After response, persist receipt,
ownership, guest state and counters. For accepted placements, acknowledge the existing
OrderSubmissionStore receipt ONLY after the strategy write succeeds. If submit_once's
accepted receipt was not itself persisted, reconcile the original submission before
acknowledgment. No next callback until both records agree. Unknown is never retryable.
Definitely rejected actions may complete with a rejected receipt, still consuming caps.

Cancellation is permitted only for an order this run demonstrably placed, with matching
scope/symbol/exchange ID, and present as cancellable in the current authorized snapshot.
Record cancel intent before HTTP too. An accepted cancel marks cancellation requested,
not terminal cancellation; block duplicate cancellation of that owned order. A partial
snapshot's absence never proves an order filled/cancelled. Ownership survives stop.

Restart loads previously active runs as paused without live authority; pending/unknown
actions become recoveryRequired. Never replay an intent. `reconcile_plugin_strategy`
is read-only at the exchange: query the original placement identity or cancellation
order through existing exact-match query helpers. Not found is still unresolved.
An authoritative placement rejection or exact accepted order resolves pending placement;
cancel recovery requires a terminal authoritative order state. Persist resolution and
acknowledgment before becoming paused. Reconciliation does NOT restart the strategy.
Resume requires explicit authorization and compares durable scope plus content identity.

## IPC and public DTOs (all exact, no unknown fields)

All new IPC is local-main-WebView-only. Keep existing plugin/remote/filesystem/shell ACL
boundaries intact and update exact security inventories, including the smoke host where
appropriate. Native events are not trusted frontend events and grant no authority.

Commands:

1. `get_plugin_strategy_access({request: AuthorityRequest}) -> StrategyAccess`.
2. `start_plugin_strategy({request: StrategyStartRequest}) -> StrategyRunView`.
3. `list_plugin_strategies() -> {schemaVersion:1,runs:StrategyRunView[]}`.
4. `control_plugin_strategy({request:{runId:string,action:"pause"|"stop"}}) -> StrategyRunView`.
5. `stop_all_plugin_strategies() -> {schemaVersion:1,runs:StrategyRunView[]}`.
6. `reconcile_plugin_strategy({runId:string}) -> StrategyRunView`.

StrategyAccess exact fields:
`schemaVersion:1, pluginId, contributionId, catalogGeneration, revision, account,
requestedCapabilities:string[], authorizationToken:string, expiresAtMs:string`.
`account` uses v5 `{accountId,sessionEpoch,environment}`. No live granted authority is
restored merely by obtaining access. Fresh tickets only come from enabled v6 commands.

StrategyStartRequest exact fields:
`authorizationToken:string, requestId:UUID, resumeRunId:string|null, symbol:string,
inputJson:string, capabilities:string[], policy:StrategyPolicy, acknowledgeAutomaticTrading:true`.
On resume, input, policy, symbol, caps and identity must match the stored run. A start
request is owned independently of the caller; losing IPC cannot cause a duplicate run.
List/status recovery, not blind start retry, handles an uncertain UI result.

StrategyRunView exact fields:
`schemaVersion:1, runId:UUID, requestId:UUID, pluginId, contributionId, account,
symbol, status, reason:string|null, policy, capabilities:string[], inputJson:string,
startedAtMs:string, expiresAtMs:string, sequence:string, actionsSubmitted:number,
totalSubmittedQty:string, lastMessage:string, lastReceipt:StrategyReceipt|null`.
No persisted private scope, authority, raw snapshots, guest state, or raw error text in
the public view. Listing is capped to 32 records. Sequences/timestamps are canonical
u64 strings, quantities Decimal strings. Last-message is bounded plain guest text.

StrategyReceipt exact fields:
`sequence:string, kind:"placeOrder"|"cancelOrder", status:"accepted"|"rejected"|"unknown",
submissionId:string|null, orderId:string|null, errorCode:string|null`.
Only closed sanitized error codes are exposed. Accepted never means filled or profit.

## User interface

Add a strategy launch dialog reachable from cards and command workbench. It shows the
plugin, connected account/environment, declared capabilities (all initially unchecked),
symbol, strategy JSON parameters, mandatory policy fields, and an initially unchecked
real automatic-trading acknowledgment. Launch button explicitly says autonomous real
trading. Show confirmation once for the run, never a per-order prompt.

Provide a persistent strategy monitor in the plugin page with status, account/symbol,
counters/budget, expiry, last message/receipt and pause/stop/reconcile/resume controls,
plus emergency stop all. Monitoring stays usable after the originating plugin is disabled
or removed, account disconnected or the dialog closed. A render/reload never starts or
resumes. Only explicit clicks mutate run state. Show Chinese explanatory copy for limits,
sleep/exit behavior, orders remaining after stop, restart authorization and unknown results.
Do not claim a stop succeeded after a network/IPC error; retain unknown state and refresh.
Strictly validate IPC and ownership/correlation of responses. Ignore stale async results
after account/catalog/intent changes. Do not send any data to external services.

## Example and verification

Ship an importable v6 threshold-once example with readable WAT, reproducible generator
and synthetic verifier. User-configured price threshold decides when to emit the user's
limit-order template; subsequent callbacks use saved state and the native receipt to
cancel that owned order once when visible, then stop. It must demonstrably inspect price,
state and receipt, not return a fixed canned output. It is an execution example, not a
profitable strategy recommendation. Import/enable must never execute it.

Acceptance: launch once against injected host, then actual native loop executes guest
callbacks, reads changing snapshots, automatically places and cancels exactly once with
no per-order UI call; journal and ack ordering verified. Pause/stop/grant invalidation,
plugin disable/reload, credential replacement, stale data, budget rejection, unknown
HTTP result, damaged storage and reconstructed supervisor must stop further mutation.
Frontend tests exercise rendered launch/monitor behavior and exact service boundaries.
Focused local tests while iterating; broad CI/build/security check once at final head.
No production AppState launch or live account/keychain/API calls in acceptance tests.

Out of this first version: strategy-origin arbitrary networking/filesystem, withdrawals,
transfers, credential export, conditional orders, TP/SL, multi-account concurrency,
automatic restart authorization, full historical data/indicator SDK, hosted marketplace,
and installer/release publication. These limits must appear in user-facing documentation.
