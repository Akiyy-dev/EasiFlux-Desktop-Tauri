# Phase 1F：受控页面快捷命令

基线：`main@c177b236`（v0.6.1），分支：`plugin/host-navigation-commands`。

## 目标与取舍

沿用户已授权的自主开发流程，给本地插件第一种实际工作区动作：用户点击后打开宿主已有页面。完整生态继续分阶段建设，本阶段不声称交付在线市场或通用代码扩展。

比较三个方向：本地更新/回滚需要单独修改跨文件事务；在线市场需要签名、信任根和发布基础设施；受控页面命令能复用已有目录授权与 AppShell 导航，且不向插件暴露业务数据。因此先实现后者，并提供可直接导入的工作区快捷入口示例和作者指南。

## 协议与兼容性

- Manifest v1 元数据行为、v2 仅 `host.showInfo` 行为保持不变，原有规范化字节/内容指纹保持不变。
- 新增 Manifest v3，仍严格接受相同顶层字段、1–16 个唯一贡献 ID、空 requestedCapabilities。仅本地声明式来源接受 v2/v3，内置继续只接受 v1。
- v3 支持原有 showInfo 以及以下新贡献：

```json
{"kind":"command","contributionId":"workspace.charts","title":"打开图表工作区","actionId":"host.openPage","params":{"destination":"charts"}}
```

- destination 只能是六个精确字符串：`home`、`trading`、`charts`、`settings.general`、`settings.notifications`、`settings.about`。不接受账户设置、任意 URL、路径、路由、IPC 名称或额外参数。
- 所有对象层级拒绝数组、重复字段、未知字段、未知 actionId/枚举、动作与参数不匹配。保持既有文本/清单容量限制。Rust typed constructor 的 validate 也执行同样约束。
- v3 的 actionId 和 destination 进入完整规范化指纹；目标改变使旧启用决定失效。不得改变 ownership/state/IPC 传输版本或扩大 ACL。

## 执行与界面

继续从 plugin store 的已确认目录派生命令；只传 pluginId + contributionId，在实际点击时重新查找并经过原有 gate。加载、重扫、启停/导入/移除中、结果未知、禁用、阻止、内容变化后均不能执行。没有自动运行、排队重放或缓存执行令牌。

store 的 `runCommand` 返回宿主定义的辨别联合：`{actionId:'host.showInfo', info:PluginCommandInfo}` 或 `{actionId:'host.openPage', pluginId, pluginName, contributionId, destination:PluginPageDestination}`，不可执行时返回 null。现有 Info 展示模型保持不变。summary 新增 actionId，openPage 的 summary 包含固定 destination，供宿主显示目标名称；summary 不作为授权。

命令卡片、工作台、导入预览必须明确显示动作类型和宿主确定的目标名称（不能只显示作者可伪造的标题）。showInfo 按钮保留“显示信息”，导航按钮显示“打开页面：<目标>”。仅用户点击执行；没有自动跳页。纯文本输出和旧结果同步撤销规则保持不变。

从 composable 到组件使用显式 Vue 事件逐层传递 `{pluginId, contributionId}` 的导航意图；AppShell 收到后再次从当前 store 调用 runCommand，只接受 openPage，再用宿主固定映射调用现有 navigateTo。不得从插件传入 NavigationTarget、异步可重放 action 对象、全局事件总线、URL 或回调。映射放在小型 `src/services/pluginNavigation.ts`，仅允许上述六个目标，不持有业务 store。

独立 smoke 宿主没有 AppShell，不提供页面导航执行：页面通过显式 `navigationAvailable` prop（默认 false）控制导航命令按钮；正常 AppShell 明确传 true。showInfo 不受影响。即使错误发出导航意图，没有宿主处理者也不产生动作。

正常页面打开后仍遵循宿主自己的数据加载和图表保存流程；不能承诺宿主页面不会发起其正常网络请求。但插件无法获得页面数据、密钥、账户/订单参数，也不能修改设置或提交交易。

## 作者体验

保留 workspace-guide v2 示例，新增 `examples/plugins/workspace-shortcuts/manifest.json` v3 示例，包含图表、交易页、通知设置三个导航命令和一条说明命令。默认不安装、不启用。README 说明导入/启用/使用/停用/移除、升级版本要求、目标白名单及限制。更新本地清单文档和增加简洁作者指南；不称为任意代码 SDK。

## 验证与非目标

少量定向 RED/GREEN 覆盖 Rust 严格解析/版本门控/指纹，前端解析与现有授权 gate，真实插件页面到 AppShell 的点击和白名单导航。最终运行相关前端回归、类型检查、范围内 lint、前端 build，以及受影响 Rust 模块；不反复跑整应用测试。

不触碰用户 AppData、keyring、账户 API 或根工作树改动，不启动生产应用，不做交易请求，不新增依赖。不改导入/移除算法，不宣称历史 Windows OS 5 已修复，不实现全局 Command Palette、在线下载、更新、签名、脚本/WASM 或后台任务。
