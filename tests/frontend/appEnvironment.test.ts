import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppStore } from '../../src/stores/app'

vi.mock('../../src/composables/useTauriCommand', () => ({
  tauriInvoke: vi.fn(),
}))

import { tauriInvoke } from '../../src/composables/useTauriCommand'

const production = {
  baseUrl: 'https://api.easicoin.io',
  label: '正式' as const,
  reachable: true,
  checkedAt: 1,
}

describe('app environment detection', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('coalesces concurrent manual environment checks', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue(production)
    const store = useAppStore()

    await Promise.all([store.refreshEnvironment(true), store.refreshEnvironment(true)])

    expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', {
      task: 'environment',
      force: true,
    })
    expect(store.environment.data?.label).toBe('正式')
    expect(store.environmentLoading).toBe(false)
  })

  it('applies environment events without another fetch', () => {
    const store = useAppStore()
    store.applyEnvironment(production)
    expect(store.environment.data?.label).toBe('正式')
  })
})
