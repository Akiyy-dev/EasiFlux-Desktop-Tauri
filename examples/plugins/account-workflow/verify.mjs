import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const directory = path.dirname(fileURLToPath(import.meta.url))
const manifestPath = path.join(directory, 'manifest.json')
const manifestBytes = await readFile(manifestPath)
const manifest = JSON.parse(manifestBytes.toString('utf8'))

assert.ok(manifestBytes.length <= 16 * 1024, 'manifest exceeds the 16 KiB package limit')
assert.equal(manifest.schemaVersion, 5)
assert.equal(manifest.id, 'com.easiflux.examples.account-workflow')
assert.deepEqual(manifest.requestedCapabilities, [
  'account.read',
  'balances.read',
  'orders.read',
  'trade.place',
  'trade.cancel',
])

const encoder = new TextEncoder()
const decoder = new TextDecoder('utf-8', { fatal: true })

function contribution(id) {
  const value = manifest.contributions.find(item => item.contributionId === id)
  assert.ok(value, `missing contribution ${id}`)
  assert.equal(value.actionId, 'sandbox.accountWorkflow')
  assert.equal(value.params.runtime, 'wasm-v1')
  assert.equal(value.params.abi, 'account-json-v1')
  return value
}

async function execute(id, context, input) {
  const command = contribution(id)
  const bytes = Buffer.from(command.params.moduleBase64, 'base64')
  assert.ok(bytes.length <= 8192, `${id} module exceeds 8192 bytes`)
  const module = new WebAssembly.Module(bytes)
  assert.deepEqual(WebAssembly.Module.imports(module), [], `${id} unexpectedly imports host authority`)
  const instance = new WebAssembly.Instance(module, {})
  const { memory, alloc, run } = instance.exports
  assert.ok(memory instanceof WebAssembly.Memory)
  assert.equal(typeof alloc, 'function')
  assert.equal(typeof run, 'function')

  const contextBytes = encoder.encode(JSON.stringify(context))
  const inputBytes = encoder.encode(JSON.stringify(input))
  const contextPtr = alloc(contextBytes.length)
  new Uint8Array(memory.buffer, contextPtr, contextBytes.length).set(contextBytes)
  const inputPtr = alloc(inputBytes.length)
  new Uint8Array(memory.buffer, inputPtr, inputBytes.length).set(inputBytes)
  const packed = run(contextPtr, contextBytes.length, inputPtr, inputBytes.length)
  const outputPtr = Number((packed >> 32n) & 0xffff_ffffn)
  const outputLength = Number(packed & 0xffff_ffffn)
  const output = decoder.decode(new Uint8Array(memory.buffer, outputPtr, outputLength))
  return JSON.parse(output)
}

function snapshot(available, orderId = 'fixture-order-a') {
  return {
    schemaVersion: 1,
    account: {
      accountId: 'fixture-account',
      sessionEpoch: '7',
      environment: 'Injected smoke environment',
    },
    capturedAtMs: '1789948800000',
    symbol: 'BTCUSDT',
    grantedCapabilities: [
      'account.read', 'balances.read', 'orders.read', 'trade.place', 'trade.cancel',
    ],
    balances: {
      items: [{ asset: 'USDT', available, frozen: '0', total: available }],
      fetchedAtMs: '1789948799000',
      partial: true,
    },
    positions: null,
    orders: {
      items: [{
        orderId,
        symbol: 'BTCUSDT',
        side: 'Buy',
        orderType: 'Limit',
        price: '50000',
        qty: '0.001',
        status: 'New',
        orderLinkId: 'fixture-link-a',
        filledQty: '0',
        avgPrice: '0',
      }],
      fetchedAtMs: '1789948799500',
      partial: true,
    },
    market: null,
  }
}

assert.deepEqual(
  await execute('account.available-balance', snapshot('12.5'), {}),
  { kind: 'display', text: 'Available balance: 12.5' },
)
assert.deepEqual(
  await execute('account.available-balance', snapshot('8'), {}),
  { kind: 'display', text: 'Available balance: 8' },
)

const placeProposal = {
  kind: 'placeOrder',
  order: {
    symbol: 'BTCUSDT',
    side: 'Buy',
    orderType: 'Limit',
    qty: '0.002',
    price: '49000',
    timeInForce: 'GTC',
    positionIdx: 1,
    reduceOnly: false,
  },
}
assert.deepEqual(
  await execute('account.place-order', snapshot('12.5'), placeProposal),
  placeProposal,
)
assert.deepEqual(
  await execute('account.cancel-first-open-order', snapshot('12.5', 'captured-order-77'), {}),
  {
    kind: 'cancelOrder',
    order: { symbol: 'BTCUSDT', orderId: 'captured-order-77' },
  },
)

console.log('account-workflow fixture: 5 checks passed')
