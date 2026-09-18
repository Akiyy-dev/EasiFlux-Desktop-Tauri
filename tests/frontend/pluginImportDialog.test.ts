import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nextTick } from 'vue'
import PluginImportDialog from '../../src/components/plugins/PluginImportDialog.vue'
import type { ReadyLocalManifestImport } from '../../src/types/plugin'

const readyPreview = {
  schemaVersion: 2,
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
  assessment: { kind: 'notInCatalog' },
} satisfies ReadyLocalManifestImport

const currentComparisonItem = {
  manifest: {
    schemaVersion: 3,
    id: 'com.example.notes',
    publisherId: 'com.example',
    publisher: 'Example 作者',
    name: '本地笔记',
    description: '当前描述',
    version: '1.0.0+old',
    contributions: [
      {
        kind: 'command', contributionId: 'workspace.charts', title: '打开图表',
        actionId: 'host.openPage', params: { destination: 'charts' },
      },
      {
        kind: 'command', contributionId: 'workspace.trading', title: '打开交易',
        actionId: 'host.openPage', params: { destination: 'trading' },
      },
      {
        kind: 'command', contributionId: 'workspace.help', title: '查看说明',
        actionId: 'host.showInfo', params: { title: '说明', text: '当前文本' },
      },
    ],
    requestedCapabilities: [],
  },
  source: 'localDeclarative',
  management: 'external',
  canRemove: false,
  toggleBlockReasonCode: null,
  status: 'enabled',
  statusReasonCode: null,
  canToggle: true,
  grantedCapabilities: [],
} as const

const comparisonPreview = {
  ...readyPreview,
  manifest: {
    ...currentComparisonItem.manifest,
    description: '候选描述',
    version: '1.0.0+new',
    contributions: [
      {
        kind: 'command', contributionId: 'workspace.help', title: '查看说明',
        actionId: 'host.showInfo', params: { title: '说明', text: '<img src=x> 候选文本' },
      },
      {
        kind: 'command', contributionId: 'workspace.charts', title: '打开首页',
        actionId: 'host.openPage', params: { destination: 'home' },
      },
      {
        kind: 'command', contributionId: 'workspace.notifications', title: '打开通知',
        actionId: 'host.openPage', params: { destination: 'settings.notifications' },
      },
    ],
  },
  assessment: {
    kind: 'existingId',
    current: currentComparisonItem,
    versionRelation: 'samePrecedence',
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
  it('previews v2 command names as text without executing them', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest, schemaVersion: 2,
            contributions: [{
              kind: 'command', contributionId: 'guide.overview', title: '<b>Guide</b>',
              actionId: 'host.showInfo', params: { title: 'Info', text: 'Read only' },
            }],
          },
        },
        committing: false, stale: false,
      },
    })
    expect(wrapper.get('[data-testid="plugin-import-commands"]').text()).toContain('<b>Guide</b>')
    expect(wrapper.find('b').exists()).toBe(false)
    expect(wrapper.text()).toContain('启用后提供只读命令')
    expect(wrapper.find('[data-testid="plugin-command-button"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('previews v3 actions with host-owned navigation labels', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest, schemaVersion: 3,
            contributions: [
              {
                kind: 'command', contributionId: 'workspace.charts', title: '作者标题',
                actionId: 'host.openPage', params: { destination: 'charts' },
              },
              {
                kind: 'command', contributionId: 'guide.overview', title: '说明',
                actionId: 'host.showInfo', params: { title: 'Info', text: 'Read only' },
              },
            ],
          },
        },
        committing: false, stale: false,
      },
    })

    const preview = wrapper.get('[data-testid="plugin-import-commands"]')
    expect(preview.text()).toContain('打开页面：图表工作区')
    expect(preview.text()).toContain('显示信息：说明')
    expect(wrapper.text()).toContain('由宿主打开白名单页面')
    wrapper.unmount()
  })

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

  it('renders an advisory existing-ID comparison and independently blocks confirmation', async () => {
    const wrapper = mount(PluginImportDialog, {
      attachTo: document.body,
      props: { preview: comparisonPreview, committing: false, stale: false },
    })
    await nextTick()

    expect(wrapper.get('#plugin-import-title').text()).toContain('比较现有插件清单')
    expect(wrapper.get('[data-testid="plugin-import-comparison-notice"]').text())
      .toMatch(/不会.*覆盖.*更新.*回滚/)
    expect(wrapper.text()).toContain('本地声明式包 · 已发现，未执行')
    expect(wrapper.text()).toContain('本地声明式包 · 外部放置，应用不会删除')
    expect(wrapper.text()).toContain('已启用')
    expect(wrapper.text()).toContain('当前描述')
    expect(wrapper.text()).toContain('候选描述')
    expect(wrapper.text()).toContain('1.0.0+old')
    expect(wrapper.text()).toContain('1.0.0+new')
    expect(wrapper.get('[data-testid="plugin-import-version-relation"]').text())
      .toContain('版本优先级相同')
    expect(wrapper.text()).toContain('新增命令')
    expect(wrapper.text()).toContain('移除命令')
    expect(wrapper.text()).toContain('变更命令')
    expect(wrapper.text()).toContain('公共命令的相对顺序已变化')
    expect(wrapper.text()).toContain('打开页面：首页')
    expect(wrapper.text()).toContain('打开页面：通知设置')
    expect(wrapper.text()).toContain('<img src=x> 候选文本')
    expect(wrapper.find('img').exists()).toBe(false)
    expect(wrapper.findAll('details').length).toBeGreaterThan(0)

    const confirm = wrapper.get<HTMLButtonElement>('[data-testid="plugin-import-confirm"]')
    expect(confirm.element.disabled).toBe(true)
    expect(wrapper.get('[data-testid="plugin-import-cancel"]').text()).toBe('关闭')
    confirm.element.disabled = false
    await confirm.trigger('click')
    expect(wrapper.emitted('confirm')).toBeUndefined()
    wrapper.unmount()
  })

  it('states clearly when an existing-ID manifest has unchanged content', () => {
    const preview = {
      ...comparisonPreview,
      manifest: currentComparisonItem.manifest,
      assessment: {
        ...comparisonPreview.assessment,
        current: currentComparisonItem,
      },
    } satisfies ReadyLocalManifestImport
    const wrapper = mount(PluginImportDialog, {
      props: { preview, committing: false, stale: false },
    })

    expect(wrapper.get('[data-testid="plugin-manifest-unchanged"]').text())
      .toContain('清单内容未变化')
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
