import { mount } from '@vue/test-utils'
import { defineComponent, h } from 'vue'
import { describe, expect, it, vi } from 'vitest'
import { useChartWorkspaceAutosaveHost } from '../../src/composables/useChartWorkspaceAutosaveHost'

const Harness = defineComponent({
  setup() {
    useChartWorkspaceAutosaveHost()
    return () => h('div')
  },
})

describe('chart workspace autosave host', () => {
  it('invokes browser timer functions with the global receiver', () => {
    const setIntervalSpy = vi.spyOn(globalThis, 'setInterval').mockImplementation(function (
      this: typeof globalThis,
      ...args: Parameters<typeof globalThis.setInterval>
    ) {
      if (this !== globalThis) throw new TypeError('Illegal invocation')
      expect(args[1]).toBe(5_000)
      return 17 as ReturnType<typeof globalThis.setInterval>
    })
    const clearIntervalSpy = vi.spyOn(globalThis, 'clearInterval').mockImplementation(function (
      this: typeof globalThis,
      ...args: Parameters<typeof globalThis.clearInterval>
    ) {
      if (this !== globalThis) throw new TypeError('Illegal invocation')
      expect(args[0]).toBe(17)
    })
    let wrapper: ReturnType<typeof mount> | null = null

    try {
      expect(() => { wrapper = mount(Harness) }).not.toThrow()
      expect(setIntervalSpy).toHaveBeenCalledOnce()

      wrapper?.unmount()
      wrapper = null

      expect(clearIntervalSpy).toHaveBeenCalledOnce()
    } finally {
      wrapper?.unmount()
      setIntervalSpy.mockRestore()
      clearIntervalSpy.mockRestore()
    }
  })
})
