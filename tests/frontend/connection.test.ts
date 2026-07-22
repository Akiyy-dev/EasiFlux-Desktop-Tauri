import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

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
})
