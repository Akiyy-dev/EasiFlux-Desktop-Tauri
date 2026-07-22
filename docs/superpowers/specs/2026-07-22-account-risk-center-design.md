# 账户与风控中心设计

日期：2026-07-22
状态：设计已由用户确认，待规格复核

## 1. 背景与目标

当前账户一级导航及“API 管理 / 资产总览 / 风险控制”二级导航只有视觉状态，没有实际内容切换。配置和 Keyring 已能保存多个账户，但界面只能编辑当前默认账户，无法列出、切换或删除账户。Rust 已实现合约余额、持仓和资金账户余额接口；严格每日风控也已具备持久化账本，但界面只暴露交易日时区，无法查看或修改其他风控参数与当天用量。

本阶段把这些已有能力收束为一个可用的账户与风控中心，目标是：

- 完成单活动账户的添加、编辑、切换和删除闭环。
- 提供只读的合约资产、持仓摘要和资金账户余额。
- 提供完整的风控配置与当天额度状态。
- 保持凭据只进不出，任何前端接口都不能读取 API Key 或 Secret。
- 保持现有每日额度为应用全局额度，账户切换不能重置或绕过风控。

## 2. 范围边界

本阶段包含：

- 账户中心真实页面及二级导航联动。
- 当前账户的凭据新增、更新、测试、切换与删除。
- 单一活动连接；切换账户时安全迁移连接和数据状态。
- 合约余额、当前持仓、每日盈亏和资金账户余额的只读展示。
- 风控启停、最大单笔数量、最大价格偏离、每日订单上限、交易日时区和当日用量。

本阶段不包含：

- 多账户并行连接、跨账户聚合资产或每账户独立风控账本。
- 资金划转、划转记录、提币或其他资产变更操作。
- 旧 Python 客户端 Keyring 自动迁移。
- React 迁移、策略引擎、插件系统、新闻中心或独立图表页。
- 改动 REST、WebSocket 或 HMAC 协议。

## 3. 方案选择

采用渐进式 Vue 集成：保留 Vue 3、Pinia、Naive UI、现有 Tauri Commands 和 Rust service/storage 分层，在账户导航下增加真实页面和少量专用命令。

不采用以下方案：

- 扩大现有设置弹窗：实现较快，但账户一级导航仍是占位页，资产与风控无法形成稳定的信息架构。
- 直接实现多账户并行：需要为 API、WebSocket、调度器、状态仓库和风控账本增加账户维度，明显超出本阶段。
- 启动 React 页面迁移：会在核心功能未闭环时引入双框架状态适配和重复组件。

## 4. 信息架构与前端组件

### 4.1 导航

`AppShell` 维护结构化导航目标：一级页面 `page` 与可选二级页面 `section`。账户页支持三个固定 section：

- `api`：API 管理
- `assets`：资产总览
- `risk`：风险控制

`Sidebar` 不再只更新自己的局部高亮，而是向 `AppShell` 发出 section 选择事件。首页“查看资产”直接导航到 `account/assets`；账户一级导航默认进入上次选中的 section，本次进程首次进入默认 `api`。设置齿轮继续打开启动所需的快速设置弹窗。

### 4.2 页面拆分

新增 `AccountCenterPage` 作为轻量容器，分别挂载：

- `AccountProfilesPanel`：账户列表、活动状态、凭据状态和编辑操作。
- `AccountAssetsPanel`：合约资产、持仓摘要、每日盈亏和资金账户余额。
- `RiskControlPanel`：风控表单、账本健康状态和当日额度。

凭据输入提取为共享的 `CredentialEditor`。账户页与现有 `SettingsDialog` 复用同一验证和保存逻辑，避免两套表单逐渐产生不同语义。单文件继续遵守单一职责，业务状态分别放入 account profile store 与 risk store，不放进页面组件。

## 5. 账户领域接口

### 5.1 对外模型

Rust 与 TypeScript 增加 camelCase 对齐模型：

```text
CredentialState = present | missing | unavailable

AccountProfile {
  accountId: string
  label: string
  baseUrl: string
  credentialState: CredentialState
  active: boolean
}

AccountSwitchResult {
  activeAccountId: string
  connected: boolean
}
```

`AccountProfile` 不包含 API Key、API Secret、Keyring 条目名称或底层错误详情。Keyring 可访问但没有记录时为 `missing`；Keyring 自身不可用或记录无法解析时为 `unavailable`。

### 5.2 Commands

增加以下命令：

- `list_account_profiles() -> Vec<AccountProfile>`
- `switch_account(account_id, start_realtime?) -> AccountSwitchResult`
- `delete_account(account_id) -> ()`

保留 `save_credentials`、`has_credentials` 和 `test_connection`，以兼容启动弹窗与现有调用。`save_credentials` 的既有行为继续支持：新账户必须提交完整 Key/Secret；已有账户留空 Key/Secret 时保留 Keyring 中的原值；保存成功后把规范化账户 ID 加入 `config.accounts`，但不自动切换活动账户。

### 5.3 列表规则

- 账户 ID 统一 trim；空 ID 规范化为 `default`。
- 返回顺序与 `config.accounts` 一致，去除空项和重复项。
- 如果活动账户不在列表中，结果第一项补入活动账户，并在下一次成功配置写入时修复列表。
- 单个账户的 Keyring 读取失败只把该账户标记为 `unavailable`，不阻断其他账户展示。

### 5.4 切换规则

账户切换由 Rust 串行执行，同一时刻只允许一个账户生命周期操作：

1. 如果目标是当前账户，直接返回当前状态；否则校验目标账户存在且凭据状态为 `present`。
2. 使用临时 API 客户端校验服务器时间、公开行情和私有余额；失败时不改变当前账户或连接。
3. 记录旧账户、连接状态和实时推送选项，然后断开旧连接。
4. 持久化新的 `active_account_id`，成功后再更新内存配置。
5. 如果旧账户原本已连接，则连接目标账户并触发既有 bootstrap；原本未连接则只完成选择，不自动连接。
6. 连接目标失败时恢复旧配置并尽力恢复旧连接；返回错误必须同时说明主失败和回滚失败，不能报告虚假的成功状态。
7. 成功后清空前端旧账户的账户、持仓、订单和私有面板状态，等待新账户快照填充。

切换到当前账户是幂等操作：不重连，直接返回当前状态。

### 5.5 删除规则

- 只能删除非活动账户。
- 至少保留一个账户；唯一账户不可删除。
- 删除前必须由 UI 显示账户 ID 并二次确认。
- Rust 先读取凭据作为回滚材料，再删除 Keyring 记录并持久化账户列表；配置写入失败时恢复原凭据。
- Keyring 与配置任一操作失败都返回错误，不从前端列表乐观移除账户。

## 6. 资产总览

资产页只展示活动账户，包含：

- 合约账户总权益与各币种 available/frozen/total。
- 当前非零持仓数量、未实现盈亏和每日已实现盈亏。
- 资金账户各币种余额。
- API 连接状态、数据最后更新时间、刷新按钮及分区错误状态。

合约账户继续使用现有 scheduler/store 数据流。资金账户增加规范化模型：

```text
FundingBalance {
  asset: string
  available: string
  frozen: string
  total: string
}
```

Rust mapper 从公开 API envelope 中提取列表，并兼容服务端常见字段别名：币种使用 `coin/currency/asset`，可用使用 `availableBalance/available/available_balance`，冻结使用 `frozenBalance/frozen/locked`，总额使用 `totalBalance/walletBalance/balance/total`。无法解析的行跳过并通过现有诊断日志报告 raw/parsed 数量不一致；接口成功但没有有效行显示空状态，不伪造余额。

资金账户请求失败不清空已显示的合约资产，只在资金账户分区显示错误。未连接时不发起私有请求，并明确显示“连接账户后刷新”。本阶段不提供划转按钮。

## 7. 风控领域接口

### 7.1 对外模型

```text
RiskLedgerState = disabled | ready | unavailable

RiskStatus {
  enabled: boolean
  maxOrderQty: string
  maxPriceDeviationPct: string
  maxDailyOrders: number
  tradingDayTimezone: string
  ledgerState: RiskLedgerState
  tradingDay: string
  occupiedOrders: number | null
  remainingOrders: number | null
  updatedAtMs: number | null
  error: string | null
}

UpdateRiskConfigRequest {
  enabled: boolean
  maxOrderQty: string
  maxPriceDeviationPct: string
  maxDailyOrders: number
  tradingDayTimezone: string
}
```

### 7.2 Commands 与状态语义

增加：

- `get_risk_status() -> RiskStatus`
- `update_risk_config(request) -> RiskStatus`

`get_risk_status` 是只读快照。若持久账本日期或时区不是当前值，快照按当前交易日显示 0，但不为了展示而写文件；下一次真实预占仍负责持久化新交易日。风控关闭时不读取账本，`ledgerState = disabled` 且用量字段为 null。账本加载失败时返回 `ledgerState = unavailable`、用量为 null 和脱敏错误；真实下单继续 fail-closed。

额度仍是应用全局额度，`risk_usage.toml` 不增加账户字段，账户切换不得清零或重建账本。`remainingOrders` 使用饱和减法，不能出现负数。

### 7.3 更新与校验

- 最大单笔数量必须是可解析且大于 0 的十进制数。
- 最大价格偏离必须是可解析且大于等于 0 的十进制数。
- 每日订单上限必须大于 0。
- 时区必须能被 `chrono_tz` 直接解析，不再静默降级到默认时区。
- 先生成完整的新 `AppConfig` 并成功写盘，再更新内存配置和 `RiskService`。
- 更新失败时表单保留用户输入，但运行时配置保持原值。
- 风控关闭允许在账本不可用时保存；重新启用后如果账本仍不可用，下单继续 fail-closed。
- 现有 `save_config` 也复用同一个 Rust 校验函数，不能绕过专用命令写入非法风控配置。

## 8. 前端状态与交互

- account profile store 负责列表、保存、切换、删除及各操作 loading/error。
- risk store 负责状态读取、表单提交和最近成功快照。
- 切换账户期间禁用所有账户变更操作和交易入口。
- 切换成功后统一清理私有 stores，再由 bootstrap 填充；公共行情保持不变。
- 所有破坏性按钮显示具体目标并要求确认；重复点击通过前端 loading 与 Rust 串行锁双重抑制。
- 表单不回显 Key/Secret。编辑已有账户时空凭据表示保留；输入任一新值时要求 Key 与 Secret 同时填写，避免误以为只更新了半套凭据。
- 风控面板激活、保存成功和手动刷新时读取最新状态；本阶段不增加常驻轮询或新事件。

## 9. 错误处理

- Keyring 不可用：账户标记 unavailable，禁止切换或保存覆盖，其他账户仍可查看。
- 凭据预检失败：不触碰当前连接和配置。
- 切换回滚失败：返回组合错误并保持交易入口禁用，要求用户重新连接或重启恢复。
- 资产接口部分失败：分区展示错误，其他已成功分区继续可用。
- 风控账本不可用：配置仍可查看和关闭，启用状态下下单继续拒绝。
- 后端错误统一经过现有 `AppError`、`errorService` 与事件日志，不在 Vue 组件中重新解释协议错误。

## 10. 测试与验收

### Rust

- 账户列表规范化、去重、活动账户补入以及 present/missing/unavailable 状态。
- 新账户完整凭据要求与已有账户空凭据保留行为。
- 切换预检失败不改变旧账户；成功切换按原连接状态决定是否连接。
- 目标连接失败时配置和旧连接回滚；并发切换被串行化。
- 当前账户、唯一账户删除被拒绝；删除失败不产生半删除状态。
- 资金余额 envelope、字段别名、空列表和无效行解析。
- 风控快照同日计数、跨日显示 0、禁用、不健康账本和饱和剩余额度。
- 风控更新的数量、偏离、上限和时区校验；写盘失败不更新运行时。
- 切换账户前后全局风控额度保持不变。

### 前端

- 一级/二级导航与首页“查看资产”进入正确 section。
- 账户列表不出现 Key/Secret；新增、编辑、切换、删除 loading 与确认行为正确。
- 切换成功清空旧私有数据，失败保留旧数据并展示错误。
- 资产各分区独立处理 loading、empty、success、error 和未连接状态。
- 风控表单校验、保存失败保留输入、账本 unavailable 告警及 remaining 展示。
- SettingsDialog 与账户页共享凭据编辑规则。

### 回归

- 现有前端测试、TypeScript strict 检查、生产构建通过。
- Rust 全量测试、目标文件 rustfmt 和当前 CI 策略下的 Clippy 通过。
- 原有自动连接、交易下单、严格每日额度、环境检测和隐藏页签刷新行为不回退。

## 11. 提交边界

设计、计划和实现分别提交；每个实现提交必须同时包含该功能的测试：

1. `docs(account): design account and risk center`
2. `docs(account): plan account and risk center implementation`
3. `feat(account): add account profile lifecycle`
4. `feat(account): add read-only asset overview`
5. `feat(risk): expose configurable daily risk status`
6. `feat(ui): integrate account and risk center navigation`

如某项在实现中必须修正已有独立缺陷，使用单独 `fix(...)` 提交，不把无关修复混入功能提交。
