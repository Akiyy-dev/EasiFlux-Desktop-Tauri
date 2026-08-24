import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import NotificationSettingsPanel from '../../src/components/settings/NotificationSettingsPanel.vue'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import { useNotificationStore } from '../../src/stores/notification'
import type { NotificationSettings } from '../../src/types/notification'

const enabled: NotificationSettings = {
  tradingToast: true,
  riskAccountToast: true,
  connectionSystemToast: true,
}

const dialogStub = {
  props: ['show'],
  template: '<section v-if="show" data-testid="notification-clear-dialog"><slot /><slot name="footer" /></section>',
}

let pinia: Pinia

function mountPanel() {
  return mount(NotificationSettingsPanel, {
    attachTo: document.body,
    global: {
      plugins: [pinia],
      stubs: { AppDialog: dialogStub },
    },
  })
}

describe('NotificationSettingsPanel', () => {
  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
  })

  afterEach(() => {
    document.body.replaceChildren()
  })

  it('keeps its stable title while a missing draft shows loading or an inline retriable load error without fake switches', async () => {
    const store = useNotificationStore()
    store.settingsSaveStatus = 'loading'
    const load = vi.spyOn(store, 'loadSettings').mockResolvedValue(undefined)
    const wrapper = mountPanel()

    expect(wrapper.get('#notification-settings-title').text()).toBe('通知设置')
    expect(wrapper.findAll('[role="switch"]')).toHaveLength(0)
    expect(wrapper.get('[data-testid="notification-settings-status"]').text()).toContain('正在加载')

    store.settingsSaveStatus = 'error'
    store.settingsError = '加载失败'
    await wrapper.vm.$nextTick()
    expect(wrapper.get('[data-testid="notification-settings-load-error"]').text()).toContain('加载失败')
    await wrapper.get('[data-testid="notification-settings-load-retry"]').trigger('click')
    expect(load).toHaveBeenCalledTimes(2)
  })

  it('renders only the three draft Toast switches and sends a complete latest snapshot for every edit', async () => {
    const store = useNotificationStore()
    store.settingsDraft = { ...enabled }
    store.settingsCommitted = {
      tradingToast: false,
      riskAccountToast: false,
      connectionSystemToast: false,
    }
    const update = vi.spyOn(store, 'updateSettings').mockImplementation(async (snapshot) => {
      store.settingsDraft = { ...snapshot }
    })
    const wrapper = mountPanel()

    const switches = wrapper.findAll('[role="switch"]')
    expect(switches).toHaveLength(3)
    expect(wrapper.get('[data-testid="notification-toggle-trading"]').element).toHaveProperty('checked', true)
    expect(wrapper.get('[data-testid="notification-toggle-risk-account"]').element).toHaveProperty('checked', true)
    expect(wrapper.get('[data-testid="notification-toggle-connection-system"]').element).toHaveProperty('checked', true)

    await wrapper.get('[data-testid="notification-toggle-trading"]').setValue(false)
    await wrapper.get('[data-testid="notification-toggle-risk-account"]').setValue(false)
    expect(update).toHaveBeenNthCalledWith(1, {
      tradingToast: false,
      riskAccountToast: true,
      connectionSystemToast: true,
    })
    expect(update).toHaveBeenNthCalledWith(2, {
      tradingToast: false,
      riskAccountToast: false,
      connectionSystemToast: true,
    })
  })

  it('renders saving and save failure state, retaining the draft and retrying the current complete snapshot', async () => {
    const store = useNotificationStore()
    store.settingsDraft = { ...enabled, riskAccountToast: false }
    store.settingsSaveStatus = 'saving'
    const update = vi.spyOn(store, 'updateSettings').mockResolvedValue(undefined)
    const wrapper = mountPanel()

    expect(wrapper.get('[data-testid="notification-settings-status"]').text()).toContain('正在保存')
    store.settingsSaveStatus = 'error'
    store.settingsError = '保存失败'
    await wrapper.vm.$nextTick()
    expect(wrapper.get('[data-testid="notification-settings-save-error"]').text()).toContain('保存失败')
    expect(wrapper.get('[data-testid="notification-toggle-risk-account"]').element).toHaveProperty('checked', false)
    await wrapper.get('[data-testid="notification-settings-save-retry"]').trigger('click')
    expect(update).toHaveBeenCalledWith({
      tradingToast: true,
      riskAccountToast: false,
      connectionSystemToast: true,
    })
  })

  it('keeps cleanup Global-only when no Store account exists and uses the matching profile only as display text', async () => {
    const notificationStore = useNotificationStore()
    notificationStore.settingsDraft = { ...enabled }
    const clear = vi.spyOn(notificationStore, 'clearCurrentAccount').mockResolvedValue(true)
    const wrapper = mountPanel()

    expect(wrapper.get('[data-testid="notification-clear-current-account"]').attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Global')
    expect(clear).not.toHaveBeenCalled()

    notificationStore.accountId = 'acct-1'
    useAccountProfilesStore().profiles = [{
      accountId: 'acct-1', label: '主账户', baseUrl: 'https://example.test', credentialState: 'valid', active: false,
    }]
    await wrapper.vm.$nextTick()
    expect(wrapper.text()).toContain('主账户（acct-1）')
  })

  it('captures the account target, requires an exact untrimmed case-sensitive token, and focuses the confirmation input', async () => {
    const notificationStore = useNotificationStore()
    notificationStore.settingsDraft = { ...enabled }
    notificationStore.accountId = 'Acct-1'
    useAccountProfilesStore().profiles = [{
      accountId: 'Acct-1', label: '交易账户', baseUrl: 'https://example.test', credentialState: 'valid', active: false,
    }]
    const wrapper = mountPanel()

    await wrapper.get('[data-testid="notification-clear-current-account"]').trigger('click')
    await flushPromises()
    const input = wrapper.get('[data-testid="notification-clear-confirmation-input"]')
    expect(document.activeElement).toBe(input.element)
    expect(wrapper.text()).toContain('交易账户（Acct-1）')
    await input.setValue('acct-1')
    expect(wrapper.get('[data-testid="notification-clear-confirm"]').attributes('disabled')).toBeDefined()
    await input.setValue(' Acct-1')
    expect(wrapper.get('[data-testid="notification-clear-confirm"]').attributes('disabled')).toBeDefined()
    await input.setValue('Acct-1')
    expect(wrapper.get('[data-testid="notification-clear-confirm"]').attributes('disabled')).toBeUndefined()

    await wrapper.get('[data-testid="notification-clear-cancel"]').trigger('click')
    expect(wrapper.find('[data-testid="notification-clear-dialog"]').exists()).toBe(false)
  })

  it('preserves the dialog and list on false, showing only the Store-local clear error before closing on true', async () => {
    const store = useNotificationStore()
    store.settingsDraft = { ...enabled }
    store.accountId = 'acct-1'
    store.items = [{ id: 'visible-item' }] as never
    const clear = vi.spyOn(store, 'clearCurrentAccount')
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
    vi.spyOn(store, 'error', 'get')
      .mockReturnValueOnce('清理失败：后端拒绝')
      .mockReturnValueOnce(null)
    const wrapper = mountPanel()

    await wrapper.get('[data-testid="notification-clear-current-account"]').trigger('click')
    const input = wrapper.get('[data-testid="notification-clear-confirmation-input"]')
    await input.setValue('acct-1')
    await wrapper.get('[data-testid="notification-clear-confirm"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="notification-clear-dialog"]').exists()).toBe(true)
    expect((wrapper.get('[data-testid="notification-clear-confirmation-input"]').element as HTMLInputElement).value).toBe('acct-1')
    expect(store.items).toHaveLength(1)
    expect(wrapper.get('[data-testid="notification-clear-error"]').text()).toBe('清理失败：后端拒绝')

    await wrapper.get('[data-testid="notification-clear-confirm"]').trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid="notification-clear-dialog"]').exists()).toBe(true)
    expect((wrapper.get('[data-testid="notification-clear-confirmation-input"]').element as HTMLInputElement).value).toBe('acct-1')
    expect(store.items).toHaveLength(1)
    expect(wrapper.find('[data-testid="notification-clear-error"]').exists()).toBe(false)

    await wrapper.get('[data-testid="notification-clear-confirm"]').trigger('click')
    await flushPromises()
    expect(clear).toHaveBeenCalledTimes(3)
    expect(wrapper.find('[data-testid="notification-clear-dialog"]').exists()).toBe(false)
  })

  it('does not retarget an open clear dialog after account context changes', async () => {
    const store = useNotificationStore()
    store.settingsDraft = { ...enabled }
    store.accountId = 'acct-1'
    const clear = vi.spyOn(store, 'clearCurrentAccount').mockResolvedValue(true)
    const wrapper = mountPanel()

    await wrapper.get('[data-testid="notification-clear-current-account"]').trigger('click')
    await wrapper.get('[data-testid="notification-clear-confirmation-input"]').setValue('acct-1')
    store.accountId = 'acct-2'
    await wrapper.vm.$nextTick()

    expect(wrapper.text()).toContain('账户上下文已变更')
    expect(wrapper.text()).toContain('acct-1')
    expect(wrapper.get('[data-testid="notification-clear-confirm"]').attributes('disabled')).toBeDefined()
    expect(clear).not.toHaveBeenCalled()
  })

  it('uses the Store pending state to disable another clear request', () => {
    const store = useNotificationStore()
    store.settingsDraft = { ...enabled }
    store.accountId = 'acct-1'
    store.clearCurrentAccountPending = true
    const wrapper = mountPanel()

    expect(wrapper.get('[data-testid="notification-clear-current-account"]').attributes('disabled')).toBeDefined()
  })
})
