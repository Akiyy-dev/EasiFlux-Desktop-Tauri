import { mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import PluginImportDialog from '../../src/components/plugins/PluginImportDialog.vue'
import { tauriInvoke } from '../../src/composables/useTauriCommand'
import type { ReadyLocalManifestImport } from '../../src/types/plugin'

vi.mock('../../src/composables/useTauriCommand', () => ({ tauriInvoke: vi.fn() }))

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
    vi.mocked(tauriInvoke).mockReset()
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

  it('discloses v4 executable code and the local-only input boundary', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest,
            schemaVersion: 4,
            contributions: [{
              kind: 'command', contributionId: 'analytics.average', title: '计算平均值',
              actionId: 'sandbox.computeSeries',
              params: {
                runtime: 'wasm-v1', abi: 'series-f64-v1',
                moduleBase64: 'AGFzbQEAAAA=',
                parameter: { label: '窗口', default: 3, min: 1, max: 10 },
              },
            }],
          },
        },
        committing: false,
        stale: false,
      },
    })

    expect(wrapper.text()).toContain('包含可执行的本地 WebAssembly 代码')
    expect(wrapper.text()).toContain('只有明确点击运行')
    expect(wrapper.text()).toContain('输入和结果仅保存在内存中')
    expect(wrapper.get('[data-testid="plugin-import-commands"]').text())
      .toContain('运行计算：计算平均值')
    const details = wrapper.get('[data-testid="plugin-import-compute-details"]')
    expect(details.text()).toContain('运行时wasm-v1')
    expect(details.text()).toContain('ABIseries-f64-v1')
    expect(details.text()).toContain('代码模块8 字节')
    expect(details.text()).toContain('参数名称窗口')
    expect(details.text()).toContain('参数默认值3')
    expect(details.text()).toContain('参数范围1 至 10')
    expect(wrapper.text()).toContain('未经认证')
    expect(wrapper.text()).not.toContain('AGFzbQEAAAA=')
    expect(tauriInvoke).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('does not describe host-only v4 contributions as executable code', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest,
            schemaVersion: 4,
            contributions: [{
              kind: 'command', contributionId: 'workspace.charts', title: '打开图表',
              actionId: 'host.openPage', params: { destination: 'charts' },
            }],
          },
        },
        committing: false,
        stale: false,
      },
    })

    expect(wrapper.text()).toContain('不运行代码')
    expect(wrapper.text()).not.toContain('包含可执行的本地 WebAssembly 代码')
    expect(wrapper.get('[data-testid="plugin-import-commands"]').text())
      .toContain('打开页面：图表工作区')
    expect(wrapper.find('[data-testid="plugin-import-compute-details"]').exists()).toBe(false)
    expect(tauriInvoke).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('makes v6 automatic trading and one-time start authority conspicuous', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest,
            schemaVersion: 6,
            requestedCapabilities: ['account.read', 'market.read', 'trade.place', 'strategy.run'],
            contributions: [{
              kind: 'command', contributionId: 'strategy.threshold', title: 'Threshold once',
              actionId: 'sandbox.strategy',
              params: {
                runtime: 'wasm-v1', abi: 'strategy-json-v1', moduleBase64: 'AGFzbQEAAAA=',
                defaultInput: '{"threshold":"50000"}',
              },
            }],
          },
        },
        committing: false,
        stale: false,
      },
    })

    expect(wrapper.text()).toContain('导入、启用、打开或刷新都不会启动策略')
    expect(wrapper.text()).toContain('自动真实交易且不再逐单确认')
    expect(wrapper.get('[data-testid="plugin-import-commands"]').text())
      .toContain('自动交易策略：Threshold once')
    const details = wrapper.get('[data-testid="plugin-import-strategy-details"]')
    expect(details.text()).toContain('strategy-json-v1')
    expect(details.text()).toContain('自动下单或撤单，不逐单确认')
    expect(details.text()).not.toContain('AGFzbQEAAAA=')
    expect(tauriInvoke).not.toHaveBeenCalled()
  })

  it('discloses v5 requested account/trading access and separate session grant and confirmation', () => {
    const wrapper = mount(PluginImportDialog, {
      props: {
        preview: {
          ...readyPreview,
          manifest: {
            ...readyPreview.manifest,
            schemaVersion: 5,
            requestedCapabilities: ['account.read', 'balances.read', 'trade.place'],
            contributions: [{
              kind: 'command', contributionId: 'trader.prepare', title: 'Prepare order',
              actionId: 'sandbox.accountWorkflow',
              params: {
                runtime: 'wasm-v1', abi: 'account-json-v1', moduleBase64: 'AGFzbQEAAAA=',
                defaultInput: '{"qty":"0.001"}',
              },
            }],
          },
        },
        committing: false,
        stale: false,
      },
    })

    expect(wrapper.text()).toContain('请求账户数据或交易提案能力')
    expect(wrapper.text()).toContain('导入和启用都不会授权')
    expect(wrapper.text()).toContain('会话内单独选择')
    expect(wrapper.text()).toContain('真实下单仍需单独确认')
    expect(wrapper.get('[data-testid="plugin-import-commands"]').text())
      .toContain('账户工作流：Prepare order')
    const details = wrapper.get('[data-testid="plugin-import-workflow-details"]')
    expect(details.text()).toContain('account-json-v1')
    expect(details.text()).toContain('{"qty":"0.001"}')
    expect(details.text()).not.toContain('AGFzbQEAAAA=')
    expect(tauriInvoke).not.toHaveBeenCalled()
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
    expect(wrapper.text()).toContain('本地插件包 · 已发现')
    expect(wrapper.text()).toContain('本地插件包 · 外部放置，应用不会删除')
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

  it('keeps exact current and candidate identity visible for an unchanged manifest as text', () => {
    const identicalManifest = {
      ...currentComparisonItem.manifest,
      name: '同名 <strong>笔记</strong>',
      publisher: '作者 <a href="https://example.invalid">链接</a>',
      publisherId: 'com.example.publisher.identity',
      version: '1.0.0+identity',
    } as const
    const preview = {
      ...comparisonPreview,
      manifest: identicalManifest,
      assessment: {
        ...comparisonPreview.assessment,
        current: { ...currentComparisonItem, manifest: identicalManifest },
      },
    } satisfies ReadyLocalManifestImport
    const wrapper = mount(PluginImportDialog, {
      props: { preview, committing: false, stale: false },
    })

    expect(wrapper.get('[data-testid="plugin-manifest-unchanged"]').text())
      .toContain('清单内容未变化')
    expect(wrapper.get('[data-testid="plugin-import-shared-id"]').text())
      .toBe('com.example.notes')
    for (const side of ['current', 'candidate']) {
      expect(wrapper.get(`[data-testid="plugin-import-${side}-identity"]`).findAll('dd')
        .map((value) => value.text())).toEqual([
        '同名 <strong>笔记</strong>',
        '1.0.0+identity',
        '作者 <a href="https://example.invalid">链接</a>',
        'com.example.publisher.identity',
      ])
    }
    expect(wrapper.get('[data-testid="plugin-import-publisher-notice"]').text())
      .toContain('由清单作者填写，未经认证')
    expect(wrapper.find('a').exists()).toBe(false)
    expect(wrapper.find('strong').exists()).toBe(false)
    expect(wrapper.find('[data-testid="plugin-command-button"]').exists()).toBe(false)
    wrapper.unmount()
  })

  it('keeps unchanged identity visible for a same-version command-only change as text', () => {
    const preview = {
      ...comparisonPreview,
      manifest: {
        ...currentComparisonItem.manifest,
        contributions: currentComparisonItem.manifest.contributions.map((command) => (
          command.contributionId === 'workspace.help'
            ? {
                ...command,
                title: '变更 <a href="https://example.invalid">说明</a>',
                params: { ...command.params, text: '候选 <button>运行</button>' },
              }
            : command
        )),
      },
      assessment: {
        ...comparisonPreview.assessment,
        current: currentComparisonItem,
      },
    } satisfies ReadyLocalManifestImport
    const wrapper = mount(PluginImportDialog, {
      props: { preview, committing: false, stale: false },
    })

    expect(wrapper.get('[data-testid="plugin-import-shared-id"]').text())
      .toBe('com.example.notes')
    for (const side of ['current', 'candidate']) {
      expect(wrapper.get(`[data-testid="plugin-import-${side}-identity"]`).findAll('dd')
        .map((value) => value.text())).toEqual([
        '本地笔记',
        '1.0.0+old',
        'Example 作者',
        'com.example',
      ])
    }
    expect(wrapper.get('[data-testid="plugin-import-publisher-notice"]').text())
      .toContain('由清单作者填写，未经认证')
    expect(wrapper.text()).toContain('变更 <a href="https://example.invalid">说明</a>')
    expect(wrapper.text()).toContain('候选 <button>运行</button>')
    expect(wrapper.find('a').exists()).toBe(false)
    expect(wrapper.find('button:not([data-testid])').exists()).toBe(false)
    expect(wrapper.find('[data-testid="plugin-command-button"]').exists()).toBe(false)
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
