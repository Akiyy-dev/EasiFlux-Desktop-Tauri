import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useRiskStore } from '../../src/stores/risk'
import { useConfigStore } from '../../src/stores/config'
import type { RiskStatus, UpdateRiskConfigRequest } from '../../src/types/models'
import { validateRiskConfig } from '../../src/utils/risk'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const ready: RiskStatus = {
  enabled: true,
  maxOrderQty: '100',
  maxPriceDeviationPct: '5',
  maxDailyOrders: 500,
  tradingDayTimezone: 'Asia/Shanghai',
  ledgerState: 'ready',
  tradingDay: '2026-07-22',
  occupiedOrders: 12,
  remainingOrders: 488,
  updatedAtMs: 1_784_692_800_000,
  error: null,
}

function request(overrides: Partial<UpdateRiskConfigRequest> = {}): UpdateRiskConfigRequest {
  return {
    enabled: true,
    maxOrderQty: '10',
    maxPriceDeviationPct: '1',
    maxDailyOrders: 25,
    tradingDayTimezone: 'UTC',
    ...overrides,
  }
}

describe('risk validator and store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it.each([
    ['invalid quantity', { maxOrderQty: 'abc' }],
    ['zero quantity', { maxOrderQty: '0' }],
    ['negative quantity', { maxOrderQty: '-1' }],
    ['invalid deviation', { maxPriceDeviationPct: 'abc' }],
    ['negative deviation', { maxPriceDeviationPct: '-1' }],
    ['non-positive limit', { maxDailyOrders: 0 }],
    ['blank timezone', { tradingDayTimezone: '   ' }],
  ])('rejects %s before invoking the backend', async (_label, overrides) => {
    const invalid = request(overrides)
    expect(validateRiskConfig(invalid)).not.toBeNull()

    await expect(useRiskStore().save(invalid)).rejects.toThrow()

    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('refreshes status through the read-only command', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(ready)
    const store = useRiskStore()

    await store.refresh()

    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
    expect(store.status).toEqual(ready)
  })

  it('trims saves and adopts the returned snapshot', async () => {
    const updated = { ...ready, maxOrderQty: '25.5', tradingDayTimezone: 'UTC' }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(updated)
    const store = useRiskStore()

    await store.save(request({
      maxOrderQty: ' 25.5 ',
      maxPriceDeviationPct: ' 0 ',
      tradingDayTimezone: ' UTC ',
    }))

    expect(tauriInvoke).toHaveBeenCalledWith('update_risk_config', {
      request: {
        enabled: true,
        maxOrderQty: '25.5',
        maxPriceDeviationPct: '0',
        maxDailyOrders: 25,
        tradingDayTimezone: 'UTC',
      },
    })
    expect(store.status).toEqual(updated)
  })

  it('keeps risk fields authoritative when ordinary settings save next', async () => {
    const config = useConfigStore()
    config.config = {
      activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
      theme: 'dark', klineInterval: '15', useWebsocket: true,
      wsPublicUrl: 'wss://example.test/public', wsPrivateUrl: 'wss://example.test/private',
      tickerPollInterval: 1, windowWidth: 1200, windowHeight: 800, accounts: ['primary'],
      riskEnabled: true, riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5',
      riskMaxDailyOrders: 100, tradingDayTimezone: 'Asia/Shanghai',
    }
    const updated = { ...ready, maxOrderQty: '25.5', maxDailyOrders: 20,
      tradingDayTimezone: 'UTC' }
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'update_risk_config') return Promise.resolve(updated)
      if (command === 'save_config') return Promise.resolve(args?.config)
      return Promise.resolve(undefined)
    })

    await useRiskStore().save(request({ maxOrderQty: '25.5', maxDailyOrders: 20 }))
    await config.saveConfig({ ...config.config!, windowWidth: 1440 })

    expect(tauriInvoke).toHaveBeenLastCalledWith('save_config', {
      config: expect.objectContaining({
        windowWidth: 1440,
        riskMaxOrderQty: '25.5',
        riskMaxDailyOrders: 20,
        tradingDayTimezone: 'UTC',
      }),
    })
  })
})
