import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import SettingsDialog from '../../src/components/settings/SettingsDialog.vue'
import OrderPanel from '../../src/components/trading/OrderPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import { useConnectionStore } from '../../src/stores/connection'
import type { AccountSwitchResult, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const config: AppConfig = {
  activeSymbol: 'BTCUSDT', activeAccountId: ' primary ', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: 'wss://example.test/public', wsPrivateUrl: 'wss://example.test/private',
  tickerPollInterval: 1000, windowWidth: 1200, windowHeight: 800,
  accounts: ['primary'], riskEnabled: true, riskMaxOrderQty: '10',
  riskMaxPriceDeviationPct: '5', riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => { resolve = done })
  return { promise, resolve }
}

describe('settings credential editor integration', () => {
  let pinia: Pinia

  beforeEach(async () => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        return Promise.resolve([{
          accountId: 'primary', label: 'Main account', baseUrl: 'https://trade.example',
          credentialState: 'present', active: true,
        }])
      }
      if (command === 'save_config') return Promise.resolve(config)
      if (command === 'get_connection_status') return Promise.resolve('connected')
      return Promise.resolve(undefined)
    })
    useConfigStore().config = config
    await useAccountProfilesStore().refreshProfiles()
  })

  function mountSettings() {
    return mount(SettingsDialog, {
      props: { show: true },
      global: {
        plugins: [pinia],
        stubs: {
          AppDialog: {
            props: ['show'],
            template: '<section v-if="show" data-testid="dialog"><slot/><slot name="footer"/></section>',
          },
        },
      },
    })
  }

  it('mounts the shared editor with sanitized active-profile metadata', async () => {
    const wrapper = mountSettings()
    await flushPromises()
    const editor = wrapper.getComponent(CredentialEditor)

    expect(editor.props()).toMatchObject({
      mode: 'edit', accountId: 'primary', initialLabel: 'Main account',
      initialBaseUrl: 'https://trade.example',
    })
    expect(wrapper.text()).not.toContain('API Secret')
    expect(wrapper.html()).not.toContain('apiSecret')
  })

  it('saves general settings then connects using stored Keyring credentials', async () => {
    const wrapper = mountSettings()
    await flushPromises()
    const edit = wrapper.findAll('button').find((button) => button.text() === '编辑凭据')
    await edit!.trigger('click')
    expect(wrapper.findAll('[data-testid="dialog"]')).toHaveLength(1)
    expect(wrapper.getComponent(CredentialEditor).props('show')).toBe(true)

    const save = wrapper.findAll('button').find((button) => button.text() === '保存')
    await save!.trigger('click')
    await flushPromises()

    const commands = vi.mocked(tauriInvoke).mock.calls.map(([command]) => command)
    expect(commands.indexOf('save_credentials')).toBeLessThan(commands.indexOf('save_config'))
    expect(commands.indexOf('save_config')).toBeLessThan(commands.indexOf('connect'))
    expect(tauriInvoke).toHaveBeenCalledWith('connect', {
      startRealtime: true, credential: undefined,
    })
    expect(wrapper.emitted('update:show')).toContainEqual([false])
    expect(wrapper.findAll('[data-testid="dialog"]')).toHaveLength(0)
  })

  it('hides settings while editing and restores it after cancel', async () => {
    const wrapper = mountSettings()
    await flushPromises()
    const edit = wrapper.findAll('button').find((button) => button.text() === '编辑凭据')
    await edit!.trigger('click')

    expect(wrapper.text()).not.toContain('行情轮询间隔')
    const cancel = wrapper.findAll('button').find((button) => button.text() === '取消')
    await cancel!.trigger('click')
    expect(wrapper.text()).toContain('行情轮询间隔')
  })
})

describe('credential draft lifetime', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.mocked(tauriInvoke).mockReset()
  })

  function mountEditor() {
    return mount(CredentialEditor, {
      props: {
        show: true,
        mode: 'edit',
        accountId: 'primary',
        initialLabel: 'Primary',
        initialBaseUrl: 'https://trade.example',
      },
      global: {
        plugins: [createPinia()],
        stubs: {
          AppDialog: {
            props: ['show'],
            template: '<section><slot/><slot name="footer"/></section>',
          },
        },
      },
    })
  }

  it('clears credential refs immediately when the dialog is hidden', async () => {
    const wrapper = mountEditor()
    const secrets = wrapper.findAll('input[type="password"]')
    await secrets[0].setValue('draft-key')
    await secrets[1].setValue('draft-secret')

    await wrapper.setProps({ show: false })

    expect(secrets[0].element.value).toBe('')
    expect(secrets[1].element.value).toBe('')
  })

  it('keeps a failed credential draft while the dialog remains open', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('save failed'))
    const wrapper = mountEditor()
    const secrets = wrapper.findAll('input[type="password"]')
    await secrets[0].setValue('draft-key')
    await secrets[1].setValue('draft-secret')
    const save = wrapper.findAll('button').find((button) => button.text() === '保存')
    await save!.trigger('click')
    await flushPromises()

    expect(wrapper.props('show')).toBe(true)
    expect(secrets[0].element.value).toBe('draft-key')
    expect(secrets[1].element.value).toBe('draft-secret')
  })
})

describe('account-switch order guard', () => {
  it('disables and programmatically blocks submit while switching', async () => {
    const pinia = createPinia()
    setActivePinia(pinia)
    useConnectionStore().setStatus('connected')
    const pending = deferred<AccountSwitchResult>()
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'switch_account') return pending.promise
      if (command === 'list_account_profiles') return Promise.resolve([])
      if (command === 'get_connection_status') return Promise.resolve('disconnected')
      return Promise.resolve(undefined)
    })
    const wrapper = mount(OrderPanel, {
      global: { plugins: [pinia], stubs: { NSlider: true, TradingAssetPanel: true } },
    })
    const fields = wrapper.findAll('.field-row input')
    await fields[0].setValue('60000')
    await fields[1].setValue('0.1')
    await flushPromises()
    expect(wrapper.get('button.submit-button').attributes('disabled')).toBeUndefined()

    const switching = useAccountProfilesStore().switchAccount('backup')
    await flushPromises()
    expect(wrapper.get('button.submit-button').attributes('disabled')).toBeDefined()
    await (wrapper.vm as unknown as { submit: () => Promise<void> }).submit()
    expect(tauriInvoke).not.toHaveBeenCalledWith('place_order', expect.anything())

    pending.resolve({ activeAccountId: 'backup', connected: false, sessionEpoch: 1 })
    await switching
  })
})
