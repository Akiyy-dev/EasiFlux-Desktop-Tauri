import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountAssetsPanel from '../../src/components/account/AccountAssetsPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountStore } from '../../src/stores/account'
import { useConnectionStore } from '../../src/stores/connection'
import type { FundingBalance } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const funding: FundingBalance = {
  asset: 'USDT',
  available: '8',
  frozen: '2',
  total: '10',
}

describe('account asset state', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('refreshes normalized funding balances', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([funding])
    const store = useAccountStore()

    await store.refreshFundingBalances()

    expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    expect(store.fundingBalances).toEqual([funding])
    expect(store.fundingError).toBeNull()
  })

  it('keeps contract data when the funding request fails', async () => {
    const store = useAccountStore()
    store.applySnapshot({
      accountId: 'primary',
      balances: [{ asset: 'USDT', available: '80', frozen: '20', total: '100' }],
      totalEquity: '100',
    })
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('funding unavailable'))

    await expect(store.refreshFundingBalances()).rejects.toThrow('funding unavailable')

    expect(store.summary?.totalEquity).toBe('100')
    expect(store.balances).toHaveLength(1)
    expect(store.fundingError).toBe('funding unavailable')
  })

  it('clears funding balances with the rest of account-bound data', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([funding])
    const store = useAccountStore()
    await store.refreshFundingBalances()

    store.clearAccountData()

    expect(store.fundingBalances).toEqual([])
    expect(store.fundingStatus).toBe('idle')
  })
})

describe('AccountAssetsPanel', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  it('does not invoke private commands while disconnected', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('连接账户后刷新')
    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('refreshes every section independently when connected', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'refresh_positions') return Promise.resolve([])
      if (command === 'fetch_funding_balances') return Promise.reject(new Error('funding unavailable'))
      return Promise.resolve(undefined)
    })

    mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'account', force: true })
      expect(tauriInvoke).toHaveBeenCalledWith('refresh_positions', { symbol: null })
      expect(tauriInvoke).toHaveBeenCalledWith('scheduler_run_task', { task: 'dailyPnl', force: true })
      expect(tauriInvoke).toHaveBeenCalledWith('fetch_funding_balances')
    })
    expect(useAccountStore().fundingError).toBe('funding unavailable')
  })

  it('keeps scheduler failures isolated from successful positions and funding', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    vi.mocked(tauriInvoke).mockImplementation((command, args) => {
      if (command === 'scheduler_run_task' && args?.task === 'account') {
        return Promise.reject(new Error('account unavailable'))
      }
      if (command === 'scheduler_run_task' && args?.task === 'dailyPnl') {
        return Promise.reject(new Error('daily PnL unavailable'))
      }
      if (command === 'refresh_positions') {
        return Promise.resolve([{
          symbol: 'BTCUSDT', side: 'Buy', size: '0.1', entryPrice: '60000',
          leverage: '10', unrealisedPnl: '1', positionIdx: 1,
        }])
      }
      if (command === 'fetch_funding_balances') return Promise.resolve([funding])
      return Promise.resolve(undefined)
    })

    const wrapper = mount(AccountAssetsPanel, { global: { plugins: [pinia] } })

    await vi.waitFor(() => {
      expect(wrapper.text()).toContain('account unavailable')
      expect(wrapper.text()).toContain('daily PnL unavailable')
      expect(wrapper.text()).toContain('BTCUSDT')
      expect(wrapper.text()).toContain('USDT')
    })
    const headings = wrapper.findAll('.asset-section h3').map((heading) => heading.text())
    expect(headings[0]).toContain('--')
    expect(headings[1]).not.toContain('--')
    expect(headings[2]).toContain('--')
    expect(headings[3]).not.toContain('--')
  })
})
