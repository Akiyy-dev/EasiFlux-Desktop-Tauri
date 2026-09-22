import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const directory = path.dirname(fileURLToPath(import.meta.url))
const manifestBytes = await readFile(path.join(directory, 'manifest.json'))
const manifest = JSON.parse(manifestBytes.toString('utf8'))

assert.ok(manifestBytes.length <= 16 * 1024, 'manifest exceeds the 16 KiB package limit')
assert.equal(manifest.schemaVersion, 6)
assert.equal(manifest.id, 'com.easiflux.examples.threshold-strategy')
assert.deepEqual(manifest.requestedCapabilities, [
  'account.read',
  'orders.read',
  'market.read',
  'trade.place',
  'trade.cancel',
  'strategy.run',
])

const command = manifest.contributions.find(item => item.contributionId === 'strategy.threshold-once')
assert.ok(command, 'missing threshold strategy contribution')
assert.equal(command.actionId, 'sandbox.strategy')
assert.equal(command.params.runtime, 'wasm-v1')
assert.equal(command.params.abi, 'strategy-json-v1')

const moduleBytes = Buffer.from(command.params.moduleBase64, 'base64')
assert.ok(moduleBytes.length > 0, 'strategy module is empty')
assert.ok(moduleBytes.length <= 8192, 'strategy module exceeds 8192 decoded bytes')

function readUnsignedLeb(bytes, start) {
  let value = 0
  let shift = 0
  let offset = start
  while (offset < bytes.length && shift <= 28) {
    const byte = bytes[offset++]
    value |= (byte & 0x7f) << shift
    if ((byte & 0x80) === 0) return { value: value >>> 0, offset }
    shift += 7
  }
  throw new Error('invalid unsigned LEB128 in packaged module')
}

function inspectNativeSandboxShape(bytes) {
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0])
  let offset = 8
  let dataSegmentCount = 0
  let usesBulkMemory = false
  while (offset < bytes.length) {
    const sectionId = bytes[offset++]
    const sectionSize = readUnsignedLeb(bytes, offset)
    const payloadStart = sectionSize.offset
    const payloadEnd = payloadStart + sectionSize.value
    assert.ok(payloadEnd <= bytes.length, 'packaged module section exceeds byte length')
    if (sectionId === 11) dataSegmentCount = readUnsignedLeb(bytes, payloadStart).value
    if (sectionId === 10) {
      for (let index = payloadStart; index + 1 < payloadEnd; index += 1) {
        if (bytes[index] === 0xfc && bytes[index + 1] === 0x0a) usesBulkMemory = true
      }
    }
    offset = payloadEnd
  }
  return { dataSegmentCount, usesBulkMemory }
}

const nativeShape = inspectNativeSandboxShape(moduleBytes)
assert.deepEqual(
  [nativeShape.dataSegmentCount <= 32, !nativeShape.usesBulkMemory],
  [true, true],
  `native sandbox shape: ${nativeShape.dataSegmentCount} data segments, bulk memory ${nativeShape.usesBulkMemory}`,
)
const module = new WebAssembly.Module(moduleBytes)
assert.deepEqual(WebAssembly.Module.imports(module), [], 'strategy module unexpectedly imports host authority')

const encoder = new TextEncoder()
const decoder = new TextDecoder('utf-8', { fatal: true })

function execute(context, input) {
  const instance = new WebAssembly.Instance(module, {})
  const { memory, alloc, run } = instance.exports
  assert.ok(memory instanceof WebAssembly.Memory)
  assert.equal(typeof alloc, 'function')
  assert.equal(typeof run, 'function')

  const contextBytes = encoder.encode(JSON.stringify(context))
  const inputText = typeof input === 'string' ? input : JSON.stringify(input)
  const inputBytes = encoder.encode(inputText)
  const contextPtr = alloc(contextBytes.length)
  new Uint8Array(memory.buffer, contextPtr, contextBytes.length).set(contextBytes)
  const inputPtr = alloc(inputBytes.length)
  new Uint8Array(memory.buffer, inputPtr, inputBytes.length).set(inputBytes)
  const packed = run(contextPtr, contextBytes.length, inputPtr, inputBytes.length)
  const outputPtr = Number((packed >> 32n) & 0xffff_ffffn)
  const outputLength = Number(packed & 0xffff_ffffn)
  assert.ok(outputLength <= 16_384, 'guest output exceeds 16 KiB')
  return JSON.parse(decoder.decode(new Uint8Array(memory.buffer, outputPtr, outputLength)))
}

const orderTemplate = {
  symbol: 'BTCUSDT',
  side: 'Buy',
  orderType: 'Limit',
  qty: '0.001',
  price: '100',
  timeInForce: 'GTC',
  positionIdx: 1,
  reduceOnly: false,
}
const parameters = { threshold: '100', order: orderTemplate }

function contextAt(lastPrice, state = {}, lastReceipt = null, orders = []) {
  return {
    schemaVersion: 1,
    runId: '00000000-0000-4000-8000-000000000003',
    sequence: '1',
    event: 'timer',
    snapshot: {
      schemaVersion: 1,
      account: {
        accountId: 'synthetic-account',
        sessionEpoch: '7',
        environment: 'Injected test environment',
      },
      capturedAtMs: '1790035200000',
      symbol: 'BTCUSDT',
      grantedCapabilities: [
        'account.read', 'orders.read', 'market.read', 'trade.place', 'trade.cancel', 'strategy.run',
      ],
      balances: null,
      positions: null,
      orders: {
        items: orders,
        fetchedAtMs: '1790035199900',
        partial: true,
      },
      market: {
        ticker: {
          symbol: 'BTCUSDT',
          lastPrice,
          bidPrice: '99',
          askPrice: '101',
          markPrice: lastPrice,
        },
        fetchedAtMs: '1790035199950',
      },
    },
    state,
    lastReceipt,
  }
}

function acceptedPlacement(orderId) {
  return {
    sequence: '2',
    kind: 'placeOrder',
    status: 'accepted',
    submissionId: '00000000-0000-4000-8000-000000000004',
    orderId,
    errorCode: null,
  }
}

function rejectedPlacement() {
  return {
    sequence: '2',
    kind: 'placeOrder',
    status: 'rejected',
    submissionId: '00000000-0000-4000-8000-000000000004',
    orderId: null,
    errorCode: 'strategy_order_rejected',
  }
}

function activeOrder(orderId, status = 'New') {
  return {
    orderId,
    symbol: 'BTCUSDT',
    side: 'Buy',
    orderType: 'Limit',
    price: '100',
    qty: '0.001',
    status,
    orderLinkId: 'synthetic-link',
    filledQty: '0',
    avgPrice: '0',
  }
}

function normalizePersistedState(value) {
  if (Array.isArray(value)) return value.map(normalizePersistedState)
  if (value !== null && typeof value === 'object') {
    return Object.fromEntries(
      Object.keys(value).sort().map(key => [key, normalizePersistedState(value[key])]),
    )
  }
  return value
}

assert.deepEqual(execute(contextAt('99'), parameters), {
  state: { phase: 'waiting' },
  action: { kind: 'none' },
  message: 'Waiting for the configured threshold.',
})

assert.deepEqual(execute(contextAt('101'), parameters), {
  state: { phase: 'placed' },
  action: { kind: 'placeOrder', order: orderTemplate },
  message: 'Threshold reached; requesting one limit order.',
})

assert.equal(execute(contextAt('100'), { ...parameters, threshold: '101' }).action.kind, 'none')

const placed = { phase: 'placed' }
assert.deepEqual(execute(contextAt('102', placed), parameters), {
  state: { phase: 'placed' },
  action: { kind: 'none' },
  message: 'Waiting for the placement receipt.',
})
assert.deepEqual(execute(contextAt('102', placed, rejectedPlacement()), parameters), {
  state: { phase: 'done' },
  action: { kind: 'stop' },
  message: 'Placement was not accepted; stopping the example.',
})
const acceptedResult = execute(contextAt('102', placed, acceptedPlacement('owned-123')), parameters)
assert.deepEqual(acceptedResult, {
  state: { phase: 'awaitingOrder', orderId: 'owned-123' },
  action: { kind: 'none' },
  message: 'Placement accepted; waiting for the owned active order.',
})

const awaiting = { phase: 'awaitingOrder', orderId: 'owned-123' }
assert.equal(execute(contextAt('102', awaiting, acceptedPlacement('owned-123')), parameters).action.kind, 'none')
assert.equal(
  execute(contextAt('102', awaiting, acceptedPlacement('owned-123'), [activeOrder('other-456')]), parameters).action.kind,
  'none',
)
assert.deepEqual(
  execute(contextAt('102', awaiting, acceptedPlacement('owned-123'), [activeOrder('owned-123')]), parameters),
  {
    state: { phase: 'cancelRequested', orderId: 'owned-123' },
    action: { kind: 'cancelOrder', order: { symbol: 'BTCUSDT', orderId: 'owned-123' } },
    message: 'Owned active order is visible; requesting one cancellation.',
  },
)

assert.deepEqual(
  execute(contextAt('102', { phase: 'cancelRequested', orderId: 'owned-123' }), parameters),
  {
    state: { phase: 'done' },
    action: { kind: 'stop' },
    message: 'Cancellation was requested; stopping the example.',
  },
)

const normalizedAwaiting = normalizePersistedState(acceptedResult.state)
assert.equal(
  execute(contextAt('102', normalizedAwaiting, acceptedPlacement('owned-123')), parameters).action.kind,
  'none',
)
const normalizedCancelResult = execute(
  contextAt('102', normalizedAwaiting, acceptedPlacement('owned-123'), [activeOrder('owned-123')]),
  parameters,
)
assert.deepEqual(normalizedCancelResult, {
  state: { phase: 'cancelRequested', orderId: 'owned-123' },
  action: { kind: 'cancelOrder', order: { symbol: 'BTCUSDT', orderId: 'owned-123' } },
  message: 'Owned active order is visible; requesting one cancellation.',
})
assert.deepEqual(
  execute(contextAt('102', normalizePersistedState(normalizedCancelResult.state)), parameters),
  {
    state: { phase: 'done' },
    action: { kind: 'stop' },
    message: 'Cancellation was requested; stopping the example.',
  },
)

assert.throws(() => execute(contextAt('101'), '{"threshold":"100"}'), WebAssembly.RuntimeError)
assert.throws(
  () => execute(
    contextAt('101'),
    JSON.stringify(parameters).replace('"reduceOnly":false', '"reduceOnly":false,"takeProfit":"1"'),
  ),
  WebAssembly.RuntimeError,
)
assert.throws(
  () => execute(contextAt('101'), `{"threshold":"${'1'.repeat(4097)}"}`),
  WebAssembly.RuntimeError,
)

console.log(`threshold-strategy fixture: 17 behavior checks passed; module ${moduleBytes.length} bytes; manifest ${manifestBytes.length} bytes; data segments ${nativeShape.dataSegmentCount}; bulk memory ${nativeShape.usesBulkMemory}`)
