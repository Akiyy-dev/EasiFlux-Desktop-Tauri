# 本地插件作者指南

本地插件以宿主严格解释的 JSON 清单为入口。v1–v3 只能声明元数据、只读说明和固定页面快捷入口；v4 可携带一个按固定数值 ABI 运行的受限 WebAssembly 模块；v5 可在用户逐项授权后，把当前账户的有限快照交给另一个固定 Wasm ABI，并准备必须由用户另行确认的下单/撤单提案。它不是 JavaScript/npm、WASI 或原生动态库机制。这是一份本地创作指南，不是在线发布市场或通用 SDK。

## 选择清单版本

| 版本 | 可声明内容 | 可体验版本 |
| --- | --- | --- |
| v1 | 元数据和启用偏好，无命令 | 已有本地插件功能的版本 |
| v2 | `host.showInfo`：宿主显示纯文本 | v0.6.1 支持 |
| v3 | v2 动作，以及 `host.openPage`：用户点击打开固定页面 | 需要包含 Phase 1F 的构建，v0.6.1 不支持 |
| v4 | v2/v3 动作，以及 `sandbox.computeSeries`：用户明确运行无导入的数值 Wasm | 需要包含本地计算运行时的构建，v0.6.1 不支持 |
| v5 | v2/v3/v4 动作，以及 `sandbox.accountWorkflow`：读取逐项授权的当前账户快照并准备需另行确认的交易提案 | 需要包含账户工作流代理的构建，v0.6.1 不支持 |

不要把旧版清单改成可以导航、计算或访问账户的清单：导航必须显式使用 v3 以上，数值计算必须显式使用 v4 以上，账户工作流必须显式使用 v5。旧版本宿主拒绝未知版本是兼容性保护，不应通过修改宿主校验绕过。

## 从示例开始

- [工作区指南 v2](../examples/plugins/workspace-guide/manifest.json)：两个纯文本命令。
- [工作区快捷入口 v3](../examples/plugins/workspace-shortcuts/manifest.json)：三个页面入口和一条说明。
- [Series SMA v4](../examples/plugins/series-sma/README.md)：作者可读 WAT、可复现 Base64 和一个数值计算命令。
- [账户工作流 v5](../examples/plugins/account-workflow/README.md)：三个作者可读 WAT，演示授权余额显示、用户参数化下单提案和从快照选取订单的撤单提案。

复制示例到自己选择的普通本地 JSON 文件，修改 `id`、`publisherId`、名称、描述和版本。不要在清单中放 API Key、Secret、账户记录或任何私人数据；清单会以普通文件保存，也会在界面展示。

`id`、`publisherId` 和 `contributionId` 是至少两段的小写反向域名标识，例如 `com.example.workspace`、`com.example`、`workspace.charts`。贡献 ID 只需在本插件内唯一。发布者名称/ID 是作者填写的元数据，不提供身份认证。

## 动作格式

### 显示信息（v2 / v3 / v4 / v5）

```json
{
  "kind": "command",
  "contributionId": "guide.help",
  "title": "查看说明",
  "actionId": "host.showInfo",
  "params": { "title": "使用说明", "text": "这是作者提供的普通文本。" }
}
```

HTML、Markdown、URL 不会被解释或激活；命令名称和信息标题各最多 80 个 UTF-8 字节，正文最多 2000 个 UTF-8 字节，均不能是空白。

### 打开宿主页面（v3 / v4 / v5）

```json
{
  "kind": "command",
  "contributionId": "workspace.charts",
  "title": "打开图表工作区",
  "actionId": "host.openPage",
  "params": { "destination": "charts" }
}
```

| destination | 宿主显示的目标 |
| --- | --- |
| `home` | 首页 |
| `trading` | 交易页 |
| `charts` | 图表工作区 |
| `settings.general` | 通用设置 |
| `settings.notifications` | 通知设置 |
| `settings.about` | 关于 |

`params` 只能包含这一个字段。网址、文件路径、`settings.account`、任意路由对象、交易对或订单参数均不支持。按钮目标由宿主决定并明确显示，插件标题不能改变其含义。

正常宿主页面可能自行加载数据或保存已有图表状态；导航命令既不接收这些数据，也不替用户修改账户、设置或提交交易。没有返回值、插件回调或后台任务。

### 本地数值计算（v4 / v5）

`sandbox.computeSeries` 固定使用 `wasm-v1` / `series-f64-v1`，接收用户明确粘贴的有限数字序列和一个数值参数，只返回有限标量。不要自定义导出名、增加 import 或把模块当作通用 Wasm/WASI 应用。完整清单形状、ABI、内存/fuel/期限、取消语义和固定失败代码见[本地数值计算运行时](plugin-compute-runtime.md)。

先从 [Series SMA](../examples/plugins/series-sma/README.md) 复制，并保留 `plugin.wat` 作为可审阅源文件。运行 `node examples/plugins/series-sma/build.mjs --check` 验证清单中的 Base64 确实由该源文件生成；生成器使用锁定的开发编译器，只读取固定示例路径，不实例化来宾，也不需要生产密钥。

### 账户工作流（仅 v5）

`sandbox.accountWorkflow` 固定使用 `wasm-v1` / `account-json-v1`。模块无 import，只能把宿主捕获的已授权 JSON 快照和用户 JSON 对象转换成纯文本、下单提案或撤单提案。清单声明的能力只是申请范围；导入、启用都不授权。用户必须在当前连接会话中逐项授予，非空授权必须包含 `account.read`。

可申请能力只有 `account.read`、`balances.read`、`positions.read`、`orders.read`、`market.read`、`trade.place`、`trade.cancel`。不要把 API Key、Secret、私有会话、账户 ID、端点或快照写入模块/清单。Run 只执行来宾并准备提案，绝不会自动交易；每一次真实下单/撤单都由宿主显示不可变的账户与规范化参数，并要求另一次明确确认。短期 token 只能使用一次；撤销授权、切换/断开账户、插件内容或运行时变化会使旧工作失效。

插件授权不能提升交易所 API Key 本身的服务端权限：读取需要该 Key 已有读取权限，确认后的下单/撤单仍需要该 Key 已有交易权限。缺少权限时宿主会拒绝，插件不能绕过。Key 的创建和权限配置应在插件外按[官方 API 快速入门](https://www.easicoin.io/api-doc/zh-CN/common/QuickStart)完成；不得把 Key 或 Secret 放进清单或 Wasm。

具体导出、JSON 联合类型、大小限制、确认/恢复语义、上游来源与派生字段规则见[账户工作流契约](plugin-account-workflow.md)。下单的 `positionIdx` 只能是 1/2：开仓 Buy/1、Sell/2，只减仓 Sell/1、Buy/2；不要独立修改方向、只减仓和仓位索引。订单快照只包含 `order_filter=Normal` 的普通活动订单，不包含条件单或 TP/SL。结果为 `unknown` 时不要重试提交，应在交易与恢复界面核对；v5 不提供定时器、自动确认或无人值守策略。

## 导入与检查

1. 使用包含相应清单版本的原生应用，从「已安装插件」或「插件管理」导入单个 JSON 文件。
2. 预览中检查动作和目标；对 v4/v5 还要核对代码变更、模块字节数、运行时、ABI 和参数范围。对 v5 逐项核对申请能力；清单里请求不等于授权。文件超过 16 KiB、未知/缺失/重复字段、非法枚举、错误参数形状均会拒绝。
3. 确认导入后内容仍默认停用。明确启用，在卡片或命令工作台点击验证；计算/工作流还必须在表单中再次点击 Run。v5 还必须连接账户并逐项授权；Run 仍不提交交易，真实下单/撤单有单独确认。导入、预览、启用、授权、搜索和渲染不会自动执行来宾或交易。
4. 停用，确认命令不可执行。重新扫描或操作结果不明确时，先确认目录状态再重试，不重放旧操作。

v2/v3/v4 必须有 1–16 个命令，`requestedCapabilities` 必须是 `[]`。v5 必须包含账户工作流且申请 `account.read`，只能使用封闭能力列表。其他命令/能力名称不会自动扩展宿主权限。完整尺寸限制与存储规则见[本地清单文档](plugin-local-manifests.md)。

## 版本更新与分发

包含 Phase 1G-1 的构建可在导入预览中比较相同 ID 的新旧清单；已发布 v0.6.1 不提供此功能。先导入原始示例，再选择[更新候选示例](../examples/plugins/workspace-shortcuts/update-candidate.json)，即可查看版本优先级、发布者 ID、字段前后值、命令新增/移除/变更与顺序变化。比较基于捕获时的目录快照，不会自动安装、停用或启用插件。

版本优先级按 SemVer 比较：例如 1.10.0 高于 1.9.0，正式版高于同版本的预发布版，`+build` 元数据不影响优先级。但同优先级、同版本号都可能有内容变化；版本更高或发布者 ID 相同也不代表来源可信。导航命令需核对宿主给出的目标名，不能只看命令标题或贡献 ID。

已有 ID 的预览只允许查看、关闭，不会覆盖旧插件；目录变化或预览过期后应重新选择。目录里未发现 ID 时保留普通导入流程，提交仍会重新扫描检查，不能把预览当作安装成功的保证。

目前仍没有同 ID 覆盖更新、版本回滚、签名认证、在线下载和自动更新。如必须人工更换，先保留原始清单，再在应用内停用、移除旧副本并导入新清单；这是两个独立操作，不提供原子替换或失败自动恢复。不要手工修改 receipt 或管理索引。受管副本和原始文件不同，编辑原文件不会自动更新已导入内容。

所有清单语义（包括导航目标、计算/工作流模块字节、能力和参数元数据）都绑定启用指纹。只改变 JSON 排版或字段顺序不改变身份；改变语义需要重新确认启用，不能继承旧内容授权。v5 授权还只存在当前应用会话，重启不恢复。

## 后续生态边界

已实现本地命令、可导入示例、作者契约、更新前比较、封闭的 Wasm 数值 ABI，以及需要逐项会话授权和逐次交易确认的账户工作流 ABI。真正的受管更新/回滚需要不可变版本槽位、权威激活记录和中断恢复，不能由“移除再导入”替代。后续还包括签名市场与发布链路、更多按最小权限开放的宿主动作，以及无人值守策略所需的独立策略/风险/恢复设计。两个 Wasm ABI 都不提供文件、任意网络、凭据或第三方 SDK；v4 计算仍不提供交易、行情或账户数据。
