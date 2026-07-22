import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import RiskControlPanel from '../../src/components/account/RiskControlPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useRiskStore } from '../../src/stores/risk'
import type { RiskStatus } from '../../src/types/models'

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

describe('RiskControlPanel', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
  })

  it('defers status request until the panel becomes active', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(ready)
    const wrapper = mount(RiskControlPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    await flushPromises()
    expect(tauriInvoke).not.toHaveBeenCalled()

    await wrapper.setProps({ active: true })
    await flushPromises()
    expect(tauriInvoke).toHaveBeenCalledWith('get_risk_status')
  })

  it('renders unavailable and disabled ledger states without fake zero usage', async () => {
    const store = useRiskStore()
    store.status = { ...ready, ledgerState: 'unavailable', occupiedOrders: null,
      remainingOrders: null, error: 'Risk usage ledger is unavailable.' }
    const wrapper = mount(RiskControlPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    expect(wrapper.text()).toContain('Risk usage ledger is unavailable.')
    expect(wrapper.get('[data-testid="risk-usage"]').text()).toContain('--')

    store.status = { ...ready, enabled: false, ledgerState: 'disabled',
      occupiedOrders: null, remainingOrders: null, updatedAtMs: null }
    await flushPromises()
    expect(wrapper.get('[data-testid="ledger-state"]').text()).toContain('Disabled')
    expect(wrapper.get('[data-testid="risk-usage"]').text()).toContain('--')
  })

  it('keeps draft and last snapshot when save fails', async () => {
    useRiskStore().status = ready
    const wrapper = mount(RiskControlPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    await wrapper.get('[data-testid="max-order-qty"]').setValue('77')
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('save failed'))

    await wrapper.get('[data-testid="save-risk"]').trigger('click')
    await flushPromises()

    expect((wrapper.get('[data-testid="max-order-qty"]').element as HTMLInputElement).value)
      .toBe('77')
    expect(useRiskStore().status).toEqual(ready)
  })

  it('replaces the snapshot after save and manual refresh', async () => {
    useRiskStore().status = ready
    const saved = { ...ready, maxOrderQty: '77' }
    const refreshed = { ...saved, occupiedOrders: 13, remainingOrders: 487 }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(saved).mockResolvedValueOnce(refreshed)
    const wrapper = mount(RiskControlPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })
    await wrapper.get('[data-testid="max-order-qty"]').setValue('77')

    await wrapper.get('[data-testid="save-risk"]').trigger('click')
    await flushPromises()
    expect(useRiskStore().status).toEqual(saved)
    expect(wrapper.get('[data-testid="max-order-qty"]').element).toHaveProperty('value', '77')

    await wrapper.get('[data-testid="refresh-risk"]').trigger('click')
    await flushPromises()
    expect(useRiskStore().status).toEqual(refreshed)
    expect(wrapper.get('[data-testid="risk-usage"]').text()).toContain('13 occupied')
  })

  it('sends enabled true when enabling a disabled snapshot', async () => {
    useRiskStore().status = { ...ready, enabled: false, ledgerState: 'disabled',
      occupiedOrders: null, remainingOrders: null, updatedAtMs: null }
    vi.mocked(tauriInvoke).mockResolvedValueOnce(ready)
    const wrapper = mount(RiskControlPanel, {
      props: { active: false }, global: { plugins: [pinia] },
    })

    await wrapper.get('input[type="checkbox"]').setValue(true)
    await wrapper.get('[data-testid="save-risk"]').trigger('click')
    await flushPromises()

    expect(tauriInvoke).toHaveBeenCalledWith('update_risk_config', {
      request: expect.objectContaining({ enabled: true }),
    })
  })
})
