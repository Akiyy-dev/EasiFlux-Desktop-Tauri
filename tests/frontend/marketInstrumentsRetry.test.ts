import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useMarketStore } from '../../src/stores/market'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((next) => {
    resolve = next
  })
  return { promise, resolve }
}

describe('market instrument loading', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    localStorage.clear()
    vi.mocked(tauriInvoke).mockReset()
  })

  it('retries after the first request fails', async () => {
    vi.mocked(tauriInvoke)
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce([{ symbol: 'BTCUSDT' }])
    const store = useMarketStore()

    await store.loadInstruments(['ETHUSDT'])
    expect(store.symbols).toEqual(['ETHUSDT'])

    await store.loadInstruments(['ETHUSDT'])

    expect(tauriInvoke).toHaveBeenCalledTimes(2)
    expect(store.symbols).toEqual(['BTCUSDT'])
    expect(store.instrumentsError).toBeNull()
  })

  it('retries after an empty response', async () => {
    vi.mocked(tauriInvoke)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([{ symbol: 'SOLUSDT' }])
    const store = useMarketStore()

    await store.loadInstruments()
    await store.loadInstruments()

    expect(tauriInvoke).toHaveBeenCalledTimes(2)
    expect(store.symbols).toEqual(['SOLUSDT'])
  })

  it('deduplicates concurrent requests and caches a non-empty success', async () => {
    const response = deferred<unknown>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(response.promise)
    const store = useMarketStore()

    const first = store.loadInstruments()
    const second = store.loadInstruments()
    expect(tauriInvoke).toHaveBeenCalledTimes(1)

    response.resolve([{ symbol: 'BTCUSDT' }])
    await Promise.all([first, second])
    await store.loadInstruments()

    expect(tauriInvoke).toHaveBeenCalledTimes(1)
    expect(store.symbols).toEqual(['BTCUSDT'])
  })
})
