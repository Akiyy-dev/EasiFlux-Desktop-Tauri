import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { describe, expect, it } from 'vitest'
import DashboardDevTimeBar from '../../src/components/dashboard/DashboardDevTimeBar.vue'
import { useTimeStore } from '../../src/stores/time'
import { onTimeUpdated } from '../../src/services/timeService'

describe('DashboardDevTimeBar', () => {
  it('renders synced server time details', () => {
    setActivePinia(createPinia())
    onTimeUpdated({
      serverTimeMs: 1_700_000_000_000,
      localTimeMs: 1_700_000_000_050,
      offsetMs: -50,
      syncStatus: 'synced',
      source: 'server',
      lastSyncAt: 1_700_000_000_000,
      lastAttemptAt: 1_700_000_000_000,
      lastError: null,
    })
    useTimeStore().start()
    const wrapper = mount(DashboardDevTimeBar)
    expect(wrapper.text()).toContain('服务器')
    expect(wrapper.text()).toContain('已同步')
  })
})
