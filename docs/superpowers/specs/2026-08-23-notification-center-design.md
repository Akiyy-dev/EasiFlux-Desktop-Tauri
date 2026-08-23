# PRD-13 通知中心（Notification Center）设计

日期：2026-08-23

状态：设计已确认，等待书面规格复核

## 1. 背景与目标

当前顶部栏已经保留通知铃铛，但仍是禁用占位；“设置 → 通知”也只显示规划中。应用现有反馈渠道主要是短暂 Toast 和技术日志：Toast 适合即时操作反馈，但重启后不可追溯；日志面向诊断，频率高、内容技术化，不适合承担用户通知。

PRD-13 建立三层清晰边界：

- Toast：短暂、即时、无需追溯的操作反馈。
- 通知中心：需要用户关注、需要后续查看或可以采取行动的持久事件。
- 日志：高频运行状态、重试、行情与解析诊断等技术信息。

本阶段目标：

- 启用顶部铃铛，并提供不打断交易工作区的通知悬浮面板。
- 让重要订单、风险、账户和连接事件能够持久保存、去重、标记已读和按账户隔离。
- 由 Rust 统一拥有通知事实与持久化，Vue 只负责查询、缓存、Toast 决策和展示。
- 建立可扩展到 PRD-16 多窗口、PRD-17 国际化和未来系统通知的稳定接口，但不提前实现这些能力。
- 避免把日志、通用错误事件或每次网络重试直接转换成通知。

## 2. 已确认的产品决策

- v1 只接收“需要用户关注或可采取行动”的事件，不建设完整活动流，也不只做错误告警箱。
- 首批范围包含订单完全成交、取消、拒绝，风控拦截，账户恢复或对账失败，持续连接故障，以及环境不可达和恢复。
- v1 只提供应用内通知中心和应用内 Toast，不接入 Windows 系统通知。
- 通知按 `accountId` 分区；面板显示“当前账户通知 + 全局通知”，切换账户时列表和角标随之切换。
- 顶部铃铛是唯一主入口，打开约 400px 宽的悬浮面板；v1 不增加独立通知历史页面。
- 打开面板不自动标记已读。点击具体通知后标记已读，并提供“全部已读”。
- 角标显示当前账户与全局通知的未读总数，超过 99 显示 `99+`。
- 通知默认保留 90 天，每个账户最多 1000 条；支持删除单条和清空当前账户通知。
- 设置中心按“交易订单、风险与账户、连接与系统”控制 Toast 是否弹出，但不能关闭通知中心持久化。
- v1 不提供声音和免打扰时段。
- 采用 Rust 通知服务加原子 JSON 持久化；不采用前端本地存储或 SQLite。

## 3. 范围边界

### 3.1 本阶段包含

- Rust 通知领域模型、策略、服务、JSON 存储、Tauri commands 和变更事件。
- 订单、风险、账户和连接生产者的显式接线。
- 账户隔离、全局通知、会话代次校验、幂等去重和状态边沿抑制。
- 90 天/每分区 1000 条保留策略、单条删除、当前账户清空和账户删除清理。
- 顶部铃铛、未读角标、全部/未读筛选、日期分组、50 条游标分页和通知操作。
- 加载、空、失败、重试和存储恢复状态。
- “设置 → 通知”的三类 Toast 开关和当前账户通知清理入口。
- 消息键、结构化参数和中文兜底文本，为后续国际化保留稳定数据结构。
- 前端、Rust、集成回归和桌面人工验收。

### 3.2 本阶段不包含

- Windows、macOS 或 Linux 系统通知及对应权限、后台投递和点击唤醒。
- 声音、免打扰时段、通知优先级规则编辑器或按具体事件逐项配置。
- 独立历史页面、全文搜索、导出、云同步或跨设备已读状态。
- 订单部分成交逐笔提醒、高频行情提醒、价格提醒或完整业务活动流。
- 任意 URL、任意路由、脚本或插件定义的通知动作。
- SQLite、浏览器 `localStorage` 或新的 Tauri 通知插件。
- PRD-16 多窗口 UI；本阶段只让状态接口具备以后同步多个窗口的能力。
- PRD-17 完整语言切换；本阶段只保存可国际化内容描述。

## 4. 方案选择与总体架构

采用“Rust 是唯一可信数据源，Vue 是查询与展示客户端”的架构：

```text
业务状态变化
  -> NotificationPolicy
  -> NotificationService
  -> NotificationStore（先持久化）
  -> notification:changed
  -> Vue Notification Store
  -> 铃铛 / 面板 / Toast
```

Rust 侧职责：

- `NotificationPolicy`：把明确的业务结果映射为通知分类、类型、等级、内容、动作和去重规则。
- `NotificationService`：创建、状态边沿判断、幂等、查询、分页、已读、删除、清空和保留清理的唯一入口。
- `NotificationStore`：加载和原子保存 schema v1 JSON，处理备份、损坏恢复和修订号。
- Tauri commands/events：提供受控操作接口，并在持久化成功后广播状态变化。
- 复用现有 `uuid`、`serde/serde_json`、`dirs` 和 `tokio`；v1 不新增 Cargo/NPM 依赖或 Tauri 插件。

Vue 侧职责：

- Notification Pinia Store：管理当前账户页面缓存、未读数、游标、修订号和事件订阅。
- 通知 service：封装 Tauri command/event 合约，不在组件中直接散落 `invoke` 与 `listen`。
- UI 组件：只渲染 Store 状态并触发语义操作。
- Toast 决策：仅针对实时创建事件，根据全局通知设置决定是否弹出；重新加载历史记录不会补弹 Toast。

未采用方案：

- Vue Pinia 加 WebView 本地存储：实现较快，但后端事件需要绕到前端才能落盘，多窗口和账户切换容易产生竞态，WebView 数据清理也可能造成丢失。
- Rust 加 SQLite：查询扩展能力强，但 90 天、每分区 1000 条的规模不需要数据库、索引和迁移成本，也会重新引入已经移除的新闻存储依赖方向。

## 5. 通知领域模型

### 5.1 持久记录

```text
NotificationRecord {
  id: UUID v4
  scope: Global | Account { accountId }
  category: Trading | RiskAccount | ConnectionSystem
  kind: NotificationKind
  severity: Success | Info | Warning | Error | Critical
  content: {
    messageKey: string
    params: map<string, scalar>
    fallbackTitle: string
    fallbackBody: string
  }
  entity: { type, id } | null
  action: NotificationAction | null
  sourceEventId: string | null
  dedupeKey: string
  occurrenceCount: integer >= 1
  createdAtMs: UTC epoch milliseconds
  updatedAtMs: UTC epoch milliseconds
  readAtMs: UTC epoch milliseconds | null
}
```

约束：

- `id` 由 Rust 生成，前端不得指定。
- `params` 只允许已定义的字符串、布尔或有限数值字段，不接受任意嵌套 JSON。
- `fallbackTitle` 和 `fallbackBody` 是当前中文界面的可靠兜底；PRD-17 可以使用 `messageKey + params` 重新渲染历史通知。
- `fallbackBody` 不保存密钥、令牌、Secret、完整服务端响应、请求签名、任意堆栈或未经脱敏的错误文本。
- `createdAtMs` 决定稳定排序和分页；语义相同但来源不同的低频事件可以合并并更新 `updatedAtMs` 与 `occurrenceCount`，不能改变 `createdAtMs`。
- `sourceEventId` 表示上游事件或本次业务尝试的稳定 ID。相同 `sourceEventId` 的重复投递是完全幂等的：不修改记录、不增加修订号、不发送事件。
- 幂等索引按 `(scope, sourceEventId)` 计算；语义合并按 `(scope, dedupeKey)` 计算，任何账户都不能命中或更新另一账户的记录。
- 新通知默认 `readAtMs = null`。语义合并不会重置已读状态，也不会再次 Toast。连接重试在进入服务前过滤，不通过高频更新 `occurrenceCount` 刷盘。

### 5.2 动作模型

`NotificationAction` 使用封闭枚举，不保存 URL 或字符串路由：

```text
OpenTrading { orderId? }
OpenAccountSettings { accountSection }
OpenGeneralSettings
```

只有现有导航能够真实处理目标时才附加动作。订单动作至少进入交易页；若当时无法定位具体订单，仍展示通知内容并停留在交易页。连接事件在没有真实状态页面或安全重连入口时不附加假动作。

点击通知时先请求标记已读，再执行白名单导航。标记失败不得阻断业务导航，但必须保留未读状态并给出非阻塞错误。

## 6. 首批事件与策略

| 分类 | 通知类型 | 等级 | 创建条件 | 去重与动作 |
| --- | --- | --- | --- | --- |
| 交易订单 | `order.filled` | Success | 订单进入完全成交终态 | `accountId + orderId + filled`；打开交易页 |
| 交易订单 | `order.canceled` | Info | 订单进入取消终态 | `accountId + orderId + canceled`；打开交易页 |
| 交易订单 | `order.rejected` | Error | 下单被交易端最终拒绝 | `accountId + submissionId + rejected`；存在订单 ID 时作为实体引用；打开交易页 |
| 风险与账户 | `risk.order_blocked` | Warning | 本地下单流程最终被风控拒绝 | `accountId + submissionId + violationCode`；打开风险设置 |
| 风险与账户 | `account.session_expired` | Warning | 活动账户凭据或会话确认失效 | 每个账户、每次有效会话边沿一次；打开账户 API 设置 |
| 风险与账户 | `account.recovery_failed` | Error | 账户恢复流程最终失败 | `accountId + attemptId + recovery`；打开账户设置 |
| 风险与账户 | `account.reconciliation_failed` | Critical | 对账流程最终失败并需要人工检查 | `accountId + attemptId + reconciliation`；打开账户设置 |
| 连接与系统 | `connection.unavailable` | Error | API 或私有 WebSocket 通道进入对用户可见的不可用状态 | `accountId + channel(api/websocket)`，每个通道状态边沿一次 |
| 连接与系统 | `connection.recovered` | Success | 对应 API 或私有 WebSocket 通道恢复 | `accountId + channel + incidentId`，每次故障周期一次 |
| 连接与系统 | `environment.unavailable` | Error | 当前账户所配置环境确认不可达 | `accountId + environment`，每个环境状态边沿一次 |
| 连接与系统 | `environment.recovered` | Success | 对应环境恢复 | `accountId + environment + incidentId`，每次故障周期一次 |

策略要求：

- “持续连接故障”沿用现有连接状态机对外暴露的不可用状态；不为本 PRD 新增任意秒数阈值。API、私有 WebSocket 和环境探测是三个独立通道，某一通道恢复不能关闭另一通道的故障周期。
- 网络重试、短暂 WebSocket 抖动、行情流断续、解析失败、快照、轮询和诊断信息继续只写日志。
- 连接事件按状态边沿创建。相同状态的重复回调只增加内部幂等命中，不产生新记录；恢复后再次不可用属于新的故障周期。
- 状态观察器在加载通知文件后从每个账户/通道最近的故障边沿恢复 active incident。应用初始即为健康且没有未关闭故障时不创建“已恢复”；若上次退出前存在未恢复故障，则本次健康探测可以关闭该 incident 并创建一次恢复通知。
- `place_order` 命令在进入业务流程时由 Rust 生成 `submissionId`，并把它贯穿风控和 API 提交流程。风控结果改为受控 `RiskViolation { code, safeParams }`，不得再从自然语言错误反推规则；API 拒绝即使没有订单 ID，也使用 `submissionId` 关联和去重。
- 订单通知通过独立的 `observe_order(accountId, sessionEpoch, order, origin)` 语义入口产生，`origin` 是 `Command | Realtime | Snapshot`。启动和轮询快照只用于播种/更新上一个状态，不创建通知；命令确认的终态或私有实时流的 `non-final -> final` 转换才创建。异常的终态到另一终态转换只写诊断，不产生第二条通知。
- 现有 `order:updated` 继续用于界面状态同步，不能直接作为通知生产源。部分成交更新、请求发送中和客户端乐观状态不创建持久通知。
- 通用 `error:occurred`、`log:entry` 和 Toast 事件不自动转换成通知；生产者必须显式调用通知策略。
- 一个逻辑错误最多产生一条诊断日志和一条符合策略的通知。Rust 已写日志的错误到达前端时不得再次追加同一条日志。
- v1 的 `connection.*` 和 `environment.*` 都是 Account scope。Global 分区和协议在 v1 保留，但首批事件没有 Global 生产者。

## 7. 账户与会话边界

- 所有账户通知生产者必须提供明确的 `accountId`。现有只携带 `sessionEpoch` 的事件需要补齐账户身份，不能由前端读取“当前账户”后猜测归属。
- 账户源事件同时携带 `sessionEpoch`。服务在持久化前对照活动会话代次；旧代次事件直接丢弃，只记录必要诊断，不创建通知或 Toast。
- 账户恢复和对账目前由前端账户 Store 编排，因此使用唯一例外的封闭客户端桥接：前端生成本次 `attemptId`，提交 `accountId`、事件开始时捕获的 `sessionEpoch` 和受控失败步骤枚举；Rust 校验账户与代次后派生消息、等级、动作和去重键。请求不得包含展示文本或原始错误。
- 面板查询始终合并当前账户分区和全局分区，并按 `createdAtMs DESC, id DESC` 排序。
- 全局通知的已读状态也是全局状态；在任一账户上下文标记后，不再计入其他账户的全局未读数。
- 切换账户时前端立即清空旧页面缓存并增加加载代次。旧请求即使晚到也不能写入新账户 Store。
- 普通账户切换不删除任何通知。
- 删除账户成功后，账户生命周期协调器在同一 mutation guard 内等待一次通知分区清理。通知数据属于非关键缓存：清理失败不得回滚已经完成的账户删除；`delete_account` 改为返回 `DeleteAccountResult { notificationCleanupPending, warningCode? }`，其中唯一警告码是 `NOTIFICATION_CLEANUP_PENDING`。同时写入脱敏日志，并在下次启动时通过“现有账户集合”清理孤儿 Account 分区，绝不把 Global 分区当作孤儿删除。
- `clear_account_notifications` 只清空指定当前账户分区，不删除全局通知；全局通知仍可逐条删除。

## 8. Tauri 命令与事件契约

### 8.1 查询与操作命令

```text
list_notifications {
  accountId?,
  filter: All | Unread,
  cursor?: opaque string,
  limit
}
  -> NotificationPage {
       items,
       nextCursor?,
       unreadCount,
       revision: decimal string
     }

get_notification_summary { accountId? }
  -> { unreadCount, revision: decimal string }

mark_notification_read { contextAccountId?, id }
  -> { notification, unreadCount, revision: decimal string }

mark_visible_notifications_read { contextAccountId? }
  -> { affectedCount, affectedScopes, unreadCount: 0, revision: decimal string }

delete_notification { contextAccountId?, id }
  -> { unreadCount, revision: decimal string }

clear_account_notifications { accountId }
  -> { affectedCount, unreadCount, revision: decimal string }

create_client_notification {
  accountId,
  sessionEpoch,
  attemptId,
  kind: AccountRecoveryFailed | AccountReconciliationFailed,
  failedSteps: (Config | Profiles | Connection | Bootstrap)[]
}
  -> { notification, unreadCount, revision: decimal string }
```

约束：

- `limit` 默认 50，最小 1，最大 100。
- 游标由服务返回并包含版本、账户/全局组合、筛选和最后一条排序键的查询指纹；前端把它当作不透明字符串，不能跨账户或筛选复用。
- 查询未提供 `accountId` 时只返回 Global 分区，支持尚未建立活动账户时打开面板。
- 修改命令未提供 `contextAccountId` 时只允许操作 Global；如果后端当前存在活动账户而请求省略上下文，仍按 Global-only 处理，不能隐式扩大到活动账户分区。
- `mark_visible_notifications_read` 作用于当前视图的账户分区和 Global 分区，因此成功后该视图未读数为 0，并显式返回受影响作用域。
- `clear_account_notifications` 需要界面显式确认，但命令仍校验账户存在性和当前账户上下文。
- 单条已读和删除请求携带发起操作时的账户上下文。后端按通知 ID 解析真实作用域，并校验该记录必须是 Global 或属于当前活动且与请求一致的账户；旧账户晚到请求返回稳定错误码 `NOTIFICATION_SCOPE_MISMATCH`，不能修改其他账户分区。
- `create_client_notification` 只允许账户恢复和对账失败两个前端编排事件。Rust 校验 `attemptId`、活动账户、`sessionEpoch` 和失败步骤枚举，并负责所有展示字段；不得借此增加新事件类型或提交任意标题、正文、等级、动作和错误文本。
- 磁盘中的修订号使用 Rust `u64`，所有 IPC 返回和事件都编码为十进制字符串，前端把它作为不透明版本值处理，避免超过 JavaScript 安全整数后失真。
- 会创建持久通知的失败命令返回结构化错误 `{ code, message, notificationId? }`。存在 `notificationId` 时，调用者可以显示行内错误，但不得再次调用全局 `reportError`；该失败的一条后端日志由 Rust 拥有，是否 Toast 由对应 Created 事件和分类设置决定。不得通过匹配错误文案判断是否已经通知。

### 8.2 变更事件

统一广播一个事件：

```text
notification:changed {
  previousRevision: decimal string,
  revision: decimal string,
  change: Created | Updated | Removed | Reset,
  affectedScopes: NotificationScope[],
  notificationId?
  toastCandidate?: {
    id,
    scope,
    sessionEpoch?,
    category,
    severity,
    content,
    action?
  }
}
```

- `NotificationScope` 使用判别联合 `{ type: "global" } | { type: "account", accountId }`，Rust 与 TypeScript 使用同一 camelCase 序列化形状。
- 服务在 copy-on-write 副本中计算 `nextRevision` 并把它随新文件一起原子持久化；成功后才替换内存状态，并发送相同的 `previousRevision/nextRevision`。失败时磁盘事实、内存修订号和事件均不变化。
- 事件只提示状态变化，JSON 存储和命令返回始终是权威状态。
- `toastCandidate` 只出现在持久化成功后的 `Created`，内容受控且已脱敏。非 Created 事件不携带 Toast 载荷。
- 当前账户或 Global 作用域受影响时，Vue Store 处理列表变化；其他账户事件不污染当前列表，但连续收到时仍推进已观察的全局修订号。
- 连续性的判断使用 `event.previousRevision === store.observedRevision`，不需要把字符串转成 Number。发现不相等、收到 `Reset`、账户切换或存储恢复时，必须丢弃项目与游标并从第一页重新查询，不能只重查当前页。
- Store 只对实时收到、作用域是当前账户或 Global、尚未存在于进程内 `seenCreatedIds`、并且 Toast 设置已经加载的 `toastCandidate` 决策 Toast。启动加载、历史分页、修订号重载和备份恢复都不得补弹旧通知。
- Notification Store 暴露幂等 `start()/stop()`；App 只初始化一次，退出、热重载或 Store 销毁时执行 unlisten，防止多次 Toast。

## 9. JSON 持久化与恢复

### 9.1 文件结构

通知数据复用现有图表状态的根目录规则，位于 `dirs::data_local_dir()/APP_NAME` 下：

```text
notifications/notifications.v1.json
notifications/notifications.v1.json.tmp
notifications/notifications.v1.json.bak
```

主文件逻辑结构：

```text
NotificationFileV1 {
  schemaVersion: 1
  revision: u64
  partitions: [
    { scope: Global | Account { accountId }, items: NotificationRecord[] }
  ]
}
```

使用显式分区数组而不是把账户 ID 当作 JSON 对象键，避免保留特殊键和转义语义。加载后可以在内存中构建按作用域索引。

### 9.2 原子写入

- 服务使用单一异步互斥锁串行化所有读改写操作。
- 修改采用 copy-on-write：在内存副本中校验、修改、清理、计算 `nextRevision` 并序列化，包含新修订号的文件写盘成功后才替换服务内存状态。
- 写入临时文件并完成刷盘后，按仓库现有配置/图表状态存储模式轮换主文件与 `.bak`，最后原子替换主文件。
- 写入失败时保证内存快照、对外修订号和事件不提交；主文件已经轮换但临时文件提升失败时，尽力把有效备份恢复为主文件并记录告警。极端磁盘故障下不承诺主路径一定存在；恢复逻辑必须检查 `main/tmp/bak` 的剩余候选，而不是假定主文件仍在。
- 每次成功领域修改全局递增一次 `revision`。纯查询、相同 `sourceEventId` 幂等命中和不产生实际删除的保留检查不递增修订号。

### 9.3 启动恢复与保留

- 启动时按 `main -> tmp -> bak` 顺序读取第一个严格校验通过的 v1 候选，和现有 `ChartStateStore` 恢复语义一致。若从 tmp 或 bak 恢复，使用不改变 revision 的原子规范化写入恢复主文件；规范化失败时仍可从已加载候选提供本次运行状态，并在下一次修改时重试。
- 校验至少包含：schema、分区作用域唯一、Global 分区最多一个、账户 ID 规范化且非空、全文件通知 ID 唯一、受控枚举、有限参数、`occurrenceCount` 位于 `1..=u32::MAX`、`updatedAtMs >= createdAtMs`、`readAtMs >= createdAtMs` 和时间戳不超过 JavaScript 安全整数。
- 只要更高优先级候选可解析且 `schemaVersion > 1`，就停止向旧候选降级。该文件属于“不支持的新版本”，不是损坏文件；应用继续启动，但通知中心进入不可用恢复态，并且当前版本不得覆盖、降级或改名该文件。
- 所有 v1 候选都损坏时，把不可解析文件保留为带时间戳的 `.corrupt-*` 排查副本，以空通知箱启动。通知不是应用启动的硬依赖；该恢复过程不发送 Created 或补弹 Toast。
- 存储目录不存在时按需创建；首次无文件是正常空状态，不报告错误。
- 统一 `prune(now)` 在启动、每次创建和每 24 小时的低频维护任务中执行；查询与摘要还必须在内存中过滤已经过期但尚未完成持久清理的记录。实际清理发生时只做一次原子写、递增一次 revision 并发送 `Reset`。
- `prune` 删除超过 90 天的项目，并保证每个账户分区最多 1000 条；Global 分区同样最多 1000 条。维护任务随 AppState 启动并在应用退出时停止。
- 清理顺序按 `createdAtMs` 最旧优先；已读与未读使用相同保留规则，通知中心不是审计日志。
- 持久文件不加密，但只能包含已脱敏通知数据。凭据与密钥继续由系统 Keyring 管理。

## 10. 查询、分页与同步

- 首次打开面板加载最新 50 条；滚动接近底部且存在 `nextCursor` 时再加载下一页。
- 服务内部使用 `(createdAtMs, id)` 降序 keyset 分页，对外编码为包含查询指纹的不透明游标；新通知插入顶部不会造成正常连续分页重复或漏项。
- “全部”和“未读”是独立查询，切换筛选时重置项目、游标和加载状态，但保留同一摘要修订号用于校准。
- 未读数由后端对当前账户与全局分区完整计算，不根据已加载页面推断。
- 全部已读、删除和清空在 Rust 完成后返回新的未读数和修订号，前端以响应校准状态。
- 前端不对已读、删除、清空和批量已读做不可逆的乐观更新；命令在途可以显示局部 pending，只有成功响应才更新，失败时保留旧列表。
- 面板关闭时可以保留当前账户缓存；账户切换、筛选变化、修订号跳跃、`Reset`、清空和存储恢复后必须清空 items/cursor/hasMore 并从第一页重新查询。
- 该修订号协议为 PRD-16 多窗口保留同步基础，但 v1 不创建第二个窗口，也不处理多个窗口重复 Toast 的产品策略。

## 11. 前端组件与交互

### 11.1 顶部铃铛

- 启用 `TopBar` 现有 Bell 按钮，作为唯一主入口。
- `TopBar` 挂载并锚定 `NotificationPopover`，只持有局部 `open` 状态；它不调用 Tauri 命令、不解释通知类型，也不直接修改页面导航。
- `NotificationPopover` 向上发送受控 `navigate(NotificationAction)` 和 `close`；`TopBar` 继续转发，`AppShell` 是唯一把动作映射为现有 `NavigationTarget` 并调用 `navigateTo` 的所有者。
- 无未读时不显示数字角标；1–99 显示真实数字；超过 99 显示 `99+`。
- 按钮的可访问名称包含未读数量，装饰角标对屏幕阅读器隐藏。
- 当前账户尚未建立或摘要加载失败时，铃铛仍可打开面板；面板展示全局通知或明确错误状态，不伪造 0 未读。

### 11.2 悬浮面板

- 使用现有 Naive UI 悬浮层能力和设计令牌，锚定顶部铃铛，桌面宽度约 400px。
- 小窗口宽度限制为 `calc(100vw - 24px)`；最大高度不超过标题栏以下可用区域，列表独立滚动。
- 标题栏包含“通知”和“全部已读”。下方只提供“全部 / 未读 N”两个筛选。
- 列表按“今天 / 更早”分组；项目显示状态图标、标题、正文、分类、相对时间和可选动作。
- 未读状态同时使用语义、字重或标记表达，不能只依赖颜色。
- 单条删除按钮在鼠标悬停或键盘聚焦时出现，不需要确认；清空当前账户放在通知设置中并要求显式确认。
- 每个 `NotificationItem` 使用一个 `article` 容器，主内容是独立 primary button，删除是与其并列的 sibling button；不得把按钮嵌套在另一个按钮中。主按钮负责“标记已读并执行可选动作”，删除按钮只删除。
- 面板底部说明当前显示“当前账户与全局通知”，并提供“通知设置”深链。该操作先关闭面板，再发送对象形式 `{ page: 'settings', settingsSection: 'notifications' }`，不能只发送字符串 `'settings'`；导航完成后把焦点交给通知设置标题。

### 11.3 状态与键盘

- 首次加载使用与列表结构一致的骨架屏。
- 空状态区分“暂无通知”和“未读通知为空”，不显示假操作。
- 加载下一页失败时保留已有列表，在列表底部提供重试；首次加载失败显示错误和重试。
- 面板打开不自动已读。点击项目主按钮、按 Enter/Space 或显式动作才标记该项目已读。
- 只有焦点位于项目主按钮且不在输入控件或删除按钮时，Delete 才删除该项目；Escape 和点击外部关闭面板，关闭后焦点返回铃铛。
- 面板使用 `aria-labelledby` 和 `aria-modal="false"` 的非模态 dialog/popover 语义，所有操作可 Tab 到达，焦点环清晰。
- 新未读计数使用非打断式 `aria-live="polite"`；Toast 不抢夺当前输入焦点。

### 11.4 初始化与 Toast 适配器

- Notification Store 在 App 生命周期中先注册一次事件监听，再读取当前账户、Toast 设置和通知摘要；初始化过程中收到的事件用于失效/重查，但在账户和最近成功设置就绪前不弹 Toast。重要事件仍已持久化，不通过补弹历史 Toast 制造启动噪声。
- 监听回调在收到事件时立即捕获该事件的 scope、当前账户快照和 load generation；异步处理完成时不能重新读取并误用已经变化的当前账户。
- 持久通知 Toast 使用独立 `notificationToastService`，共享现有 `NMessageProvider`，但不写日志，也不调用创建通知命令。它支持 Success/Info/Warning/Error/Critical 到现有消息等级的确定映射。
- `seenCreatedIds` 只存在于当前应用进程，用于抵御重复监听和同一 Created 重放；它不是持久已读状态。

## 12. 通知设置

“设置 → 通知”从占位页升级为真实设置面板，提供三个全局开关：

- 交易订单 Toast
- 风险与账户 Toast
- 连接与系统 Toast

前后端使用独立窄模型和命令：

```text
NotificationSettings {
  tradingToast: boolean
  riskAccountToast: boolean
  connectionSystemToast: boolean
}

get_notification_settings -> NotificationSettings
update_notification_settings { NotificationSettings } -> NotificationSettings
```

设置语义：

- 三个开关默认开启，跨账户生效。
- Rust 在现有配置持久化中增加带 `serde(default)` 的通知设置字段，旧配置加载时得到全开启默认值；前端通过独立类型和命令读取，不整体回写或暴露完整 `AppConfig`。
- 开关只控制新通知是否弹出应用内 Toast，不影响通知创建、持久化、未读计数或列表展示。
- 即使 Critical 分类的 Toast 被关闭，记录仍必须进入通知中心。
- 采用设置中心现有的自动保存、保存中、失败和重试交互，不增加页面级“保存”按钮。
- 使用窄范围 `update_notification_settings` 命令，只更新通知偏好，不能提交完整 `AppConfig` 或覆盖其他设置。
- 更新命令获取现有配置/账户生命周期写协调，从最新运行态配置克隆并只替换 `NotificationSettings`；原子持久化成功后才更新运行态和前端最后成功快照，防止与账户、通用或风控配置并发时丢字段。
- 设置保存失败保留草稿与最近成功快照；在后端确认保存之前，实时 Toast 决策继续使用最后成功配置。
- 面板包含“清空当前账户通知”危险操作，显示具体账户并二次确认；该操作不清空全局通知。
- `NotificationSettingsPanel` 通过 Notification Store 执行清空并消费命令返回/修订事件，不直接修改 Popover 的页面缓存；现有设置侧栏及 `settings-nav-notifications` 测试定位保持不变。
- v1 不展示不可用的声音、系统通知或免打扰假开关。

## 13. 错误处理与恢复语义

| 失败位置 | 用户可见行为 | 状态保证 |
| --- | --- | --- |
| 创建通知持久化失败 | 按稳定错误码限频显示错误 Toast，并写脱敏日志 | 不发送 `notification:changed`，不修改内存事实，不递归创建存储故障通知 |
| 查询或首次加载失败 | 面板显示错误与重试；角标不伪造为 0 | 保留最近成功缓存并标记为可能过期 |
| 加载下一页失败 | 保留已加载项目，页尾重试 | 游标不前移 |
| 标记已读失败 | 显示非阻塞提示；业务跳转仍可继续 | 项目保持未读，后续重查对账 |
| 删除或清空失败 | 保留原列表并提示重试 | 不做不可逆乐观删除 |
| 变更事件丢失 | 修订号跳跃后重新查询 | 命令与 JSON 状态为权威来源 |
| 主文件损坏 | 按 main/tmp/bak 恢复并记录警告 | 恢复有效候选，或保留损坏副本后空箱启动；未来 schema 不覆盖 |
| 通知设置保存失败 | 行内错误与重试 | Toast 决策继续使用最后成功设置 |
| 白名单动作目标暂不可用 | 通知仍标记为已读，显示轻量提示 | 不执行字符串路由或任意副作用 |

通知存储失败使用稳定错误码 `NOTIFICATION_STORAGE_UNAVAILABLE`，错误 Toast 以该码在 60 秒窗口内最多显示一次；任一后续通知写入成功后重置限频状态。限频只影响 Toast，不吞掉脱敏诊断日志。

错误渠道迁移规则：

- 通知策略不监听通用日志和错误总线；业务生产者只在最终状态点显式创建通知。
- Rust 后台错误的 `error:occurred` 与对应 `log:entry` 共享同一 `eventId`。App 的后台错误监听只调用“显示 Toast、不写日志”的适配器，并用 `eventId` 抑制事件重放。
- 命令调用失败由 invoke 调用者使用现有 `reportError` 负责一条前端日志和一个 Toast；同一失败的 Rust 路径不得再调用后台 `emit_error`。同时服务于命令和后台任务的代码必须接收明确的错误报告所有权上下文，不能两边都报告。
- 通知感知命令是上一规则的显式例外：结构化错误带 `notificationId` 时，Rust 已拥有日志且 Created 事件拥有可配置 Toast，前端只显示行内错误。账户恢复/对账桥接成功后也不得再为同一聚合失败调用通用错误 Toast。
- 前端本地错误继续使用 `reportError`；持久通知 Toast 使用 `notificationToastService`，它不写日志。
- 以上迁移只统一错误投递所有权，不把普通错误自动升级为持久通知。

## 14. 测试设计

### 14.1 Rust 单元与服务测试

- 每个首批事件到分类、类型、等级、消息键、参数和动作的确定映射。
- 不允许的事件类型、任意动作、非有限数值、未脱敏字段和错误作用域被拒绝。
- Rust `submissionId`、结构化 `RiskViolation`，以及订单观察器对 Command/Realtime/Snapshot 来源和 `non-final -> final` 的处理。
- 订单终态幂等、连接各通道状态边沿、故障恢复周期、相同 source ID 完全幂等，以及语义合并不重置已读或再次 Toast。
- 账户恢复/对账客户端桥接只接受两个 kind、受控失败步骤、活动账户和有效 epoch；展示文本与动作不能由前端提交。
- 过期 `sessionEpoch` 不创建记录；账户身份不能从当前 UI 状态推断。
- 当前账户与全局分区合并排序、All/Unread 查询、稳定游标、limit 边界和完整未读数。
- 单条已读/删除的当前账户可见性校验、`NOTIFICATION_SCOPE_MISMATCH`、全部可见已读、当前账户清空和 Global 保留语义。
- 90 天、账户 1000 条、Global 1000 条、24 小时维护、一次 Reset 和相同时间戳确定排序。
- copy-on-write、并发串行、next revision 随文件持久化、IPC 十进制修订字符串和 previous/next 事件一致性。
- main/tmp/bak 恢复顺序、temp 提升失败与 backup 恢复失败候选集、双文件损坏保留副本、未来 schema 不覆盖、首次无文件和孤儿账户分区清理。
- 只有持久化成功才发送变更事件；幂等命中与失败不发送 Created，恢复和历史加载不发送 Toast candidate。
- 账户删除通知清理失败不回滚账户删除，并返回 `NOTIFICATION_CLEANUP_PENDING`。

文件系统测试使用临时目录和可注入存储失败点，不依赖真实用户数据目录。

### 14.2 前端 Store 与组件测试

- Store 单次 start/stop、初始化顺序、摘要加载、50 条不透明游标分页、查询指纹、筛选重置和页尾重试。
- previous/next 修订连续事件、无关账户事件推进 observed revision、修订跳跃/Reset 从第一页重载、事件解除订阅和历史重载不补弹 Toast。
- 快速切换账户时忽略晚到响应，旧账户通知不进入当前列表或角标。
- 角标隐藏、1–99 和 `99+`，以及当前账户加全局的未读计算。
- 打开面板不自动已读；单条已读、全部已读、删除和清空成功/失败行为。
- `toastCandidate` 的当前账户/Global 判定、`seenCreatedIds` 去重、初始化未就绪不补弹，以及三类开关只影响实时 Toast；设置失败继续使用最后成功快照。
- 通知感知命令错误有 `notificationId` 时只显示行内错误，不调用通用 `reportError`；没有 ID 的普通命令错误仍使用既有通道。
- 加载、全部为空、未读为空、首次失败、分页失败和动作目标不可用状态。
- TopBar 局部 open、Popover -> TopBar -> AppShell 单向动作、对象形式通知设置深链、关闭后目标标题焦点、清空确认和账户名称展示。
- item 主按钮与删除 sibling 不嵌套；Enter/Space、受限 Delete、Escape、外部点击、焦点归还、可访问名称、选中语义和 `aria-live`。
- `get/update_notification_settings` 窄契约、旧配置默认值、自动保存失败继续使用最后成功快照。
- 后台错误展示不写第二条日志、命令错误由调用者单独拥有，以及通知 Toast adapter 从不写日志。

`NPopover` 在 jsdom 组件测试中使用受控 stub，只验证 show/open、触发按钮、事件链和 ARIA；真实锚定位置、窗口裁切和外部点击行为纳入桌面人工验收。

### 14.3 回归门禁与人工验收

自动门禁：

```powershell
pnpm lint
pnpm test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets
cargo build --manifest-path src-tauri/Cargo.toml --locked
```

任何与本分支无关的既有失败必须单独记录，不能用缩小测试范围掩盖。

桌面人工验收使用 `pnpm tauri dev`，至少验证：

1. 顶部铃铛、角标、悬浮定位、滚动和设置深链在最小窗口下可用。
2. 创建符合策略的订单、风控和连接事件后只出现一条通知，Toast 遵守设置。
3. 打开面板不自动已读；单条已读、全部已读、删除和当前账户清空正确。
4. 切换两个账户后列表和未读角标完全隔离，全局通知行为一致。
5. 重启应用后通知和已读状态保留，过期记录按规则清理。
6. 重复连接重试不刷屏，恢复后只出现一次恢复通知。
7. 模拟损坏文件和不可写存储后，应用仍能启动并提供可理解的恢复提示。
8. 全程不出现重复 Toast、重复日志或由日志自动生成的通知。

## 15. 验收标准

- 顶部铃铛成为可用入口，未读角标准确显示当前账户与全局通知总数。
- 约 400px 悬浮面板具备全部/未读、日期分组、分页、全部已读、单条删除和设置深链。
- 打开面板不会改变已读状态；所有已读、删除和清空结果以 Rust 权威状态为准。
- 首批业务事件只在最终、可行动状态创建通知，高频日志和重试不会进入中心。
- 订单和连接重复事件不会刷屏；旧账户会话事件不会污染新账户。
- 通知重启后保留，执行 90 天和每分区 1000 条策略，文件损坏不会阻止应用启动。
- 三类设置只控制实时 Toast，关闭后通知仍持久化并计入未读。
- 通知内容、动作和 IPC 都是受控结构，不包含密钥、原始响应、任意 URL 或任意路由。
- 错误日志、Toast 与持久通知不存在无意重复或递归。
- 前端、Rust 回归门禁和桌面人工验收通过；任何既有失败都被明确区分。

## 16. 预期文件边界

主要新增 Rust 单元：

- `src-tauri/src/models/notification.rs`
- `src-tauri/src/storage/notification_store.rs` 及测试
- `src-tauri/src/services/notification.rs`、`src-tauri/src/services/notification/` 策略与测试子模块
- `src-tauri/src/commands/notification.rs` 及命令测试
- 通知策略子模块和受控事件映射测试

主要修改 Rust 单元：

- `src-tauri/src/state.rs`、`lib.rs` 及各 `mod.rs` 注册
- `src-tauri/src/events/emitter.rs`
- 订单命令的 submission/观察入口、结构化风险拒绝、账户恢复/对账桥接和连接状态生产者
- 账户删除生命周期协调器
- 配置模型与窄范围通知设置命令

主要新增前端单元：

- `src/types/notification.ts`
- `src/services/notificationService.ts`
- `src/services/notificationToastService.ts`
- `src/stores/notification.ts`
- `src/components/notifications/NotificationPopover.vue`
- `src/components/notifications/NotificationList.vue`
- `src/components/notifications/NotificationItem.vue`
- `src/components/settings/NotificationSettingsPanel.vue`
- 对应 Store、组件和设置测试

主要修改前端单元：

- `src/components/layout/TopBar.vue`
- `src/components/layout/AppShell.vue` 的通知动作到 `NavigationTarget` 映射
- `src/components/settings/SettingsCenterPage.vue`
- `src/components/settings/settingsSections.ts`
- `src/stores/accountProfiles.ts` 与账户服务/类型，接入恢复/对账桥接和删除清理警告
- `src/App.vue` 的单一通知监听初始化边界
- `src/services/errorService.ts`、`src/components/common/ErrorToastBridge.vue` 与日志接线，明确后台/命令错误所有权

具体拆分允许在实施计划中按现有文件规模调整，但必须保持模型、策略、存储、服务、命令、Store 和组件职责分离。不得把通知业务堆入 `App.vue`、`TopBar.vue` 或通用错误服务。

## 17. 提交边界

设计文档、实施计划和功能实现保持独立 Conventional Commit。建议后续提交方向：

1. `docs(notifications): design notification center`
2. `docs(notifications): plan notification center implementation`
3. `feat(notifications): add persistent notification domain`
4. `feat(notifications): connect actionable business events`
5. `feat(notifications): add notification center UI and settings`

实现不得提交 `.superpowers/brainstorm/` 可视化会话文件，也不得顺带加入系统通知、SQLite、独立历史页或无关重构。
