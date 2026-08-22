import { createPinia, setActivePinia } from 'pinia'
import { watch } from 'vue'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

function reconnectAction(store: ReturnType<typeof useConnectionStore>) {
  // Pinia wraps public action promises for subscriptions. Exercise the setup
  // action itself when asserting the identity of the store's shared flight.
  return (store as unknown as {
    _hmrPayload: { actions: { reconnect: typeof store.reconnect } }
  })._hmrPayload.actions.reconnect
}

describe('connection store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('tracks api and websocket status separately', () => {
    const store = useConnectionStore()
    store.setStatus('connected')
    store.setWsStatus('error')
    expect(store.status).toBe('connected')
    expect(store.wsStatus).toBe('error')
    expect(store.connected).toBe(true)
    expect(store.wsConnected).toBe(false)
  })

  it('refreshes websocket status from the authoritative backend snapshot', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_websocket_status') return Promise.resolve('connecting')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    await expect(store.refreshWsStatus()).resolves.toBe('connecting')

    expect(tauriInvoke).toHaveBeenCalledWith('get_websocket_status')
    expect(store.wsStatus).toBe('connecting')
  })

  it('does not let an in-flight websocket snapshot overwrite a newer event', async () => {
    let resolveSnapshot!: (status: 'connecting') => void
    const snapshot = new Promise<'connecting'>((resolve) => { resolveSnapshot = resolve })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_websocket_status') return snapshot
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const refresh = store.refreshWsStatus()
    store.setWsStatus('connected')
    resolveSnapshot('connecting')
    await refresh

    expect(store.wsStatus).toBe('connected')
  })

  it('does not let an in-flight websocket snapshot overwrite a completed disconnect', async () => {
    let resolveSnapshot!: (status: 'connected') => void
    const snapshot = new Promise<'connected'>((resolve) => { resolveSnapshot = resolve })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_websocket_status') return snapshot
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connecting')

    const refresh = store.refreshWsStatus()
    await store.disconnect()
    resolveSnapshot('connected')
    await refresh

    expect(store.wsStatus).toBe('disconnected')
  })

  it('passes through invoke error message', async () => {
    vi.mocked(tauriInvoke).mockRejectedValue('认证失败: 无效密钥')
    const store = useConnectionStore()
    await expect(store.connect()).rejects.toThrow('认证失败: 无效密钥')
    expect(store.status).toBe('error')
    expect(store.lastError).toBe('认证失败: 无效密钥')
  })

  it('does not run status refresh or sync for an older connect success', async () => {
    const firstInvoke = deferred<void>()
    const secondInvoke = deferred<void>()
    let connectCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') {
        connectCalls += 1
        return connectCalls === 1 ? firstInvoke.promise : secondInvoke.promise
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.connect(true)
    const second = store.connect(true)
    firstInvoke.resolve()
    await first

    expect(store.status).toBe('connecting')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)

    secondInvoke.resolve()
    await second

    expect(store.status).toBe('connected')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(1)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(1)
  })

  it('rejects an older failed caller without overwriting a newer success', async () => {
    const firstInvoke = deferred<void>()
    let connectCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') {
        connectCalls += 1
        return connectCalls === 1 ? firstInvoke.promise : Promise.resolve(undefined)
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.connect(true)
    await store.connect(true)
    firstInvoke.reject(new Error('apiKey=raw-key apiSecret=raw-secret'))

    await expect(first).rejects.toThrow('apiKey=raw-key apiSecret=raw-secret')
    expect(store.status).toBe('connected')
    expect(store.lastError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(1)
  })

  it('keeps a completed disconnect authoritative over an older pending connect', async () => {
    const pendingConnect = deferred<void>()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return pendingConnect.promise
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const connecting = store.connect(true)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))
    await store.disconnect()
    pendingConnect.resolve()
    await connecting

    expect(store.status).toBe('disconnected')
    expect(store.wsStatus).toBe('disconnected')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('does not sync from an older status refresh after a newer connect starts', async () => {
    const firstStatus = deferred<'connected'>()
    const secondInvoke = deferred<void>()
    let connectCalls = 0
    let statusCalls = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') {
        connectCalls += 1
        return connectCalls === 1 ? Promise.resolve(undefined) : secondInvoke.promise
      }
      if (command === 'get_connection_status') {
        statusCalls += 1
        return statusCalls === 1 ? firstStatus.promise : Promise.resolve('connected')
      }
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.connect(true)
    await vi.waitFor(() => expect(statusCalls).toBe(1))
    const second = store.connect(true)
    firstStatus.resolve('connected')
    await first

    expect(store.status).toBe('connecting')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)

    secondInvoke.resolve()
    await second

    expect(store.status).toBe('connected')
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(1)
  })

  it('does not apply a connect success after its caller validity expires', async () => {
    const pendingConnect = deferred<void>()
    let callerValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return pendingConnect.promise
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    const connecting = store.connect(false, undefined, () => callerValid)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))
    callerValid = false
    pendingConnect.resolve()

    await expect(connecting).resolves.toBeUndefined()
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('connected')
    expect(store.lastError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('does not apply a connect failure after its caller validity expires', async () => {
    const pendingConnect = deferred<void>()
    let callerValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return pendingConnect.promise
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    const connecting = store.connect(false, undefined, () => callerValid)
    const rejection = expect(connecting).rejects.toThrow('apiKey=raw-key apiSecret=raw-secret')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))
    callerValid = false
    pendingConnect.reject(new Error('apiKey=raw-key apiSecret=raw-secret'))

    await rejection
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('connected')
    expect(store.lastError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('does not apply a status refresh after its connect caller validity expires', async () => {
    const pendingStatus = deferred<'connected'>()
    let callerValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return Promise.resolve(undefined)
      if (command === 'get_connection_status') return pendingStatus.promise
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    const connecting = store.connect(false, undefined, () => callerValid)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_connection_status'))
    callerValid = false
    pendingStatus.resolve('connected')

    await expect(connecting).resolves.toBeUndefined()
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('disconnected')
    expect(store.lastError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('does not apply a status refresh failure after its connect caller validity expires', async () => {
    const pendingStatus = deferred<'connected'>()
    let callerValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return Promise.resolve(undefined)
      if (command === 'get_connection_status') return pendingStatus.promise
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    const connecting = store.connect(false, undefined, () => callerValid)
    const rejection = expect(connecting).rejects.toThrow('stale status failed')
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('get_connection_status'))
    callerValid = false
    pendingStatus.reject(new Error('stale status failed'))

    await rejection
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('disconnected')
    expect(store.lastError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('preserves the invoke error when a completion validity predicate throws', async () => {
    const invokeError = new Error('original connect failed')
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return Promise.reject(invokeError)
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    await expect(store.connect(true, undefined, () => {
      throw new Error('validity predicate failed')
    })).rejects.toThrow('original connect failed')

    expect(store.status).toBe('connecting')
    expect(store.lastError).toBeNull()
  })

  it('delegates post-connect refresh to scheduler bridge', async () => {
    vi.mocked(tauriInvoke).mockImplementation((cmd) => {
      if (cmd === 'get_connection_status') {
        return Promise.resolve('connected')
      }
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    await store.connect(true)
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true,
      credential: undefined,
    })
    expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', {
      task: 'market',
      force: true,
    })
  })

  it('clears a stale websocket status after connecting without realtime', async () => {
    vi.mocked(tauriInvoke).mockImplementation((cmd) => {
      if (cmd === 'get_connection_status') {
        return Promise.resolve('connected')
      }
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    await store.connect(false)

    expect(store.status).toBe('connected')
    expect(store.wsStatus).toBe('disconnected')
  })

  it('does not clear websocket status when realtime is requested', async () => {
    vi.mocked(tauriInvoke).mockImplementation((cmd) => {
      if (cmd === 'get_connection_status') {
        return Promise.resolve('connected')
      }
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setWsStatus('connected')

    await store.connect(true)

    expect(store.wsStatus).toBe('connected')
  })

  it('reconnects once by disconnecting before connecting', async () => {
    const store = useConnectionStore()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'get_connection_status') return Promise.resolve('connected')
      if (command === 'scheduler_run_task') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })

    await Promise.all([store.reconnect(false), store.reconnect(false)])

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(store.reconnecting).toBe(false)
    expect(store.reconnectError).toBeNull()
  })

  it('publishes the active reconnect promise before synchronous true watchers reenter', async () => {
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    const reconnect = reconnectAction(store)
    let joined!: Promise<void>
    const stop = watch(
      () => store.reconnecting,
      (reconnecting) => {
        if (reconnecting) joined = reconnect(true)
      },
      { flush: 'sync' },
    )

    try {
      const first = reconnect(false)
      let firstSettled = false
      let joinedSettled = false
      void first.then(() => { firstSettled = true })
      void joined.then(() => { joinedSettled = true })

      expect(joined).toBe(first)
      await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
      await Promise.resolve()
      expect(firstSettled).toBe(false)
      expect(joinedSettled).toBe(false)

      releaseDisconnect()
      await expect(Promise.all([first, joined])).resolves.toEqual([undefined, undefined])

      const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
      expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
      expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
      expect(tauriInvoke).toHaveBeenCalledWith('connect', {
        startRealtime: false,
        credential: undefined,
      })
    } finally {
      stop()
    }
  })

  it('starts a new reconnect flight from a synchronous false watcher', async () => {
    let releaseSecondDisconnect!: () => void
    const secondDisconnectPending = new Promise<void>((resolve) => {
      releaseSecondDisconnect = resolve
    })
    let disconnectCount = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') {
        disconnectCount += 1
        return disconnectCount === 1 ? Promise.resolve(undefined) : secondDisconnectPending
      }
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    const reconnect = reconnectAction(store)
    let second: Promise<void> | undefined
    let startedSecond = false
    const stop = watch(
      () => store.reconnecting,
      (reconnecting) => {
        if (!reconnecting && !startedSecond) {
          startedSecond = true
          second = reconnect(true)
        }
      },
      { flush: 'sync' },
    )

    try {
      const first = reconnect(false)
      await first

      expect(second).toBeDefined()
      expect(second).not.toBe(first)
      await vi.waitFor(() => expect(disconnectCount).toBe(2))

      let secondSettled = false
      void second!.then(() => { secondSettled = true })
      await Promise.resolve()
      expect(secondSettled).toBe(false)

      releaseSecondDisconnect()
      await expect(second).resolves.toBeUndefined()

      const reconnectCommands = vi.mocked(tauriInvoke).mock.calls
        .filter(([command]) => command === 'disconnect' || command === 'connect')
      expect(reconnectCommands).toEqual([
        ['disconnect'],
        ['connect', { startRealtime: false, credential: undefined }],
        ['disconnect'],
        ['connect', { startRealtime: true, credential: undefined }],
      ])
    } finally {
      stop()
    }
  })

  it('observes intents appended synchronously while evaluating a reconnect guard', async () => {
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    const reconnect = reconnectAction(store)
    let joined!: Promise<void>

    const first = reconnect(false, () => {
      joined = reconnect(true)
      return false
    })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    releaseDisconnect()

    await vi.waitFor(() => expect(joined).toBeDefined())
    expect(joined).toBe(first)
    await expect(Promise.all([first, joined])).resolves.toEqual([undefined, undefined])

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true,
      credential: undefined,
    })
  })

  it('skips connect when the caller guard invalidates during disconnect', async () => {
    let allowConnect = true
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const reconnecting = store.reconnect(false, () => allowConnect)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    allowConnect = false
    releaseDisconnect()
    await reconnecting

    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect(store.status).toBe('disconnected')
    expect(store.reconnecting).toBe(false)
    expect(store.reconnectError).toBeNull()
  })

  it('does not apply a guarded reconnect success after its selected intent expires', async () => {
    const pendingConnect = deferred<void>()
    let selectedIntentValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return pendingConnect.promise
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setStatus('connected')

    const reconnecting = store.reconnect(false, () => selectedIntentValid)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))
    selectedIntentValid = false
    pendingConnect.resolve()

    await expect(reconnecting).resolves.toBeUndefined()
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('disconnected')
    expect(store.lastError).toBeNull()
    expect(store.reconnectError).toBeNull()
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('rejects a stale guarded reconnect flight without applying its failure', async () => {
    const pendingConnect = deferred<void>()
    let selectedIntentValid = true
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'connect') return pendingConnect.promise
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    store.setStatus('connected')

    const first = store.reconnect(true, () => selectedIntentValid)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('connect', expect.anything()))
    const joined = store.reconnect(true, () => selectedIntentValid)
    const firstRejection = expect(first).rejects.toThrow('stale reconnect failed')
    const joinedRejection = expect(joined).rejects.toThrow('stale reconnect failed')
    selectedIntentValid = false
    pendingConnect.reject(new Error('stale reconnect failed'))

    await firstRejection
    await joinedRejection
    expect(store.status).toBe('connecting')
    expect(store.wsStatus).toBe('disconnected')
    expect(store.lastError).toBeNull()
    expect(store.reconnectError).toBeNull()
    expect(store.reconnecting).toBe(false)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'connect')).toHaveLength(1)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'get_connection_status')).toHaveLength(0)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'scheduler_run_task')).toHaveLength(0)
  })

  it('uses a later default intent when the first guarded intent becomes stale', async () => {
    let firstIntentValid = true
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.reconnect(false, () => firstIntentValid)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    const second = store.reconnect(true)
    firstIntentValid = false
    releaseDisconnect()

    await expect(Promise.all([first, second])).resolves.toEqual([undefined, undefined])
    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true,
      credential: undefined,
    })
  })

  it('keeps the first default intent ahead of a later guarded intent', async () => {
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.reconnect(false)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    const second = store.reconnect(true, () => true)
    releaseDisconnect()

    await expect(Promise.all([first, second])).resolves.toEqual([undefined, undefined])
    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
  })

  it('keeps the first valid guarded intent ahead of a later default intent', async () => {
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()

    const first = store.reconnect(false, () => true)
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    const second = store.reconnect(true)
    releaseDisconnect()

    await expect(Promise.all([first, second])).resolves.toEqual([undefined, undefined])
    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: false,
      credential: undefined,
    })
  })

  it('rejects the flight when an intent guard throws', async () => {
    let releaseDisconnect!: () => void
    const disconnectPending = new Promise<void>((resolve) => {
      releaseDisconnect = resolve
    })
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return disconnectPending
      return Promise.resolve(undefined)
    })
    const store = useConnectionStore()
    const guardError = new Error('reconnect guard failed')

    const first = store.reconnect(false, () => { throw guardError })
    await vi.waitFor(() => expect(tauriInvoke).toHaveBeenCalledWith('disconnect'))
    const second = store.reconnect(true)
    releaseDisconnect()

    await expect(first).rejects.toThrow('reconnect guard failed')
    await expect(second).rejects.toThrow('reconnect guard failed')
    expect(tauriInvoke).not.toHaveBeenCalledWith('connect', expect.anything())
    expect(store.reconnecting).toBe(false)
    expect(store.reconnectError).toBe('reconnect guard failed')
  })

  it('rejects a failed reconnect and leaves it retryable', async () => {
    const store = useConnectionStore()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'disconnect') return Promise.resolve(undefined)
      if (command === 'connect') return Promise.reject(new Error('reconnect socket failed'))
      return Promise.resolve(undefined)
    })

    await expect(store.reconnect(true)).rejects.toThrow('reconnect socket failed')

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'disconnect')).toHaveLength(1)
    expect(commands.filter((command) => command === 'connect')).toHaveLength(1)
    expect(commands.indexOf('disconnect')).toBeLessThan(commands.indexOf('connect'))
    expect(store.reconnecting).toBe(false)
    expect(store.reconnectError).toBe('reconnect socket failed')
  })
})
