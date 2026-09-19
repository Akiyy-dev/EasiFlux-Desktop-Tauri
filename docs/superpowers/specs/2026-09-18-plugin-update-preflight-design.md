# Phase 1G-1：插件更新前的目录比较

基线：`main@b42f45b`，PR #37 已合并；工作分支 `plugin/update-preflight`。

## 范围与取舍

本阶段交付已有导入流程中的同 ID 清单比较，不交付覆盖更新、自动更新或版本回滚。沿用户明确授权的自主开发流程执行。

三种方案的结论：直接覆盖 manifest/receipt 缺少跨文件原子性；把移除接导入会丢失旧版本并产生中断缺口；不可变版本槽位加权威激活记录适合作为后续事务方案，但需要同时升级发现、所有权和恢复协议。目前发现过程拒绝重复 ID，所有权 V1 只支持 Managed/Removing 且每个 ID 只能有一个条目。因此先交付有实际用途的比较界面，继续保留后端重复 ID 拒绝。

## 比较协议

仅 `prepare_local_manifest_import` 的 ready 响应升级为 `schemaVersion: 2`，新增必填 `assessment`。取消响应仍为 v1，commit 响应仍为 v2；目录、清单、所有权、状态文件版本均不变。前端严格拒绝缺失 assessment 的旧 ready 响应，不将未知响应默认为新安装。

```typescript
type ImportAssessment =
  | { kind: 'notInCatalog' }
  | {
      kind: 'existingId'
      current: PluginCatalogItem
      versionRelation: 'incomingLower' | 'samePrecedence' | 'incomingHigher'
    }
```

`current.manifest.id` 必须等于所选清单 ID。current 使用既有完整目录条目校验，不能伪造 builtIn/localDeclarative、状态、management 的组合。所有 assessment 对象拒绝未知、缺失字段和数组。不存在路径、receipt、槽位、文件对象身份、凭据、授权令牌或 canUpdate/eligible 字段。

后端在文件选择/读取完成后，从同一个 registry 读锁取得完整目录快照和 generation；比较只查询该快照。ImportPreview、session 和捕获内容继续共享原有 300 秒一次性确认生命周期。Rust 使用既有 semver 的 `cmp_precedence`：忽略 build metadata 的优先级差别，保留原始版本字符串用于内容差异显示。

`notInCatalog` 只代表所捕获目录快照中没有该 ID，不意味着磁盘一定无冲突或承诺可以安装。原 commit 路径仍重新扫描、检查重复 ID、容量和所有权等条件。比较本身不调用暂存、包准备或状态/所有权写入；不改变已有 get_catalog 初始化/重试时的协调行为，不能把整个 prepare 宣传成绝对零写入。

## 前端语义差异

增加独立纯函数 `comparePluginManifests(current, incoming)`，输入为已校验、同 ID 的清单；不读取 store、不发 IPC、不对原对象排序或修改。

- 比较字段固定为 schemaVersion、publisherId、publisher、name、description、version、requestedCapabilities。字段顺序固定；ID 已由协议保证相同。
- 按 contributionId 配对，分别报告 added、removed、changed。changed 比较完整 title、actionId 和对应全部 params；键顺序不是差异。新增/变更条目按 incoming 顺序，移除条目按 current 顺序。
- orderChanged 仅比较两边共同命令的相对顺序；纯新增/移除不单独触发重排提示。语义完全相同是所有字段、命令和相对顺序都未变。
- publisherIdChanged 由 publisherId 判断，不使用显示名称。相同 publisherId 不代表发布者经过认证。
- 同版本不同内容、只改 build metadata、v1/v2/v3 清单变化都可见；版本优先级提示不是安全评价。

使用一个小型比较组件展示“目录中的清单”和“所选清单”、版本优先级、来源/管理状态、发布者 ID 变化、字段前后值、命令新增/移除/变更前后值及重排。信息正文、作者文本和 URL 只按普通文本显示，无 HTML/Markdown/链接激活；页面动作使用既有宿主目标标签。大段正文可在原生 details 内展开，默认显示变更类别和命令身份，避免长内容撑满对话框。

## 操作与失效规则

- notInCatalog 保持正常导入预览与确认；明确提示仍需提交时重新检查。
- existingId 显示“同 ID 清单比较”，明确“仅比较，不覆盖安装；更新与回滚尚未开放”。关闭操作取消原 token；确认导入按钮禁用，点击处理和 store.commitImport 再次拒绝 existingId。
- 即使直接调用后端 commit，同 ID 也继续返回原冲突结果；没有新更新命令或暗中串联停用、移除、导入。
- 不根据前端当前目录临时更换 comparison.current；比较始终绑定捕获 generation。已有目录变化、过期、取消、离页、重复点击和未知结果规则不减弱，已失效的比较不能变成可提交的新导入。
- 所有比较都不执行插件命令或更改启用状态。受管、外部、内置、冲突/不可用条目均只能被展示，不得被称为“可更新”。

## 验证与文档

定向 RED/GREEN 覆盖后端快照绑定与版本优先级、前端严格解析、纯差异函数、真实组件与 store 的提交拒绝。版本用例包含数字优先级、预发布、build metadata；差异用例覆盖纯格式/键顺序、同版本修改、发布者 ID/名称、参数、动作和共同命令重排。至少一个 runtime 测试证明 picker 期间目录变化后 assessment 和 generation 来自同一后置快照；保留直接同 ID commit 被拒绝的测试。

只运行相关回归、类型检查、范围内 lint/rustfmt 和前端构建；不重复全套。增加与 workspace-shortcuts 相同 ID 的 update-candidate.json 及体验步骤，用真实 Rust parser 验证示例。文档明确已发布 v0.6.1 不包含此功能。

## 后续真实事务的约束

后续采用不可变包及一个权威所有权文件记录 active、可选 previous 和有界 pending transition。先建立并验证新版本，持久化新身份停用决定与待切换证据，再切换激活角色，最后一次性发布完整目录。孤立暂存/旧版本不能降级为可执行 external 插件；遇到不确定状态保留文件并阻止受影响插件。

回滚同样重新校验对象、持久化停用状态并要求重新启用。旧版本历史淘汰需要单独的有界清理授权；不能把现有 removal quarantine 当作回滚仓库。当前阶段不迁移任何生命周期/存储协议，也不承诺历史 Windows OS 5 已修复。

## 不变边界

不新增 IPC、ACL、依赖、网络接口或脚本能力；不修改导入提交、移除、发现及持久化算法。不启动生产应用，不读真实 AppData/keyring，不请求账户/交易 API。保留根工作树及其他分支的所有既有改动。
