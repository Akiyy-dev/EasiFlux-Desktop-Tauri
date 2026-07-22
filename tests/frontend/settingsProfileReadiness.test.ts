import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import CredentialEditor from '../../src/components/account/CredentialEditor.vue'
import SettingsDialog from '../../src/components/settings/SettingsDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useConfigStore } from '../../src/stores/config'
import type { AccountProfile, AppConfig } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

const config = {
  activeSymbol: 'BTCUSDT', activeAccountId: 'primary', watchlistSymbols: ['BTCUSDT'],
  theme: 'dark', klineInterval: '15', useWebsocket: true,
  wsPublicUrl: '', wsPrivateUrl: '', tickerPollInterval: 1000,
  windowWidth: 1200, windowHeight: 800, accounts: ['primary'], riskEnabled: true,
  riskMaxOrderQty: '10', riskMaxPriceDeviationPct: '5', riskMaxDailyOrders: 100,
  tradingDayTimezone: 'Asia/Shanghai',
} satisfies AppConfig

const profile: AccountProfile = {
  accountId: 'primary', label: 'Sanitized label', baseUrl: 'https://loaded.example',
  credentialState: 'present', active: true,
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

describe('settings profile readiness', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    useConfigStore().config = config
    vi.mocked(tauriInvoke).mockReset()
  })

  function mountSettings() {
    return mount(SettingsDialog, {
      props: { show: true },
      global: {
        plugins: [pinia],
        stubs: { AppDialog: {
          props: ['show'], template: '<section v-if="show"><slot/><slot name="footer"/></section>',
        } },
      },
    })
  }

  function editorEntryButtons(wrapper: ReturnType<typeof mountSettings>) {
    return wrapper.findAll('button').filter((button) =>
      button.text() === 'Edit credentials'
        || button.text() === 'Save credentials and connect')
  }

  it('blocks both entry points until the sanitized active profile is loaded', async () => {
    const pending = deferred<AccountProfile[]>()
    vi.mocked(tauriInvoke).mockReturnValueOnce(pending.promise)
    const wrapper = mountSettings()

    expect(wrapper.text()).toContain('Loading account profile')
    expect(editorEntryButtons(wrapper)).toHaveLength(2)
    expect(editorEntryButtons(wrapper)
      .every((button) => button.attributes('disabled') !== undefined))
      .toBe(true)
    expect(wrapper.findComponent(CredentialEditor).exists()).toBe(false)

    pending.resolve([profile])
    await flushPromises()
    const edit = wrapper.findAll('button').find((button) => button.text() === 'Edit credentials')
    await edit!.trigger('click')
    const editor = wrapper.getComponent(CredentialEditor)
    expect(editor.props()).toMatchObject({
      accountId: 'primary', initialLabel: 'Sanitized label',
      initialBaseUrl: 'https://loaded.example', requireCredentials: false,
    })
  })

  it('blocks both entry points for unavailable credentials', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([{ ...profile, credentialState: 'unavailable' }])
    const wrapper = mountSettings()
    await flushPromises()

    expect(wrapper.text()).toContain('Credential storage is unavailable')
    expect(editorEntryButtons(wrapper)
      .every((button) => button.attributes('disabled') !== undefined))
      .toBe(true)
  })

  it('shows profile load errors and keeps both entry points blocked', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('profile list failed'))
    const wrapper = mountSettings()
    await flushPromises()

    expect(wrapper.get('[role="alert"]').text()).toContain('profile list failed')
    expect(editorEntryButtons(wrapper)
      .every((button) => button.attributes('disabled') !== undefined))
      .toBe(true)
  })

  it('refreshes a stale profile after reopening and restores both entry points', async () => {
    const staleProfile = { ...profile, label: 'Stale', baseUrl: 'https://stale.example' }
    vi.mocked(tauriInvoke).mockResolvedValueOnce([staleProfile])
    await useAccountProfilesStore().refreshProfiles()
    vi.mocked(tauriInvoke).mockReset()
    vi.mocked(tauriInvoke)
      .mockRejectedValueOnce(new Error('temporary profile failure'))
      .mockResolvedValueOnce([profile])
    const wrapper = mount(SettingsDialog, {
      props: { show: false },
      global: {
        plugins: [pinia],
        stubs: { AppDialog: {
          props: ['show'], template: '<section v-if="show"><slot/><slot name="footer"/></section>',
        } },
      },
    })

    await wrapper.setProps({ show: true })
    await flushPromises()
    expect(wrapper.get('[role="alert"]').text()).toContain('temporary profile failure')
    expect(editorEntryButtons(wrapper)
      .every((button) => button.attributes('disabled') !== undefined))
      .toBe(true)

    await wrapper.setProps({ show: false })
    await wrapper.setProps({ show: true })
    await flushPromises()
    expect(wrapper.find('[role="alert"]').exists()).toBe(false)
    expect(editorEntryButtons(wrapper)
      .every((button) => button.attributes('disabled') === undefined))
      .toBe(true)
    expect(vi.mocked(tauriInvoke).mock.calls
      .filter(([command]) => command === 'list_account_profiles')).toHaveLength(2)

    await editorEntryButtons(wrapper)[0].trigger('click')
    expect(wrapper.getComponent(CredentialEditor).props()).toMatchObject({
      initialLabel: 'Sanitized label', initialBaseUrl: 'https://loaded.example',
    })
  })
})
