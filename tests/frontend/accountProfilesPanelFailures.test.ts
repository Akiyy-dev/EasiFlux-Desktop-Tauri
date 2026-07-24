import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AccountProfilesPanel from '../../src/components/account/AccountProfilesPanel.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import { useAccountProfilesStore } from '../../src/stores/accountProfiles'
import type { AccountProfile } from '../../src/types/models'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail })
  return { promise, resolve, reject }
}

const profiles: AccountProfile[] = [
  {
    accountId: 'primary', label: 'Primary', baseUrl: 'https://primary.example',
    credentialState: 'present', active: true,
  },
  {
    accountId: 'backup', label: 'Backup', baseUrl: 'https://backup.example',
    credentialState: 'present', active: false,
  },
]

const dialogStub = {
  props: ['show'],
  template: '<section v-if="show" data-testid="dialog"><slot/><slot name="footer"/></section>',
}

describe('AccountProfilesPanel mutation failures', () => {
  let pinia: Pinia

  beforeEach(() => {
    pinia = createPinia()
    setActivePinia(pinia)
    vi.mocked(tauriInvoke).mockReset()
  })

  function mountPanel() {
    return mount(AccountProfilesPanel, {
      global: {
        plugins: [pinia],
        stubs: { CredentialEditor: true, AppDialog: dialogStub },
      },
    })
  }

  it('renders account controls and credential state in Chinese', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(profiles)
    const wrapper = mountPanel()
    await flushPromises()

    expect(wrapper.text()).toContain('账户管理')
    expect(wrapper.text()).toContain('添加账户')
    expect(wrapper.text()).toContain('已配置')
    expect(wrapper.text()).toContain('当前账户')
    expect(wrapper.text()).toContain('编辑')
    expect(wrapper.text()).toContain('切换')
    expect(wrapper.text()).toContain('删除')
  })

  it('renders switchError and consumes a rejected switch without changing profiles', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve(profiles)
      if (command === 'switch_account') return Promise.reject(new Error('switch backend failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await flushPromises()
    const before = [...useAccountProfilesStore().profiles]
    const backupRow = wrapper.findAll('li').find((row) => row.text().includes('backup'))
    const switchButton = backupRow!.findAll('button').find((button) => button.text() === '切换')
    await switchButton!.trigger('click')
    await flushPromises()

    expect(wrapper.get('[role="alert"]').text()).toContain('switch backend failed')
    expect(useAccountProfilesStore().switchError).toBe('switch backend failed')
    expect(useAccountProfilesStore().profiles).toEqual(before)
  })

  it('keeps delete confirmation open and renders deleteError after rejection', async () => {
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') return Promise.resolve(profiles)
      if (command === 'delete_account') return Promise.reject(new Error('delete backend failed'))
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await flushPromises()
    const before = [...useAccountProfilesStore().profiles]
    await wrapper.get('[data-testid="delete-backup"]').trigger('click')
    await wrapper.get('[data-testid="confirm-delete"]').trigger('click')
    await flushPromises()

    expect(wrapper.get('[data-testid="dialog"]').exists()).toBe(true)
    expect(wrapper.get('[data-testid="dialog"]').text()).toContain('backup')
    expect(wrapper.get('[role="alert"]').text()).toContain('delete backend failed')
    expect(useAccountProfilesStore().deleteError).toBe('delete backend failed')
    expect(useAccountProfilesStore().profiles).toEqual(before)
  })

  it('closes after a committed delete and disables mutations until list retry succeeds', async () => {
    const pendingRefresh = deferred<AccountProfile[]>()
    let listCall = 0
    vi.mocked(tauriInvoke).mockImplementation((command) => {
      if (command === 'list_account_profiles') {
        listCall += 1
        if (listCall === 1) return Promise.resolve(profiles)
        if (listCall === 2) return pendingRefresh.promise
        return Promise.resolve(profiles)
      }
      if (command === 'delete_account') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await flushPromises()
    await wrapper.get('[data-testid="delete-backup"]').trigger('click')
    await wrapper.get('[data-testid="confirm-delete"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[data-testid="dialog"]').exists()).toBe(false)
    expect(useAccountProfilesStore().deleteError).toBeNull()
    expect(useAccountProfilesStore().loading).toBe(true)
    expect(wrapper.findAll('.actions button')
      .every((button) => button.attributes('disabled') !== undefined)).toBe(true)

    pendingRefresh.reject(new Error('refresh after delete failed'))
    await flushPromises()

    expect(wrapper.get('.account-profiles [role="alert"]').text())
      .toContain('refresh after delete failed')
    expect(wrapper.get('[data-testid="profile-list-retry"]').exists()).toBe(true)
    expect(wrapper.findAll('.actions button')
      .every((button) => button.attributes('disabled') !== undefined)).toBe(true)

    await wrapper.get('[data-testid="profile-list-retry"]').trigger('click')
    await flushPromises()
    expect(useAccountProfilesStore().listError).toBeNull()
    expect(wrapper.find('[data-testid="profile-list-retry"]').exists()).toBe(false)
    expect(wrapper.findAll('.actions button')
      .some((button) => button.attributes('disabled') === undefined)).toBe(true)
  })
})
