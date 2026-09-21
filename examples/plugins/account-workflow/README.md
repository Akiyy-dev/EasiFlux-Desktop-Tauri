# Account workflow executable example

This v5 example contains three small, import-free WebAssembly guests:

- `balance.wat` reads the first captured balance entry's granted `available` value.
  It does not select USDT or aggregate assets; check the asset in the host snapshot.
- `place.wat` returns the user's strict `placeOrder` JSON proposal.
- `cancel.wat` derives a `cancelOrder` proposal from the first order captured by the host.

The manifest contains no API keys, endpoints, account IDs, or account data. Importing,
enabling, granting capabilities, and running a guest do not place or cancel an order.
For every proposed mutation, EasiFlux displays an immutable summary and requires a
separate, explicit confirmation labelled as a real order or real cancellation.

## Verify or rebuild

From the repository root, with the locked Rust dependencies already available:

```powershell
node examples/plugins/account-workflow/build.mjs --check
node examples/plugins/account-workflow/verify.mjs
node examples/plugins/account-workflow/build.mjs --write
```

`--check` proves that each checked-in Base64 module was compiled from its matching
WAT source. `verify.mjs` instantiates the packaged bytes only with synthetic context
data and checks that the balance, place, and cancel results depend on their inputs.
The fixed-path generator accepts no caller-selected source path. It does not connect
to an account or submit anything.

## Try it deliberately

1. Read all three WAT files and import `manifest.json` in a build that supports v5.
   The plugin remains disabled and has no grants after import.
2. Enable it, open a workflow, verify the displayed connected account/environment,
   and grant only the requested session capabilities you intend to use.
3. Run **Show granted available balance** to see a value derived from the captured
   balance snapshot.
4. For **Prepare example limit order**, replace the example quantity, price, and
   other fields with the intended values before Run. The opening Buy uses long-side
   `positionIdx: 1`; do not change side/reduce-only/index independently. Running only
   prepares a proposal.
5. Read the host's canonical order summary. Submit only with the separate explicit
   real-order confirmation. A new proposal always needs a new confirmation.
6. The cancellation example uses an order ID captured from the open-order snapshot.
   Again, Run prepares; only the separate real-cancellation confirmation mutates.
7. Revoke the grants when finished. Grants are session-only and are not embedded in
   the manifest.

If a confirmed operation reports `unknown`, do not confirm or submit it again. Inspect
the existing trading and recovery views to determine the exchange outcome. A prepared
token is short-lived, single-use, and invalidated by account, grant, plugin-content,
or lifecycle changes.

See [the complete authoring contract](../../../docs/plugin-account-workflow.md).
