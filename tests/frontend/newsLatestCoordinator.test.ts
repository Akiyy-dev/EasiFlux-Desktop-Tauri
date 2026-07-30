import { describe, expect, it } from 'vitest'
import { createNewsLatestCoordinator } from '../../src/stores/newsLatestCoordinator'

function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((ok) => { resolve = ok })
  return { promise, resolve }
}

describe('news latest coordinator', () => {
  it('starts a new automatic flight when an event arrives in the settle-to-finally window', async () => {
    const gate = deferred()
    let calls = 0
    let injectAtSettling = false
    let settledWindowFlight: Promise<void> | undefined
    let coordinator!: ReturnType<typeof createNewsLatestCoordinator>
    coordinator = createNewsLatestCoordinator((busy) => {
      if (busy || !injectAtSettling) return
      injectAtSettling = false
      settledWindowFlight = coordinator.automatic(async () => { calls += 1 })
    })

    const firstFlight = coordinator.automatic(async () => {
      calls += 1
      await gate.promise
    })
    injectAtSettling = true
    gate.resolve()
    await firstFlight
    await settledWindowFlight

    expect(calls).toBe(2)
  })

  it('does not let an old epoch stop the current flight accepting a coalesced successor', async () => {
    const gateA = deferred()
    const gateB = deferred()
    const busy: boolean[] = []
    let runsA = 0
    let runsB = 0
    let runsC = 0
    let runsD = 0
    const coordinator = createNewsLatestCoordinator((value) => busy.push(value))

    const flightA = coordinator.automatic(async () => {
      runsA += 1
      await gateA.promise
    })
    coordinator.reset()
    const flightB = coordinator.automatic(async () => {
      runsB += 1
      if (runsB === 1) await gateB.promise
    })

    gateA.resolve()
    await flightA
    const flightC = coordinator.automatic(async () => { runsC += 1 })
    const flightD = coordinator.automatic(async () => { runsD += 1 })

    expect(flightC).toBe(flightB)
    expect(flightD).toBe(flightB)
    gateB.resolve()
    await Promise.all([flightB, flightC, flightD])

    expect({ runsA, runsB, runsC, runsD }).toEqual({ runsA: 1, runsB: 2, runsC: 0, runsD: 0 })
    expect(busy).toEqual([true, false, true, false])
  })
})
