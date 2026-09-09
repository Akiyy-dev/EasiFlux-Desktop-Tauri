import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import PluginRemovalDialog from '../../src/components/plugins/PluginRemovalDialog.vue'
import type { PluginCatalogItem } from '../../src/types/plugin'

const plugin: PluginCatalogItem = {
  manifest: {
    schemaVersion: 1,
    id: 'com.example.notes.' + 'x'.repeat(96),
    publisherId: 'com.example',
    publisher: 'Example 作者',
    name: '本地笔记',
    description: '只包含声明式元数据。',
    version: '1.2.3',
    contributions: [],
    requestedCapabilities: [],
  },
  source: 'localDeclarative',
  management: 'managed',
  canRemove: true,
  toggleBlockReasonCode: null,
  grantedCapabilities: [],
  status: 'disabled',
  canToggle: true,
  statusReasonCode: null,
}

let showModalDescriptor: PropertyDescriptor | undefined
let closeDescriptor: PropertyDescriptor | undefined

function restoreDialogMethod(
  name: 'showModal' | 'close',
  descriptor: PropertyDescriptor | undefined,
): void {
  if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor)
  else delete HTMLDialogElement.prototype[name]
}

function mountDialog(overrides: Partial<{
  plugin: PluginCatalogItem
  submitting: boolean
  stale: boolean
  opener: HTMLButtonElement | null
}> = {}) {
  return mount(PluginRemovalDialog, {
    attachTo: document.body,
    props: { plugin, submitting: false, stale: false, ...overrides },
  })
}

describe('PluginRemovalDialog', () => {
  beforeEach(() => {
    showModalDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'showModal')
    closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close')
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
      configurable: true,
      value(this: HTMLDialogElement) { this.setAttribute('open', '') },
    })
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
      configurable: true,
      value(this: HTMLDialogElement) { this.removeAttribute('open') },
    })
  })

  afterEach(() => {
    restoreDialogMethod('showModal', showModalDescriptor)
    restoreDialogMethod('close', closeDescriptor)
    document.body.replaceChildren()
  })

  it('opens an accessible modal with Cancel focused and keeps hostile metadata as bdi text', async () => {
    const hostile = {
      ...plugin,
      manifest: { ...plugin.manifest, name: '<img src=x onerror=alert(1)>' },
    }
    const wrapper = mountDialog({ plugin: hostile })
    await nextTick()

    const dialog = wrapper.get('dialog')
    expect(dialog.attributes()).toMatchObject({
      role: 'dialog',
      'aria-modal': 'true',
      'aria-labelledby': 'plugin-removal-title',
      'aria-describedby': 'plugin-removal-warning',
      open: '',
    })
    expect(document.activeElement).toBe(wrapper.get('[data-testid="plugin-removal-cancel"]').element)
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    expect(wrapper.findAll('dd')).toHaveLength(5)
    expect(wrapper.findAll('dd').every((value) => value.find('bdi').exists())).toBe(true)
    expect(wrapper.text()).toContain(plugin.manifest.id)
    expect(wrapper.text()).toContain('EasiFlux 管理的本地副本将被移除')
    expect(wrapper.text()).toContain('最初选择的源文件不会被修改')
    expect(wrapper.text()).toContain('无法撤销')
    expect(wrapper.text()).toContain('停用偏好将保留')
    wrapper.unmount()
  })

  it('wraps forward and backward Tab through enabled dialog actions', async () => {
    const wrapper = mountDialog()
    await nextTick()
    const dialog = wrapper.get('dialog')
    const cancel = wrapper.get<HTMLButtonElement>('[data-testid="plugin-removal-cancel"]')
    const confirm = wrapper.get<HTMLButtonElement>('[data-testid="plugin-removal-confirm"]')

    confirm.element.focus()
    dialog.element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(cancel.element)
    cancel.element.focus()
    dialog.element.dispatchEvent(new KeyboardEvent('keydown', {
      key: 'Tab', shiftKey: true, bubbles: true, cancelable: true,
    }))
    expect(document.activeElement).toBe(confirm.element)
    wrapper.unmount()
  })

  it('cancels once on Escape and restores only a still-connected opener', async () => {
    const opener = document.createElement('button')
    document.body.append(opener)
    opener.focus()
    const wrapper = mountDialog({ opener })
    await nextTick()
    const cancelEvent = new Event('cancel', { cancelable: true })
    wrapper.get('dialog').element.dispatchEvent(cancelEvent)
    await nextTick()

    expect(cancelEvent.defaultPrevented).toBe(true)
    expect(wrapper.emitted('cancel')).toHaveLength(1)
    wrapper.unmount()
    expect(document.activeElement).toBe(opener)

    const disconnected = document.createElement('button')
    document.body.append(disconnected)
    const second = mountDialog({ opener: disconnected })
    await nextTick()
    disconnected.remove()
    second.unmount()
    expect(document.activeElement).not.toBe(disconnected)
  })

  it('guards stale and double confirmation', async () => {
    const stale = mountDialog({ stale: true })
    await nextTick()
    expect(stale.get<HTMLButtonElement>('[data-testid="plugin-removal-confirm"]').element.disabled).toBe(true)
    expect(stale.get('[data-testid="plugin-removal-stale"]').attributes('role')).toBe('alert')
    await stale.get('[data-testid="plugin-removal-confirm"]').trigger('click')
    expect(stale.emitted('confirm')).toBeUndefined()
    stale.unmount()

    const ready = mountDialog()
    await nextTick()
    const confirm = ready.get('[data-testid="plugin-removal-confirm"]')
    await confirm.trigger('click')
    await confirm.trigger('click')
    expect(ready.emitted('confirm')).toHaveLength(1)
    ready.unmount()
  })

  it('allows cancellation when submission revalidation makes a requested confirmation stale', async () => {
    const wrapper = mountDialog()
    await nextTick()
    await wrapper.get('[data-testid="plugin-removal-confirm"]').trigger('click')
    await wrapper.setProps({ stale: true })
    await wrapper.get('[data-testid="plugin-removal-cancel"]').trigger('click')

    expect(wrapper.emitted('confirm')).toHaveLength(1)
    expect(wrapper.emitted('cancel')).toHaveLength(1)
    wrapper.unmount()
  })

  it('keeps accepted-flight teardown focus-neutral across stale updates and remounts', async () => {
    const opener = document.createElement('button')
    document.body.append(opener)
    opener.focus()
    let nativeReturnTarget = opener
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
      configurable: true,
      value: vi.fn(function(this: HTMLDialogElement) {
        this.removeAttribute('open')
        nativeReturnTarget.focus()
      }),
    })
    const wrapper = mountDialog({ opener })
    await nextTick()
    await wrapper.get('[data-testid="plugin-removal-confirm"]').trigger('click')
    await wrapper.setProps({ submitting: true })

    expect(wrapper.get('dialog').attributes('aria-busy')).toBe('true')
    expect(wrapper.get('[data-testid="plugin-removal-progress"]').attributes()).toMatchObject({
      role: 'status', 'aria-live': 'polite',
    })
    expect(wrapper.findAll<HTMLButtonElement>('button').every((button) => button.element.disabled)).toBe(true)
    const cancelEvent = new Event('cancel', { cancelable: true })
    wrapper.get('dialog').element.dispatchEvent(cancelEvent)
    await wrapper.get('[data-testid="plugin-removal-cancel"]').trigger('click')
    expect(cancelEvent.defaultPrevented).toBe(true)
    expect(wrapper.emitted('cancel')).toBeUndefined()

    await wrapper.setProps({ submitting: false, stale: true })
    document.body.tabIndex = -1
    document.body.focus()
    wrapper.unmount()
    expect(document.activeElement).toBe(document.body)

    const remountOpener = document.createElement('button')
    document.body.append(remountOpener)
    remountOpener.focus()
    nativeReturnTarget = remountOpener
    const remounted = mountDialog({ opener: remountOpener, submitting: true })
    await nextTick()
    document.body.focus()
    remounted.unmount()
    expect(document.activeElement).toBe(document.body)
  })
})
