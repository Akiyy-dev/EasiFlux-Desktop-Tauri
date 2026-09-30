import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'

const manifestUrl = new URL('./manifest.json', import.meta.url)
assert.ok(existsSync(manifestUrl), 'managed-entry packaged manifest must exist')
const bytes = readFileSync(manifestUrl)
const manifest = JSON.parse(bytes)
assert.equal(manifest.schemaVersion, 7)
assert.deepEqual(manifest.requestedCapabilities, [
  'account.read', 'orders.read', 'market.read', 'trade.place', 'trade.protect', 'trade.amend', 'strategy.run',
])
assert.ok(bytes.length <= 16384)
const wasm = Buffer.from(manifest.contributions[0].params.moduleBase64, 'base64')
assert.ok(wasm.length > 0 && wasm.length <= 8192)
const module = new WebAssembly.Module(wasm)
assert.deepEqual(WebAssembly.Module.imports(module), [])
let cursor = 8
const leb = () => {
  let result = 0, shift = 0, byte
  do { byte = wasm[cursor++]; result |= (byte & 127) << shift; shift += 7 } while (byte & 128)
  return result
}
let segments = 0
while (cursor < wasm.length) {
  const id = wasm[cursor++], size = leb(), end = cursor + size
  if (id === 11) segments = leb()
  // MVP-only source; actual native Wasmi validation is also exercised in Rust.
  if (id === 10) assert.equal(wasm.subarray(cursor, end).includes(Buffer.from([0xfc, 0x0a])), false)
  cursor = end
}
assert.ok(segments <= 32)
const input = { symbol: 'BTCUSDT', qty: '0.001', entryPrice: '50000', amendPrice: '50010', takeProfit: '55000', stopLoss: '45000' }
const submission = '00000000-0000-4000-8000-000000000004'
const receipt = { sequence: '1', kind: 'placeProtectedOrder', status: 'accepted', submissionId: submission, orderId: 'owned-1', errorCode: null }
const order = { orderId: 'owned-1', symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', price: '50000', qty: '0.001', status: 'New', orderLinkId: submission, filledQty: '0', avgPrice: '0' }
function context(state = {}, lastReceipt = null, orders = [], sequence = '1', event = sequence === '1' ? 'start' : 'timer') {
  return {
    schemaVersion: 1, runId: '00000000-0000-4000-8000-000000000003', sequence, event,
    snapshot: {
      schemaVersion: 1, account: { accountId: 'synthetic-account', sessionEpoch: '7', environment: 'Injected test environment' },
      capturedAtMs: '1790035200000', symbol: 'BTCUSDT', grantedCapabilities: manifest.requestedCapabilities,
      balances: null, positions: null,
      orders: { items: orders, fetchedAtMs: '1790035199900', partial: true },
      market: { ticker: { symbol: 'BTCUSDT', lastPrice: '50000', bidPrice: '49999', askPrice: '50001', markPrice: '50000' }, fetchedAtMs: '1790035199950' },
    }, state, lastReceipt,
  }
}
function execute(ctx, parameters = input) {
  const { exports: { memory, alloc, run } } = new WebAssembly.Instance(module)
  const encode = value => Buffer.from(typeof value === 'string' ? value : JSON.stringify(value))
  const c = encode(ctx), i = encode(parameters)
  const cp = alloc(c.length), ip = alloc(i.length)
  new Uint8Array(memory.buffer, cp, c.length).set(c)
  new Uint8Array(memory.buffer, ip, i.length).set(i)
  const packed = run(cp, c.length, ip, i.length)
  const ptr = Number(packed >> 32n), length = Number(packed & 0xffffffffn)
  assert.ok(length <= 16384)
  const output = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(new Uint8Array(memory.buffer, ptr, length)))
  assert.ok(Buffer.byteLength(JSON.stringify(output.state)) <= 4096)
  return output
}
const normalize = state => Object.fromEntries(Object.entries(state).sort(([a], [b]) => a.localeCompare(b)))
let checks = 0
function check(name, body) { body(); checks++; console.log('PASS ' + name) }
check('input-dependent protected entry', () => {
  assert.deepEqual(execute(context()).action, {
    kind: 'placeProtectedOrder',
    order: { symbol: 'BTCUSDT', side: 'Buy', orderType: 'Limit', qty: '0.001', price: '50000', timeInForce: 'GTC', positionIdx: 1, reduceOnly: false },
    protection: { takeProfit: '55000', stopLoss: '45000', triggerBy: 'LastPrice' },
  })
  const other = execute(context(), { ...input, symbol: 'ETHUSDT', qty: '0.002', entryPrice: '2000', amendPrice: '2010', takeProfit: '2500', stopLoss: '1500' })
  assert.equal(other.action.order.symbol, 'ETHUSDT')
  assert.equal(other.action.order.qty, '0.002')
  assert.equal(other.action.order.price, '2000')
  assert.equal(other.action.protection.takeProfit, '2500')
})
const placed = execute(context()).state
check('empty state outside initial start cannot submit', () => {
  for (const ctx of [context({}, null, [], '2'), context({}, null, [], '1', 'timer')])
    assert.ok(['none', 'stop'].includes(execute(ctx).action.kind))
})
check('missing receipt cannot repeat placement', () => {
  assert.equal(execute(context(normalize(placed), null, [], '2')).action.kind, 'none')
})
check('stale/wrong-kind receipts cannot authorize management', () => {
  for (const patch of [{ sequence: '99' }, { kind: 'placeOrder' }])
    assert.equal(execute(context(placed, { ...receipt, ...patch }, [order], '2')).action.kind, 'none')
})
check('rejected/unknown placement stops without retry', () => {
  for (const status of ['rejected', 'unknown'])
    assert.equal(execute(context(placed, { ...receipt, status, orderId: null }, [], '2')).action.kind, 'stop')
})
const awaiting = execute(context(placed, receipt, [], '2')).state
check('accepted acknowledgement waits for visible exact ownership', () => {
  for (const orders of [[], [{ ...order, orderId: 'foreign' }], [{ ...order, orderLinkId: 'foreign' }], [{ ...order, symbol: 'ETHUSDT' }], [{ ...order, status: 'Filled' }], [{ ...order, orderType: 'Market' }], [{ ...order, side: 'Sell' }], [{ ...order, qty: '0.0005' }], [{ ...order, filledQty: '0.001' }], [order, order]])
    assert.equal(execute(context(normalize(awaiting), receipt, orders, '3')).action.kind, 'none')
})
let amended
check('one owned amendment with unchanged total qty', () => {
  const result = execute(context(normalize(awaiting), receipt, [order], '3'))
  assert.deepEqual(result.action, { kind: 'amendOrder', order: { symbol: 'BTCUSDT', orderId: 'owned-1', price: '50010', qty: '0.001' } })
  amended = result.state
  assert.equal(execute(context(normalize(amended), null, [order], '4')).action.kind, 'none')
  assert.equal(execute(context(amended, receipt, [order], '4')).action.kind, 'none')
})
const amendReceipt = { sequence: '3', kind: 'amendOrder', status: 'accepted', submissionId: null, orderId: 'owned-1', errorCode: null }
check('only matching amendment acknowledgement completes', () => {
  for (const patch of [{ sequence: '1' }, { orderId: 'foreign' }, { kind: 'cancelOrder' }])
    assert.equal(execute(context(amended, { ...amendReceipt, ...patch }, [order], '4')).action.kind, 'none')
  const done = execute(context(normalize(amended), amendReceipt, [order], '4'))
  assert.equal(done.action.kind, 'stop')
  assert.equal(execute(context(done.state, amendReceipt, [order], '5')).action.kind, 'stop')
})
check('unresolved amendment never retries', () => {
  for (const status of ['rejected', 'unknown'])
    assert.equal(execute(context(amended, { ...amendReceipt, status }, [order], '4')).action.kind, 'stop')
})
check('malformed input/state/receipt cannot trade', () => {
  const safe = (ctx, parameters = input) => {
    try { assert.ok(['none', 'stop'].includes(execute(ctx, parameters).action.kind)) }
    catch (error) { assert.ok(error instanceof WebAssembly.RuntimeError, String(error)) }
  }
  for (const parameters of [{}, { ...input, qty: '0' }, { ...input, amendPrice: '50000' }, { ...input, qty: 'NaN' }, { ...input, symbol: 'BTC"USDT' }, { ...input, extra: 1 }, '{"symbol":"BTCUSDT","symbol":"ETHUSDT"}', JSON.stringify(input).replace(/}$/, ',}'), 'x'.repeat(4097)])
    safe(context(), parameters)
  safe(context({ phase: 'bogus' }))
  for (const patch of [{ orderId: null }, { submissionId: null }, { status: 'garbage' }, { errorCode: 'unexpected' }])
    safe(context(placed, { ...receipt, ...patch }, [order], '2'))
})
check('input and persisted key ordering do not change behavior', () => {
  assert.deepEqual(execute(context(), normalize(input)).action, execute(context()).action)
  assert.deepEqual(execute(context(normalize(awaiting), receipt, [order], '3')).action,
    { kind: 'amendOrder', order: { symbol: 'BTCUSDT', orderId: 'owned-1', price: '50010', qty: '0.001' } })
})
check('amendment also depends on configured input and exact decimals', () => {
  const parameters = { ...input, qty: '0.002', amendPrice: '50020' }
  const first = execute(context(), parameters)
  const owned = execute(context(first.state, receipt, [], '2'), parameters)
  assert.deepEqual(execute(context(owned.state, receipt, [{ ...order, qty: '0.0020', filledQty: '0.0019' }], '3'), parameters).action,
    { kind: 'amendOrder', order: { symbol: 'BTCUSDT', orderId: 'owned-1', price: '50020', qty: '0.002' } })
  assert.equal(execute(context(owned.state, receipt, [{ ...order, qty: '0.002', filledQty: '0.0020' }], '3'), parameters).action.kind, 'none')
})
check('missing snapshot/order collection cannot authorize amendment', () => {
  for (const snapshot of [null, {}, { orders: null }, { orders: { items: null } }])
    assert.equal(execute({ ...context(awaiting, receipt, [], '3'), snapshot }).action.kind, 'none')
})
console.log(`managed-entry fixture: ${checks} behavior groups passed; module ${wasm.length} bytes; manifest ${bytes.length} bytes; data segments ${segments}; bulk memory false`)
