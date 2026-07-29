import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import {
  loadChartWorkspace,
  refreshChartKlines,
  saveChartWorkspace,
} from '../../src/services/chartWorkspaceService'
import {
  flushActiveChartWorkspace,
  flushAllChartWorkspaces,
  registerChartWorkspaceFlusher,
} from '../../src/services/chartWorkspaceFlushRegistry'
import {
  key,
  samplePreferences,
  sampleSaveRequest,
  sampleSnapshot,
  successfulSave,
} from './helpers/chartWorkspaceFixtures'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

describe('chartWorkspaceService', () => {
  beforeEach(() => {
    vi.mocked(tauriInvoke).mockReset()
  })

  it('loads the exact workspace range', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue(sampleSnapshot())

    await loadChartWorkspace(
      { symbol: 'BTCUSDT', interval: '15' },
      { from: 1_000, to: 2_000, limit: 500 },
    )

    expect(tauriInvoke).toHaveBeenCalledWith('load_chart_workspace', {
      symbol: 'BTCUSDT',
      interval: '15',
      from: 1_000,
      to: 2_000,
      limit: 500,
    })
  })

  it('sends nullable dirty parts without uploading klines', async () => {
    const request = sampleSaveRequest({ viewState: null, preferences: samplePreferences(3) })
    vi.mocked(tauriInvoke).mockResolvedValue(successfulSave())

    await saveChartWorkspace(request)

    expect(tauriInvoke).toHaveBeenCalledWith('save_chart_workspace', { request })
    expect(tauriInvoke).not.toHaveBeenCalledWith(
      'save_chart_workspace',
      expect.objectContaining({ klines: expect.anything() }),
    )
  })

  it('fetches the exact REST range with existing command argument names', async () => {
    vi.mocked(tauriInvoke).mockResolvedValue([])

    await refreshChartKlines(key('ETHUSDT', '15'), {
      from: 1_000,
      to: 2_000,
      limit: 500,
    })

    expect(tauriInvoke).toHaveBeenCalledWith('fetch_klines', {
      symbol: 'ETHUSDT',
      interval: '15',
      start: 1_000,
      end: 2_000,
      limit: 500,
    })
  })
})

describe('chart workspace flush registry', () => {
  it('flushes only the active owner for page/context and every owner for close', async () => {
    const first = vi.fn().mockResolvedValue(undefined)
    const second = vi.fn().mockRejectedValue(new Error('disk full'))
    const a = registerChartWorkspaceFlusher(first)
    const b = registerChartWorkspaceFlusher(second)

    try {
      a.setActive(true)
      b.setActive(true)
      a.setActive(false)

      await expect(flushActiveChartWorkspace('page')).rejects.toThrow('disk full')
      await expect(flushAllChartWorkspaces('close')).resolves.toBeUndefined()
      expect(first).not.toHaveBeenCalledWith('page')
      expect(first).toHaveBeenCalledWith('close')
      expect(second).toHaveBeenCalledWith('page')
      expect(second).toHaveBeenCalledWith('close')
    } finally {
      a.unregister()
      b.unregister()
    }
  })

  it('does not let stale unregister clear a newer active owner', async () => {
    const first = vi.fn().mockResolvedValue(undefined)
    const second = vi.fn().mockResolvedValue(undefined)
    const a = registerChartWorkspaceFlusher(first)
    const b = registerChartWorkspaceFlusher(second)

    try {
      a.setActive(true)
      b.setActive(true)
      a.unregister()
      await flushActiveChartWorkspace('context')

      expect(first).not.toHaveBeenCalled()
      expect(second).toHaveBeenCalledWith('context')
    } finally {
      a.unregister()
      b.unregister()
    }
  })
})
