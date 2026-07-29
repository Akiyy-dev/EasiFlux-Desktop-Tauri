# PRD-08 图表工作区（Chart Workspace）设计

日期：2026-07-29

状态：用户已确认；实施计划已完成，等待执行

工作分支：`chart/createworkspace`

## 1. 需求权威与已确认决策

本规格以用户在本轮提供并逐项确认的要求为权威。已确认：

- 图表模块只显示一个 K 线图表，图表占满模块主内容区。
- 使用当前项目已有的 KLineCharts，不使用 ECharts。
- 保留应用顶部栏和左侧主导航，隐藏图表页二级侧栏。
- 保存范围同时包含 K 线数据、用户绘图对象和图表配置。
- 数据由 Rust 写入应用本地数据目录，不使用 `localStorage`、IndexedDB 或云同步作为权威存储。
- 每 5 秒进行一次脏检查；只有数据发生变化才写盘。
- 切换交易对、周期、页面或正常退出前立即补存。
- 绘图对象和视口按“交易对 + 周期”隔离；指标等通用偏好全局保存。
- 启用 KLineCharts Pro 的完整内置绘图栏。
- 图表工作区与交易页完全共享当前交易对和周期。
- 每个“交易对 + 周期”保留最新 10,000 根 K 线。
- 进入页面时先显示本地数据，再由 REST / WebSocket 去重、补缺和更新。
- 采用“分离存储 + KLineCharts 适配层”方案，不引入 SQLite。

## 2. 当前实现与约束

当前“图表”一级导航仍指向占位内容；交易页已通过 `KlineChart.vue` 使用 `@klinecharts/pro 0.1.1` 和 `klinecharts 9.8.12`。现有实现具备：

- 交易对、周期和实时 K 线的共享 `market` store。
- `EasiKlineDatafeed`、REST 历史数据与 WebSocket 实时更新。
- Rust `KlineStore` 的 JSONL 持久化、时间戳去重和缺口检测。
- 前端指标、布局及视口的 `localStorage` 配置。

需要调整的现状：

- `KlineStore` 当前最多保存 2,000 根，展示链路通常只返回最新 200 根。
- `EasiKlineDatafeed.getHistoryKLineData` 当前忽略 `from` / `to`，无法按范围读取本地 10,000 根历史。
- K 线当前在行情合并时直接写盘，不符合统一的 5 秒脏保存语义。
- KLineCharts Pro 没有公开“导出全部绘图对象”或取得核心图表实例的接口；其 `_chartApi` 实际仅为 Pro 门面，不包含视口、绘图和 action 方法。
- 图表配置当前以浏览器 `localStorage` 为权威存储，无法由 Rust 原子恢复。

升级时允许一次性只读解析旧键 `easiflux.chart-settings.v1`：只在对应 Rust revision 仍为 0 时迁移安全的指标和视口，确认所有目标状态已由 Rust 持久化后才删除旧键。迁移期间不得继续向 `localStorage` 写图表状态；旧布局开关不迁移，因为工作区和交易页的绘图栏模式已固定。

Rust 继续拥有行情、去重、补缺和本地文件业务逻辑；Vue 只负责调用 Commands、监听 Events、驱动 KLineCharts 和收集其可序列化视图状态。

## 3. 目标与非目标

### 3.1 目标

- 提供单图、全尺寸、可绘制的专用图表工作区。
- 复用现有行情链路和 KLineCharts Pro 内置交互。
- 在本地快速恢复 K 线、绘图、指标和视口。
- 在本地存储健康且周期保存成功时，将意外退出的数据损失窗口限制在最近 5 秒。
- 让图表持久化具备明确的数据所有权、版本、并发和损坏恢复语义。

### 3.2 非目标

- 多图表分屏、面板拖拽或多窗口布局。
- 云同步、跨设备共享或服务端图表配置接口。
- SQLite、策略回测或自定义绘图引擎。
- 重写 KLineCharts Pro 绘图栏。
- 保留旧交易图表外置的“紧凑/标准”重建开关；本 PRD 将工作区固定为完整绘图栏、交易页固定为紧凑模式，避免 Pro 0.1.1 无销毁 API 导致的实例泄漏。
- 在本 PRD 内实现 PRD-16 工作区布局、PRD-17 国际化或 PRD-18 主题系统；图表只预留服从这些全局能力的边界。

## 4. 页面和组件边界

### 4.1 页面结构

`AppShell` 在 `activePage === 'charts'` 时挂载独立的 `ChartWorkspacePage`：

- 顶部栏和左侧一级导航继续显示。
- 二级 `Sidebar` 不占据图表页空间。
- 主内容区不使用带标题的 `AppCard`，不显示深度、下单、订单、持仓或分析面板。
- 唯一内容是自动填满可用宽高的 KLineCharts Pro 图表。
- 交易对、周期、指标和绘图栏属于图表内部控件，可以覆盖在图表画布内。

图表容器通过 `ResizeObserver` 或等价的组件级尺寸监听调用核心图表 `resize()`，不依赖固定窗口尺寸。

`@klinecharts/pro 0.1.1` 没有公开销毁 Pro 实例的方法，且内部没有保留 Solid disposer。图表工作区因此使用 `KeepAlive` 或等价的首次挂载后 `v-show` 保活方式保持单一 Pro 实例，在页面离开时停用画布捕获并补存、返回时复用；交易页的 Pro 实例也在首次访问后保活，避免反复挂载泄漏。不得通过反复 `new KLineChartPro()` 和 `replaceChildren()` 应用配置。

### 4.2 前端单元

- `ChartWorkspacePage`：只提供全尺寸页面容器；加载/保存异常沿用全局非阻塞错误提示，不增加常驻标题或面板。
- `KlineChart`：继续作为交易页与图表工作区共享的 KLineCharts 视图入口；通过明确的显示模式决定是否固定展示完整绘图栏。
- `KLineChartsWorkspaceAdapter`：唯一允许访问 KLineCharts Pro 私有核心桥接的模块，负责绘图注册表、快照、恢复和兼容降级。
- `ChartWorkspaceAutosave`：维护前端视图 revision、5 秒脏检查、single-flight 保存和立即补存。
- `market` store：继续共享当前交易对、周期和实时行情；不保存持久化绘图副本或文件业务逻辑。

页面组件不直接拼装 Rust 文件路径，也不解释存储格式。

## 5. KLineCharts 适配层

### 5.1 兼容边界

当前 Pro 类只公开主题、样式、语言、时区、交易对和周期方法。适配层不把 `_chartApi` 误认为核心图表，而是针对锁定版本使用受控的 DOM / 实例注册表桥接：

1. 在 Pro 创建后定位其唯一 `.klinecharts-pro-widget`。
2. 读取 KLineCharts 写入的 `k-line-chart-id`，仅在 widget 没有 DOM `id` 时把该值赋给 `id`；若已有 `id` 与标记不同则在调用 `init` 前降级失败。
3. 调用同一份 `klinecharts.init(widget)` 取回已注册的核心实例，而不是创建第二张图。
4. 校验 `klinecharts.version() === '9.8.12'`、返回实例 ID 与 DOM 标记一致，并校验所需方法。

为保证 Pro 与应用共享同一实例注册表，`klinecharts` 固定为 `9.8.12`，`@klinecharts/pro` 固定为 `0.1.1`，Vite 对 `klinecharts` 启用依赖去重。适配层集中封装该版本敏感逻辑，并在运行时校验：

- `createOverlay`
- `getOverlayById`
- `overrideOverlay`
- `removeOverlay`
- `createIndicator`
- `getIndicatorByPaneId`
- `removeIndicator`
- 后台历史校准所需的受控 `applyNewData`
- 视口读取、恢复和 action 订阅

KLineCharts 9.8.12 的 `visibleRange.to` 是排他上界；保存右端时间戳时读取 `data[visibleRange.to - 1]`，避免恢复位置向右偏一根。

如果桥接缺少必要方法，图表行情仍可显示，但绘图持久化进入不可用状态，并通过现有错误服务报告一次兼容错误；不得因强制类型转换失败导致白屏。

### 5.2 绘图注册表

KLineCharts 9.8.12 只能按 ID 读取绘图，没有公开的“列出全部绘图”方法。适配层因此：

1. 包装核心 `createOverlay`，记录内置绘图栏创建和恢复创建返回的 ID。
2. 包装 `removeOverlay`，同步移除注册表中的失效 ID。
3. 通过 `getOverlayById` 获取每个仍存在且 `currentStep === -1` 的已完成绘图并生成快照；进行中的临时绘图不落盘。
4. 在绘制结束、拖动结束、删除、锁定或样式变化时递增视图 revision。
5. 每次 5 秒检查额外计算规范化快照指纹；即使第三方回调遗漏，也能发现实际状态变化。

恢复时只向 `createOverlay` 传递可序列化字段，并把语义化 pane 引用解析为本次图表实例的 pane ID 后作为第二参数传入；不恢复函数、回调、内部构造器或当前绘制步骤。副图 pane ID 在重启后会重新生成，因此不得直接持久化原始 pane ID；无法解析的指标 pane 降级到主图并记录一次诊断。

## 6. 数据模型

所有跨 Tauri 边界的 Rust / TypeScript 模型使用 camelCase 对齐。

```text
ChartWorkspaceKey {
  symbol: string
  interval: string
}

ChartPaneRef =
  | { kind: 'candle' }
  | { kind: 'indicator', indicatorName: string }

ChartOverlaySnapshot {
  id: string
  groupId: string
  pane: ChartPaneRef
  name: string
  lock: boolean
  visible: boolean
  zLevel: number
  mode: string
  modeSensitivity: number
  points: Array<{ timestamp?: number, dataIndex?: number, value?: number }>
  extendData: JsonValue | null
  styles: JsonValue | null
}

ChartViewportSnapshot {
  barSpace?: number
  rightTimestamp?: number
}

ChartViewStateV1 {
  schemaVersion: 1
  symbol: string
  interval: string
  revision: number
  savedAtMs: number
  overlays: ChartOverlaySnapshot[]
  viewport: ChartViewportSnapshot
}

ChartPreferencesV1 {
  schemaVersion: 1
  revision: number
  savedAtMs: number
  mainIndicators: string[]
  subIndicators: string[]
}

ChartWorkspaceSnapshot {
  key: ChartWorkspaceKey
  klines: Kline[]
  viewState: ChartViewStateV1
  preferences: ChartPreferencesV1
}
```

主题、语言和时区不重复写入 `ChartPreferencesV1`；它们服从应用级配置。Pro 0.1.1 没有恢复绘图栏默认模式的 API，因此不保存绘图栏全局默认模式；每个已创建绘图自身的 `mode` 仍随 `ChartOverlaySnapshot` 完整恢复。当前交易对和周期继续使用现有 `AppConfig` 和 `market` store，图表状态文件只带冗余键用于校验文件与请求是否匹配。

## 7. Tauri Commands 与数据流

### 7.1 Commands

新增专用命令：

```text
load_chart_workspace(symbol, interval, from?, to?, limit?)
  -> ChartWorkspaceSnapshot

save_chart_workspace(request: SaveChartWorkspaceRequest)
  -> ChartWorkspaceSaveResult

set_chart_context(symbol, interval)
  -> Kline[]
```

`load_chart_workspace` 未提供时间范围时返回最新 200 根作为首次渲染窗口；当 datafeed 传入 `from` / `to` 时，从本地 10,000 根集合中按范围返回。`EasiKlineDatafeed` 必须真正使用 KLineCharts 提供的时间范围，而不是继续忽略参数。datafeed 先把已加载的本地范围交给 Pro，REST 在后台补缺；其带键和 generation 的结果由适配器合并当前实时 bar 后受控替换历史并恢复现场视口。

`set_chart_context` 在 Rust 内一次性更新交易对与周期配置/运行时订阅，返回新键的本地最新 200 根；前端只在命令成功后同时提交两个 Pinia 字段，避免中间键和恢复事件竞态。

`save_chart_workspace` 接收视图状态与全局偏好，但不接收前端重复上传的 10,000 根 K 线。Rust 在同一调用中刷新当前键的 K 线脏缓冲，并分别返回 K 线、视图状态和全局偏好的保存结果：

```text
ChartWorkspaceSaveResult {
  key: ChartWorkspaceKey
  viewRevision: number
  preferencesRevision: number
  savedAtMs: number
  klineSaved: boolean
  viewStateSaved: boolean
  preferencesSaved: boolean
  klineError?: string
  viewStateError?: string
  preferencesError?: string
}
```

输入校验或调用协议错误仍返回 `AppError`；单个存储部分失败则返回结构化结果，以便前端只保留失败部分的 dirty 状态。`ChartWorkspaceSaveResult.savedAtMs`、视图状态的 `savedAtMs` 和全局偏好的 `savedAtMs` 均由 Rust 在成功写入时设置，不信任前端时间。

### 7.2 初次进入

1. 读取当前共享交易对和周期。
2. 调用 `load_chart_workspace` 并把本地 K 线预置给 datafeed；Pro 自己执行首次 `applyNewData`。
3. 首个匹配的 `OnDataReady` 先枚举 Pro 已创建的指标 pane，再按保存顺序恢复绘图对象，最后恢复 barSpace 和右端时间戳。
4. 后台启动 REST / WebSocket 刷新；后台全量历史替换另设 data-ready 门闩，只恢复替换前的现场视口。
5. Rust 依据 `openTime` 合并、去重和补缺；网络更新不得覆盖绘图或视口。

### 7.3 切换交易对和周期

1. 为旧键执行立即补存。
2. 再更新共享 `market` store 和 Rust 当前行情订阅。
3. 加载新键的本地快照。
4. 任何旧键的异步结果携带键和 revision；到达时若不匹配当前键，只更新对应待保存记录，不得覆盖当前图表。

## 8. Rust 服务与存储

### 8.1 `ChartWorkspaceService`

服务负责：

- 规范化和校验交易对、周期及 schema。
- 串行化同一视图键的加载、保存和压缩。
- 协调 `KlineStore` 与 `ChartStateStore`。
- 保留各部分最后成功 revision 和保存时间。
- 向 Commands 返回结构化的部分成功结果。

不同视图键可以独立处理；同一键不允许并发写入。

### 8.2 `KlineStore`

- 上限从 2,000 调整为每键最新 10,000 根有效 K 线。
- 内存或缓存合并只以 `openTime` 为唯一键，当前未闭合 K 线可以覆盖同一时间戳记录。
- 行情更新只更新 Rust 脏缓冲，不在每次 WebSocket 更新时重写全部文件。
- 5 秒刷新采用增量 JSONL 日志；尾部半行或单条损坏可被跳过。
- 日志物理记录数超过 12,000 条或唯一记录超过 10,000 条时执行压缩：读取、去重、排序、裁剪，写入临时文件后原子替换正式文件。
- 每次成功刷新都把新增日志同步到操作系统；压缩时同步临时文件后再执行原子替换，以支撑健康存储条件下的 5 秒数据损失上限。
- 应用没有挂载图表组件时，Rust 调度器仍每 5 秒刷新 K 线脏缓冲；图表页保存命令调用同一幂等刷新方法。

### 8.3 `ChartStateStore`

建议目录：应用本地数据目录下的 `chart_workspace/`：

```text
chart_workspace/
  preferences.v1.json
  views/
    BTCUSDT_15.v1.json
    BTCUSDT_60.v1.json
```

文件名只允许由规范化后的 `[A-Z0-9_-]+` 交易对和受支持周期生成。保存顺序为：序列化并校验 -> 写 `.tmp` -> 同步文件 -> 保留上一份 `.bak` -> 原子替换正式文件。任何失败都不得破坏最后成功快照。

## 9. 自动保存和并发语义

- `AppShell` 创建一个应用级自动保存控制器并启动唯一的 5 秒定时器；交易页和图表工作区的保活图表只注册捕获源，不各自创建保存队列。
- 同一时刻只有当前页面的捕获源可读取画布或标记 dirty；全局指标偏好只有一份 revision、待保存负载和确认状态，隐藏图表不得以旧偏好覆盖活动图表。
- 绘图或视口变化递增前端视图 revision；全局偏好变化递增独立的偏好 revision；K 线变化递增 Rust 侧 revision。
- 无 dirty 数据时不调用保存命令、不写文件。
- 保存为 single-flight；一次保存进行中时不启动第二次保存。
- 发起保存时记录快照 revision；成功后只清除不高于该 revision 的 dirty 标记。
- 保存期间出现的新变化保留 dirty，并在下一周期保存。
- 低于或等于已持久化 revision、但内容不同的请求按冲突处理；前端仅在待保存负载仍是原负载时基于返回的持久化 revision 递增后重试，不把冲突误判为成功。
- 交易对、周期、页面和正常窗口关闭前立即补存，不等待下一次定时器。
- 切换时补存失败不锁死导航；旧键进入内存待重试队列，并在下一周期重试。
- 隐藏图表停止读取其画布；应用级控制器继续保留并重试所有已序列化的旧键失败负载，定时重试不因页面隐藏而停止。
- 正常退出时前端先补存视图状态，Rust `RunEvent::Exit` 再刷新剩余 K 线缓冲。
- 崩溃或强制结束不承诺退出钩子；当本地存储健康且周期保存持续成功时，允许最多丢失最近 5 秒尚未持久化的变化。若系统已报告写盘失败，则只保证最后成功快照不被破坏，不承诺 5 秒上限。

## 10. 异常与恢复

- 文件不存在：视为首次使用，返回默认状态，不提示错误。
- 视图 JSON 损坏：读取 `.bak`；备份也无效时恢复默认状态，并只报告一次非阻塞错误。
- K 线日志坏行：跳过坏行，保留其余有效记录，随后由 REST 补缺。
- 未知或不兼容的绘图名称：跳过单个绘图并记录诊断，不阻止页面加载。
- 写盘失败：保留对应 dirty 标记，下一周期自动重试；用户提示必须限频。
- K 线、视图状态或全局偏好任一部分失败：已成功部分不回滚，只重试失败部分。
- 离线：继续展示本地 K 线和绘图，显示连接状态；重新联网后自动校准。
- 路径和 schema 校验失败：拒绝读写，不使用未经校验的字符串拼接文件路径。
- 旧 `localStorage` 迁移解析或写入失败：保留旧键供下次启动重试，不阻止图表使用 Rust 默认/已有状态。

## 11. 测试设计

### 11.1 前端

- 图表页只渲染全尺寸图表，二级侧栏和交易面板不出现。
- 图表工作区与交易页共享交易对和周期。
- 绘图创建、移动、删除、锁定和样式变化生成可恢复快照。
- 快照 round-trip 后绘图、指标和视口一致。
- 5 秒无变化不保存；有变化只发起一次 single-flight 保存。
- 保存期间再次变化不会错误清除较新 revision。
- 两个保活图表共享同一 autosave；隐藏源的回调不会标脏或覆盖活动源的全局偏好。
- 页面、交易对和周期切换立即补存旧键。
- 迟到的旧键加载或保存结果不能覆盖当前图表。
- Pro 私有桥接缺失时安全降级，不出现白屏。
- datafeed 使用 `from` / `to` 请求正确的本地历史范围。

### 11.2 Rust

- K 线按 `openTime` 去重、排序并裁剪为最新 10,000 根。
- 同一时间戳的未闭合 K 线更新覆盖旧值。
- 增量日志压缩前后数据一致，尾部坏行可恢复。
- 视图状态临时文件、原子替换与备份恢复。
- 非法键、非法 schema、未知绘图和损坏文件的降级行为。
- 同一视图并发保存串行化，不同 revision 不互相覆盖。
- 三个保存部分分别返回正确状态，只保留失败部分 dirty。
- 正常退出刷新剩余 K 线缓冲。

### 11.3 回归

- 现有前端测试、TypeScript strict、lint 和生产构建通过。
- Rust 全量测试、格式检查和 Clippy 通过。
- 交易页行情、周期切换、K 线实时更新、下单与账户功能不回退。
- 现有凭据相关未提交修改不纳入 PRD-08 变更范围。

## 12. 验收标准

- 点击“图表”后，模块主内容区只显示一个完整 KLineCharts 图表。
- 顶部栏和左侧主导航保留，图表页不显示二级侧栏。
- KLineCharts Pro 完整内置绘图栏可用。
- 交易页与图表工作区显示相同的当前交易对和周期。
- 本地最多保存每键最新 10,000 根 K 线，并能按时间范围供 datafeed 使用。
- 离线进入时立即恢复本地 K 线、绘图、指标和视口。
- 联网后自动去重、补缺和实时更新，绘图状态不被覆盖。
- 绘图或配置变化后最迟 5 秒写入本地；无变化不产生写盘。
- 切换或正常退出后重新进入，旧视图状态完整恢复。
- 不同交易对和周期的绘图与视口互不串用。
- 文件损坏或部分写盘失败不会破坏最后成功快照，也不会导致图表白屏。
- 本地存储健康且周期保存成功时，强制结束进程的数据损失窗口不超过最近 5 秒；发生已报告的写盘失败时，最后成功快照仍然可恢复。

## 13. 提交边界

设计、实现计划和实现应保持可辨识的 Conventional Commit 边界。建议后续提交方向：

1. `docs(chart): design chart workspace persistence`
2. `docs(chart): plan chart workspace implementation`
3. `feat(chart): add chart workspace persistence service`
4. `feat(chart): add fullscreen chart workspace`

本规格文件在用户明确要求前不提交。实现提交必须排除当前工作区已有的凭据编辑器修改和 `.pnpm-store/`。
