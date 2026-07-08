import { createPinia, setActivePinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useConnectionStore } from '../../src/stores/connection'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

const emptySummary = {
  accountId: 'default',
  balances: [],
  totalEquity: '0',
}

const emptyPanels = {
  openOrders: [],
  orderHistory: [],
  positions: [],
}

function mockSuccessfulConnectInvoke(): void {
  vi.mocked(tauriInvoke).mockImplementation((cmd) => {
    if (cmd === 'get_connection_status') {
      return Promise.resolve('connected')
    }
    if (cmd === 'refresh_account') {
      return Promise.resolve(emptySummary)
    }
    if (cmd === 'refresh_market') {
      return Promise.resolve(undefined)
    }
    if (cmd === 'refresh_private_panels') {
      return Promise.resolve(emptyPanels)
    }
    return Promise.resolve(undefined)
  })
}

describe('connection store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  afterEach(() => {
    useConnectionStore().stopDataSync()
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

  it('passes through invoke error message', async () => {
    vi.mocked(tauriInvoke).mockRejectedValue('认证失败: 无效密钥')
    const store = useConnectionStore()
    await expect(store.connect()).rejects.toThrow('认证失败: 无效密钥')
    expect(store.status).toBe('error')
    expect(store.lastError).toBe('认证失败: 无效密钥')
  })

  it('refreshes account, market, and private panels after connect', async () => {
    mockSuccessfulConnectInvoke()
    const store = useConnectionStore()
    await store.connect(true)
    expect(store.status).toBe('connected')
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true,
      credential: undefined,
    })
    expect(tauriInvoke).toHaveBeenCalledWith('get_connection_status')
    expect(tauriInvoke).toHaveBeenCalledWith('refresh_account')
    expect(tauriInvoke).toHaveBeenCalledWith('refresh_market')
    expect(tauriInvoke).toHaveBeenCalledWith('refresh_private_panels', { symbol: null })
  })

  it('passes inline credential to connect command', async () => {
    mockSuccessfulConnectInvoke()
    const cred = {
      apiKey: 'k',
      apiSecret: 's',
      baseUrl: 'https://api.easicoin.io',
      label: 'default',
    }
    const store = useConnectionStore()
    await store.connect(true, cred)
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true,
      credential: cred,
    })
  })
})
