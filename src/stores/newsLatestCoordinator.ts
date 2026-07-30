export interface NewsLatestCoordinator {
  enqueue<T>(operation: () => Promise<T>): Promise<T | undefined>
  automatic(operation: () => Promise<void>): Promise<void>
  reset(): void
}

interface AutomaticFlightState {
  epoch: number
  accepting: boolean
  followUp: boolean
  promise: Promise<void>
}

export function createNewsLatestCoordinator(
  onBusyChange: (busy: boolean) => void,
): NewsLatestCoordinator {
  let epoch = 0
  let pendingCount = 0
  let tail: Promise<void> | null = null
  let automaticState: AutomaticFlightState | null = null

  function enqueue<T>(operation: () => Promise<T>): Promise<T | undefined> {
    const operationEpoch = epoch
    pendingCount += 1
    onBusyChange(true)
    const run = () => operationEpoch === epoch ? operation() : Promise.resolve(undefined)
    const result = tail ? tail.then(run) : run()
    const nextTail = result.then(() => undefined, () => undefined)
    tail = nextTail
    void nextTail.then(() => { if (tail === nextTail) tail = null })
    return result.finally(() => {
      if (operationEpoch !== epoch) return
      pendingCount -= 1
      if (pendingCount === 0) onBusyChange(false)
    })
  }

  function automatic(operation: () => Promise<void>): Promise<void> {
    if (automaticState?.accepting) {
      automaticState.followUp = true
      return automaticState.promise
    }
    const operationEpoch = epoch
    let resolveFlight!: () => void
    let rejectFlight!: (reason?: unknown) => void
    const promise = new Promise<void>((resolve, reject) => {
      resolveFlight = resolve
      rejectFlight = reject
    })
    const state: AutomaticFlightState = {
      epoch: operationEpoch, accepting: true, followUp: false, promise,
    }
    automaticState = state
    const execution = enqueue(async () => {
      let firstError: unknown
      let failed = false
      do {
        state.followUp = false
        if (operationEpoch !== epoch) return
        try {
          await operation()
        } catch (error) {
          if (!failed) firstError = error
          failed = true
        }
      } while (state.followUp && operationEpoch === epoch)
      if (operationEpoch === epoch && automaticState === state) state.accepting = false
      if (failed) throw firstError
    })
    void execution.then(
      () => {
        if (automaticState === state) automaticState = null
        resolveFlight()
      },
      (error: unknown) => {
        if (automaticState === state) automaticState = null
        rejectFlight(error)
      },
    )
    return promise
  }

  function reset(): void {
    epoch += 1
    pendingCount = 0
    tail = null
    automaticState = null
    onBusyChange(false)
  }

  return { enqueue, automatic, reset }
}
