import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import PluginImportDialog from '../../src/components/plugins/PluginImportDialog.vue'
import type { ReadyLocalManifestImport } from '../../src/types/plugin'

const readyPreview = {
  schemaVersion: 1,
  status: 'ready',
  token: 'a'.repeat(32),
  expiresInSeconds: 300,
  catalogGeneration: '1',
  manifest: {
    schemaVersion: 1,
    id: 'com.example.notes',
    publisherId: 'com.example',
    publisher: 'Example 作者',
    name: '本地笔记',
    description: '只包含声明式元数据。',
    version: '1.2.3',
    contributions: [],
    requestedCapabilities: [],
  },
} satisfies ReadyLocalManifestImport

let showModalDescriptor: PropertyDescriptor | undefined
let closeDescriptor: PropertyDescriptor | undefined

function restoreDialogMethod(
  name: 'showModal' | 'close',
  descriptor: PropertyDescriptor | undefined,
): void {
  if (descriptor) Object.defineProperty(HTMLDialogElement.prototype, name, descriptor)
  else delete HTMLDialogElement.prototype[name]
}

describe('PluginImportDialog', () => {
  beforeEach(() => {
    showModalDescriptor = Object.getOwnPropertyDescriptor(
      HTMLDialogElement.prototype,
      'showModal',
    )
    closeDescriptor = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, 'close')
    Object.defineProperty(HTMLDialogElement.prototype, 'showModal', {
      configurable: true,
      value(this: HTMLDialogElement) {
        this.setAttribute('open', '')
      },
    })
    Object.defineProperty(HTMLDialogElement.prototype, 'close', {
      configurable: true,
      value(this: HTMLDialogElement) {
        this.removeAttribute('open')
      },
    })
  })

  afterEach(() => {
    restoreDialogMethod('showModal', showModalDescriptor)
    restoreDialogMethod('close', closeDescriptor)
    document.body.replaceChildren()
  })

  it('renders every manifest field as isolated text and cancels rather than confirming on Escape', async () => {
    const wrapper = mount(PluginImportDialog, {
      attachTo: document.body,
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest,
            name: '<img src=x onerror=alert(1)>',
          },
        },
        committing: false,
        stale: false,
      },
    })
    await nextTick()

    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
    const dialog = wrapper.get('dialog')
    expect(dialog.attributes('role')).toBe('dialog')
    expect(dialog.attributes('aria-modal')).toBe('true')
    expect(dialog.attributes('aria-labelledby')).toBe('plugin-import-title')
    expect(dialog.attributes('aria-describedby')).toBe('plugin-import-warning')
    expect(dialog.attributes('open')).toBeDefined()
    expect(wrapper.findAll('dt').map((term) => term.text())).toEqual([
      '名称',
      'ID',
      '版本',
      '描述',
      '发布者',
      '发布者 ID',
    ])
    expect(wrapper.findAll('dd')).toHaveLength(6)
    expect(wrapper.findAll('dd').every((value) => value.find('bdi').exists())).toBe(true)
    expect(wrapper.text()).toContain('预览将在五分钟后失效')

    const cancelEvent = new Event('cancel', { bubbles: false, cancelable: true })
    dialog.element.dispatchEvent(cancelEvent)
    await nextTick()

    expect(cancelEvent.defaultPrevented).toBe(true)
    expect(wrapper.emitted('cancel')).toHaveLength(1)
    expect(wrapper.emitted('confirm')).toBeUndefined()
    wrapper.unmount()
  })

  it('blocks Escape and repeated confirmation while committing', async () => {
    const wrapper = mount(PluginImportDialog, {
      attachTo: document.body,
      props: { preview: readyPreview, committing: false, stale: false },
    })
    await nextTick()
    const confirm = wrapper.get<HTMLButtonElement>('[data-testid="plugin-import-confirm"]')

    await confirm.trigger('click')
    await confirm.trigger('click')
    expect(wrapper.emitted('confirm')).toHaveLength(1)

    await wrapper.setProps({ committing: true })
    expect(confirm.element.disabled).toBe(true)
    expect(wrapper.get('dialog').attributes('aria-busy')).toBe('true')
    expect(wrapper.get('[role="status"]').text()).toContain('正在导入清单')
    const cancelEvent = new Event('cancel', { bubbles: false, cancelable: true })
    wrapper.get('dialog').element.dispatchEvent(cancelEvent)
    await wrapper.get('[data-testid="plugin-import-cancel"]').trigger('click')
    await nextTick()

    expect(cancelEvent.defaultPrevented).toBe(true)
    expect(wrapper.emitted('cancel')).toBeUndefined()
    expect(wrapper.emitted('confirm')).toHaveLength(1)
    wrapper.unmount()
  })

  it('disables and guards confirmation when the preview is stale', async () => {
    const wrapper = mount(PluginImportDialog, {
      attachTo: document.body,
      props: { preview: readyPreview, committing: false, stale: true },
    })
    await nextTick()
    const confirm = wrapper.get<HTMLButtonElement>('[data-testid="plugin-import-confirm"]')

    expect(confirm.element.disabled).toBe(true)
    expect(wrapper.get('[data-testid="plugin-import-stale"]').text())
      .toContain('预览已失效')
    await confirm.trigger('click')
    expect(wrapper.emitted('confirm')).toBeUndefined()
    wrapper.unmount()
  })

  it('focuses cancel first and restores focus only to a connected opener', async () => {
    const opener = document.createElement('button')
    opener.textContent = '导入本地清单'
    document.body.append(opener)
    opener.focus()
    const first = mount(PluginImportDialog, {
      attachTo: document.body,
      props: { preview: readyPreview, committing: false, stale: false },
    })
    await nextTick()

    expect(document.activeElement).toBe(
      first.get('[data-testid="plugin-import-cancel"]').element,
    )
    first.unmount()
    expect(document.activeElement).toBe(opener)

    opener.focus()
    const second = mount(PluginImportDialog, {
      attachTo: document.body,
      props: { preview: readyPreview, committing: false, stale: false },
    })
    await nextTick()
    opener.remove()
    second.unmount()

    expect(document.activeElement).not.toBe(opener)
  })
})
