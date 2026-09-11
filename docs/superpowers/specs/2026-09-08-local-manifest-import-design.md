# EasiFlux 单文件本地清单导入设计

**日期：** 2026-09-08

**所属 PRD：** PRD-11 插件系统 UI（Plugin Marketplace）

**阶段：** Phase 1B，metadata-only 本地清单导入

**基线：** `2e519da9c35ab7ccdec4de2171cc81e429c7c437`（运行时基础与 Phase 1A 本地发现）

**设计分支：** `plugin/local-manifest-import`

**状态：** 已批准（用户委托自主决策），进入实现；本文规定的验收边界具有约束力。

## 1. 目标与范围决策

用户从插件页选择一份 JSON 清单，查看宿主解析后的准确内容，确认后由宿主复制成固定目录中的合法包。导入结果默认停用，不执行代码、不解释贡献、不授予权限。

本文延续《2026-09-07-plugin-system-design》和《2026-09-07-local-manifest-discovery-design》；主要扩展“不能通过应用导入清单”的阶段边界。现有手工包格式、来源、指纹规则和扫描上限保持兼容；第 3.5 节是唯一恢复语义收紧：手工包与导入包只要属于 `localDeclarative`，从次级 state 候选恢复时都必须重新明确启用。

方案比较：

| 方案 | 收益 | 代价 | 决策 |
| --- | --- | --- | --- |
| 只为已发现项目增加确认 | 改动很小 | 用户仍须手工创建不透明槽位，未解决导入摩擦 | 不单独交付 |
| 单文件导入、内容预览、默认停用 | 输入与写入范围可界定，独立可用 | 必须定义令牌、暂存、失败与目录发布 | 采用 |
| 导入、更新、卸载、通用信任仓库 | 生命周期完整 | 所有权索引、删除授权、恢复事务、兼容迁移显著扩张 | 留给后续独立设计 |

“宿主管理槽位”在本阶段仅表示宿主生成目录名并写入完整清单，不表示操作系统级不可变内容、签名信任或可以安全卸载的所有权凭据。导入后仍以 `localDeclarative` 参加现有扫描；不新增第二类本地来源。

## 2. 明确非目标

- 不导入目录、ZIP 或其他压缩包；不读取附件、图片、资源、HTML、脚本、原生库或 WASM。
- 不执行插件或宿主贡献；`contributions`、`requestedCapabilities`、`grantedCapabilities` 仍为空。
- 不下载、市场安装、自动更新、覆盖更新、版本降级、卸载、回滚或删除手工包。
- 不认证自报的 `publisher` / `publisherId`，不创建通用 `trusted` 标记或授予未来权限。
- 不接收前端提供的路径、URL、原始 JSON、目标槽位、来源或 fingerprint。
- 不增加拖放路径、剪贴板导入、目录扫描器、文件监听或插件事件订阅。
- 不改变 manifest schema 1、catalog transport 2 或 state schema 2。
- 不实现跨进程插件状态协调；并发运行多个 EasiFlux 进程时的插件写操作不受支持。

## 3. 安全模型与明确代价

### 3.1 信任边界

来源文件及清单文案是不受信任输入。宿主只相信自己的严格解析结果、内容指纹、令牌记录和固定目的路径。原生文件选择只表明用户选择了某个输入，不表示输入可信。

必须防止路径穿越、链接／硬链接／reparse point 重定向、设备与 FIFO 阻塞、过量输入、重复 ID 覆盖、发布者仿冒、预览后换文件、令牌重放、取消后误提交、迟到响应复活和未完成文件被当作合法包。

本阶段继续排除已控制宿主进程或当前 OS 用户账户的攻击者；不承诺抵御该攻击者持续改写配置目录。原生选择器可能涉及 OS 文件提供程序，本应用不读取选择项以外的文件，不处理远程 URL。通过 OS 挂载的文件系统仍可能有较慢或失败的系统调用；字节与并发限制不等于任意文件系统 I/O 的硬实时保证。

### 3.2 命名上的信任限制

UI 使用“确认导入此清单”，不使用“信任此发布者”或“允许插件运行”。清单内容指纹仅绑定确认内容，不是签名。该确认不会记录为未来 schema、贡献、权限或版本的授权。

### 3.3 单进程范围裁决

`PluginRuntime` 内串行化同一进程的 reload、启停和导入。**不新增 `runtime.lock` 或跨进程租约，也不改应用实例生命周期。** 两个 EasiFlux 进程并发修改同一配置目录属于本设计不支持的使用方式；本文的顺序、无丢失更新和失败后默认停用保证以单进程写入为前提。解决多实例属于后续进程生命周期设计，不能把现有进程内 gate 宣称为跨进程事务。

### 3.4 失败关闭而非全回滚

采用“完整暂存 → 持久化停用 → 提升包 → 权威扫描发布”。不为一个清单引入跨文件事务日志。

这允许失败留下当前 ID/source 的停用决定，或留下一份扫描根外的暂存文件。它不能留下继承旧启用的导入包。UI 必须区分未导入、未导入但停用已保存、已写入但尚未确认目录显示三种情况，不宣称所有失败均无副作用。

### 3.5 次级状态恢复同样失败关闭

现有 state store 在主文件无效时可以读取合法 `.tmp` / `.bak`。为避免导入前备份中的本地 `enabled=true` 在主文件损坏后复活，从任何非主候选恢复时都把所有 `localDeclarative` 条目规范为 `enabled=false`，并标记需要安全重写；built-in 决定保持原值。有效主文件仍完整保留本地启用状态，未来 schema、主文件超限等现有终止性错误仍禁止降级读取。

这项安全恢复不是新的用户决定，不单独增加 revision；首次目录发布仍提供新的进程内 generation。代价是状态文件损坏并从次级候选恢复后，用户需要重新明确启用本地插件。将备份手工复制成主文件属于当前 OS 用户主动修改配置，超出本文威胁模型。

## 4. 固定路径与资源预算

沿用 `dirs::config_dir()/APP_NAME`，不切换到 bundle identifier 根：

```text
<platform config_dir>/EasiFlux Desktop/plugins/
├── state.json                         现有 state v2 与恢复候选
├── local/
│   └── pkg-<32 lowercase hex>/
│       └── manifest.json              提升后的完整包
└── import-staging/
    └── stage-<32 lowercase hex>/
        └── manifest.json              从不参加本地扫描
```

目标与暂存槽位使用分别生成的 UUID v4 simple 字符串；不能来自 manifest ID、输入文件名或前端。槽位只在后端存活，不进入 IPC。`import-staging/` 与 `local/` 必须同卷，禁止跨卷复制降级。

| 资源 | 精确上限／策略 |
| --- | --- |
| 一次选择 | 恰好一个普通本地文件；不接受目录与 URI 变体 |
| 输入清单 | 16,384 字节；从已打开句柄读取，最多读取 16,385 字节以检测超限 |
| 最终规范序列化清单 | 16,384 字节；超限不生成 preview |
| 原生选择／读取／预览／提交会话 | 全进程最多 1 个；busy 直接返回，不排无界队列 |
| ready 令牌 | 最多 1 个；使用 UUID v4 simple 的 32 个小写十六进制字符 |
| ready 令牌有效期 | 从 preview 准备完成时起 300 秒；后端单调时钟，精确到期即无效 |
| committing 会话 | 最多 1 个；取得提交所有权后不因 TTL 或 UI 断开中止 |
| 源路径／源句柄保留 | 读取完即释放；不写日志、状态、令牌或 IPC |
| import-staging 直接项 | 最多 16 个，包括不合法或未清理项；第 17 个前拒绝新建 |
| 每个阶段目录 | 只能有一个 `manifest.json`；只清理由本次进程持有所有权的准确路径 |
| 目标槽位碰撞重试 | 最多生成 4 个目标槽位；仍碰撞则拒绝，不覆盖 |
| import-staging 新目录名碰撞 | 最多生成 4 个名称；仍碰撞则拒绝 |
| 现有本地扫描 | 根直接项 256、结构合格包 128、单清单 16 KiB、总读取 2 MiB，全部保留 |
| 状态持久化 | 256 KiB、512 个身份，全部保留 |

没有预览刷新计时器；新 prepare、cancel、commit 入口按单调时钟清理过期 ready 会话。OS 选择器未关闭时不开放第二个选择器。异常慢输入仍仅占据一个 worker 和一个会话，不再接受更多源读取。

暂存项跨重启保留，不自动递归清扫，也不当作已确认导入恢复。容量满时显示净化的“暂存区需要清理”错误；维护文档给出固定 `import-staging` 路径及退出应用后人工检查方法，不提供一键清空或自动删除未知内容。

## 5. 原生选择与输入验证

Rust 引入 `tauri-plugin-dialog` 2 的宿主 API，注册其 Tauri framework plugin；不引入前端 dialog SDK，不授予任何 `dialog:*` capability。宿主使用单文件 `pick_file` callback，经 oneshot 返回结果；不得在 UI 主线程调用 blocking picker。选择器归属当前 local main 窗口，标题为“选择插件清单”，过滤器为 JSON 文件，禁止多选。该 API 的普通回调只有 `Option<FilePath>`：`None` 统一作为“无选择／cancelled”；`failed` 只表示宿主能够观察到的窗口获取、适配器启动、callback channel 关闭或后台任务故障，不承诺区分原生后端未暴露的选择器错误。

过滤器只是选择提示；判断是否接受以安全文件打开和清单内容为准。用户选择的文件不要求名为 `manifest.json`，目标文件名始终由宿主固定为此值。

只接受 `FilePath` 的原生路径结果，不把 URI 转换成文件路径。路径必须绝对；Windows 仅接受本机盘符与 verbatim 本机盘符路径，拒绝 UNC、设备命名空间、ADS 与父目录组件。Unix 从 `/` 开始逐组件打开。源路径祖先与文件均不得通过 symlink/reparse point 跳转；因此通过链接访问的合法文件也会被拒绝，这是刻意的保守边界。

输入文件必须为单链接普通文件：

- Unix：handle-relative、`NOFOLLOW`；文件打开含 `NONBLOCK | CLOEXEC | NOCTTY`，读取前 `fstat` 验证普通文件和 link count 为 1。
- Windows：祖先目录句柄不允许 delete sharing，使用 `FILE_FLAG_OPEN_REPARSE_POINT`，拒绝所有 reparse 属性；文件句柄只允许 read sharing，并在读取前检查文件类型及 link count。
- 不使用“检查 metadata 后普通 open”作为安全边界；存在的 Phase 1A safe-reader 原语可以抽取共享，但不能放宽其现有行为。

读取在 blocking worker 中进行。严格 UTF-8、拒绝 BOM、恰好一个 JSON object、全部九个必需字段、无未知／重复字段、有效 SemVer、现有 ID 与文案字节上限，贡献和能力为空。不得先解析到会丢弃重复字段的通用 JSON map。

验证后创建 `PluginRecord::local_declarative`，使用现有冻结字段顺序的 canonical DTO 序列化。preview 展示的 manifest、内部 fingerprint 和最终写入的字节来自同一份已验证对象。释放输入句柄后不再访问源文件；源文件移动、删除或修改不会改变待确认内容。

## 6. 导入会话与固定协议

新增协议 envelope 独立使用 `schemaVersion: 1`，内部嵌入现有 manifest v1 与 catalog snapshot v2。每个 union 分支都严格校验精确 key；不增加 optional 任意字段。

```ts
type PrepareLocalManifestImportResult =
  | { schemaVersion: 1; status: 'cancelled' }
  | {
      schemaVersion: 1
      status: 'ready'
      token: string
      expiresInSeconds: 300
      catalogGeneration: string
      manifest: PluginManifestV1
    }

interface CancelLocalManifestImportResult {
  schemaVersion: 1
  status: 'cancelled'
}

type CommitLocalManifestImportResult =
  | {
      schemaVersion: 1
      status: 'imported'
      pluginId: string
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'notImported'
      disabledDecisionSaved: boolean
      reasonCode: LocalManifestImportCommitFailure
      snapshot: PluginCatalogSnapshot
    }
  | {
      schemaVersion: 1
      status: 'importedNotVisible'
      pluginId: string
      reasonCode: 'plugin_import_publication_unconfirmed'
      snapshot: PluginCatalogSnapshot
    }
```

固定命令：

```text
prepare_local_manifest_import()
cancel_local_manifest_import(token)
commit_local_manifest_import(token, expectedCatalogGeneration)
```

prepare 先以现有普通 get 语义完成首扫／状态恢复；要求顶层 availability 和 localDiscovery.status 都为 available，否则不打开选择器并返回对应净化错误。读取完成后再次检查这两个条件，并获取当前 snapshot 的 generation；token 绑定该 generation、canonical manifest、fingerprint 和到期单调时间。token 不保存磁盘路径；preview 不返回指纹、槽位、输入名称或原始 JSON。

一次成功 prepare 不写包、不创建 plugins 子目录、不更改状态或 revision；token 仅在内存保存。初始 get 的既有首扫不算安装行为。

commit 要求传入的 generation 同时等于 token 绑定值与执行时 registry 值。不能从前端换成更新后的 generation 来复用旧 preview。有效 token 在取得提交所有权时一次性消耗；随后因 stale、冲突、I/O 等原因拒绝也不恢复 token。重复提交、到期、格式错误、其他 token 和重启后的 token 均为 `plugin_import_token_invalid`，不产生磁盘写入。

cancel 在 ready 时清除匹配 token；已经取消、到期或未知 token 同样返回 cancelled，不泄露 token 是否存在。已取得提交所有权时返回 `plugin_import_busy`，不能取消写盘。用户仍在原生选择器中时通过原生取消退出；不会创建 token。

ready 会话占有 prepare admission；新的 prepare 返回 busy，不静默替换待确认内容。前端取消后才可重新选择。

取得提交所有权但响应丢失后，客户端不得自动重发或重新 prepare 安装相同内容。显示“结果尚未确认，请重新扫描”，通过显式 reload 确认磁盘状态；重放旧 token 不能再次写入。

## 7. 提交流程与明确提交点

### 7.1 前置校验与暂存

1. 原子取得有效 token 与单会话提交所有权，启动拥有全部工作状态的后台任务；在首个可能丢弃调用方的 await 之前完成所有权转交。
2. 进入 runtime 操作 gate，检查 expected generation；失配返回 notImported、stale、`disabledDecisionSaved=false` 和当前 snapshot。
3. 在 blocking worker 完整扫描固定 local 根，不持有 registry guard。发布这一权威扫描结果，再比较 generation；磁盘变化导致 generation 改变时拒绝本次确认，不替用户重放。
4. 要求顶层 availability 为 available、本地发现为 available。degraded 或 unavailable 均拒绝导入，避免无法判断的冲突或预算；内置插件仍按 Phase 1A 正常工作。
5. 拒绝与任何内置或本地项目相同的 ID，包括同 publisher、同版本、同 fingerprint 的重复导入；不更新、不去重覆盖、不选赢家。由于步骤 4 要求完整无拒绝扫描，本地重复 ID 已导致 degraded，不会漏过冲突。
6. 检查增加一个普通包后的目录预算：当前根直接项最多 255，结构合格包最多 127，实际本轮读取字节数加最终清单字节数最多 2,097,152。扫描器向后端返回计数，不将这些计数加入 catalog IPC。
7. 要求当前 catalogGeneration 小于 `u64::MAX`，为提升后的单次发布保留一个 increment；revision 容量、状态字节数与身份容量按步骤 9 的候选验证，不靠目录大小推断。
8. 安全打开／创建固定 plugins、local 和 import-staging 链。新建路径逐层、no-follow、无任意 `create_dir_all` 越界；暂存根计数有界。独占创建暂存目录和清单文件，写入 canonical bytes、file sync；Unix 同步阶段目录及其父目录。暂存完全准备成功前不得更改启用状态。

创建固定空目录或清理本次暂存是允许的辅助副作用；不能将“尚未保存 disabled”表述为整个文件系统完全未变化。

### 7.2 先持久化停用

9. 在 registry 写锁中构造候选 state：清理同 `(id, localDeclarative)` 的所有历史 publisher/fingerprint 身份，加入当前精确内容身份且 `enabled=false`。身份即使当前不在 catalog 中也可由内部专用方法记录；不开放任意 ID 的新 IPC。
10. 验证有界 state、revision 和 migration；用现有 state store 原子保存。只有保存成功才提交 registry 内存；候选失败完全保留原内存、旧身份和 migration 标记。state store 同时落实第 3.5 节：只有从 `.tmp` / `.bak` 恢复时才强制清除全部本地 enabled，主文件的正常启用不受影响。
11. 状态逻辑改变才增加 revision；纯 schema/order rewrite 不增加；规范 disabled 决策已存在时允许 no-op。后文的 `disabledDecisionSaved=true` 表示步骤 10 已成功确认该停用决定持久成立，包含无需重新写入的合法 no-op，不表示一定增加 revision。

这个步骤必须发生在目标包变得可发现之前。它处理 Phase 1A 的 orphan 恢复行为：相同清单重新出现也不会复活导入前的 enabled 决定。失败后不恢复旧 enabled；未来用户可再次明确启用。

### 7.3 包提升与目录发布

12. 释放 registry guard，但保持操作 gate。将完整阶段目录通过同卷、**不得替换现有目标**的目录重命名提升到新 `local/pkg-…`。不能用“先 exists 检查再普通可覆盖 rename”替代 no-replace 语义。
13. Windows 使用 `MoveFileExW` 且不设置 REPLACE_EXISTING 或 COPY_ALLOWED；Linux 使用 `renameat2(RENAME_NOREPLACE)`；macOS 使用 `renameatx_np(RENAME_EXCL)`。平台不提供符合契约的操作时失败，不退化为逐文件复制或覆盖。相关原生 API 封装于受测试的 filesystem adapter。Unix 使用持有的父目录句柄相对定位；Windows 始终固定源、目标祖先句柄，提升前关闭不允许 delete sharing 的阶段目录／文件读句柄，避免自身阻止重命名；此处不宣称抵抗同用户攻击者在最后窗口持续替换对象。提升前后分别校验对象身份，清理仍必须重新验证所有权。
14. 目录重命名成功是“包已导入”的提交点。Unix 随后尽力同步 local 与 import-staging 父目录；同步失败记录净化诊断但不能报告 notImported。Windows 保留 file sync 与原生重命名边界，不承诺等同 Unix 目录 fsync 的断电耐久性。
15. 保持 gate，在 blocking worker 执行完整本地扫描，然后在 registry 写锁中一次发布候选 local slice，并使用当前已提交 state 构建 snapshot。禁止乐观插入卡片或拼接旧 local slice。
16. snapshot 包含当前精确身份、来源 localDeclarative、disabled，且顶层可用时返回 imported。其他不相关包使扫描 degraded 不妨碍当前精确目标可见时的 imported；仍返回完整发现摘要。
17. 提升成功但最终扫描不可用、项目被修改／消失／发生 ID 冲突或无法确认其 disabled 身份时，发布能形成的合法权威结果并返回 importedNotVisible。不得删除已提升目录或回滚 disabled；用户通过 reload 恢复。若扫描 worker panic，以 local unavailable 发布空 local slice，保持 built-in。

步骤 7 保证一次正常最终发布不会 generation 溢出；所有 runtime 写入均经同一 gate，期间不存在其他 generation 发布。不可形成合法 envelope 的意外内部故障只返回净化错误，前端按“结果未知”处理而非当作确定未导入。

## 8. 失败、取消和崩溃矩阵

| 中断位置 | 可发现的目标 | 停用状态 | 客户端／重启语义 |
| --- | --- | --- | --- |
| 选择器取消、源无效、preview 取消或过期 | 无新增 | 不改变 | cancelled 或净化错误；可重新选择 |
| stale、冲突、发现／预算拒绝 | 无新增 | 不改变 | notImported / false；采用返回 snapshot，重新准备 |
| 暂存创建／写入／sync 失败 | 无新增 | 不改变 | notImported / false；仅清理本次已知暂存 |
| disabled 保存失败 | 无新增 | 旧状态完整保留 | notImported / false；绝不提升包 |
| disabled 成功后、提升前失败 | 无新增 | 当前身份停用，历史授权已清理 | notImported / true；不恢复旧 enabled |
| 提升失败、目标碰撞或跨卷 | 无新增 | 同上 | notImported / true；碰撞不覆盖，最多 4 次槽位尝试 |
| 提升成功、后续目录 sync 失败 | 完整包 | 停用 | 不报告未导入；继续扫描 |
| 提升成功、扫描无法确认 | 完整包曾成功提交，随后可能受外部变化影响 | 停用决定已提交 | importedNotVisible；重新扫描，不自动重装 |
| ready 时应用退出 | 无新增 | 不改变 | token 丢失，重新选择 |
| 暂存成功但 disabled 前崩溃 | 无新增，暂存不扫描 | 旧状态 | 不恢复暂存；重新选择 |
| disabled 后、提升前崩溃 | 无新增 | 从现有 state 恢复停用 | 暂存保留但不恢复导入 |
| 提升后、响应前崩溃 | 若文件系统保留提交则是完整包 | 从现有 state 恢复停用 | 首扫权威发现；token 不可重放 |

输入源从不修改。清理只针对本次调用独占创建且句柄／对象身份仍匹配的阶段文件与空目录，使用非递归 unlink/remove-dir；遇到未知文件、替换对象或链接即停止并保留证据。当前进程退出遗留内容不自动取得所有权。

不能把普通进程崩溃恢复扩大为任意断电下的双文件原子耐久保证。若底层文件系统丢失已同步状态、用户手工替换主文件或多个应用进程并发写入，已超出本阶段保证。自动恢复会按第 3.5 节清除次级候选中的本地 enabled；耗尽合法候选或遇到终止性错误时，全部插件仍按现有规则 blocked。

## 9. 并发与 runtime 边界

将现有 `reload_gate` 扩展为共享的 `operation_gate`，覆盖首次发现、显式 reload、set_enabled 和提交导入。保留 FIFO 接受顺序及 owned task 不因调用方取消而丢弃已接受操作的规则。

- prepare 的 OS 对话框、源读取和等待确认不持有 operation gate 或 registry 锁；使用独立单会话 admission。
- commit 从前置扫描到状态停用、包提升和最终发布持有 gate。
- set_enabled 必须也进入 gate，不能在 import 的 disabled 提交与提升之间重新启用同内容身份。
- reload 与首扫的已有 owned waiter 和 cancellation 测试必须保留。
- 普通已初始化 get 不扫描、不进入 gate，只读取已确认 snapshot；state unavailable 时的重试属于写入 registry 状态，需经 gate 再检查，不能穿插恢复提交。
- 同步文件操作放在 blocking worker；扫描期间不持 registry 锁。状态 clone/persist/commit 整段在同一个 blocking worker 内取得并释放 registry 的 blocking write guard，沿用现有事务语义；异步层只转交 runtime 的共享所有权，不能把已经取得的锁 guard 跨异步 suspension 或移动到其他 worker。state unavailable 的磁盘恢复同样遵守这一规则。
- get 可能在步骤 11 与 12 之间看到较新 revision、尚无目标的旧 generation。这是合法、已提交的状态，不是半包；最终 snapshot 再发布新 generation。
- 不允许 action 接受无限量后台任务：导入由单会话 admission 有界；沿用现有读／启停请求仲裁，不增加定时重试循环。

## 10. 错误闭集与传输校验

prepare/cancel/token-admission 错误仍使用精确 `{ code, message }`；前端只按 code 映射，绝不展示后端原 message、路径、OS 错误或源内容。

新增稳定错误码及中文映射：

| code | 显示文案 |
| --- | --- |
| `plugin_import_busy` | 已有清单导入操作，请先完成或取消。 |
| `plugin_import_dialog_unavailable` | 无法打开文件选择器，请重试。仅用于宿主可观察的窗口、适配器、channel 或后台任务故障；普通 `None` 是 cancelled。 |
| `plugin_import_source_rejected` | 无法安全读取所选文件，请选择普通本地 JSON 文件。 |
| `plugin_import_manifest_invalid` | 清单格式或内容不符合当前插件要求。 |
| `plugin_import_token_invalid` | 预览已过期或失效，请重新选择清单。 |
| `plugin_import_id_conflict` | 已存在相同插件 ID；当前不支持覆盖或更新。 |
| `plugin_import_discovery_unavailable` | 请先修复本地插件发现问题，再导入清单。 |
| `plugin_import_capacity_exceeded` | 本地插件数量或读取预算已达上限。 |
| `plugin_import_staging_capacity_exceeded` | 导入暂存区需要人工检查和清理。 |
| `plugin_import_write_failed` | 无法完成清单写入，请检查后重试。 |
| `plugin_import_publication_unconfirmed` | 清单已写入，但目录结果尚未确认，请重新扫描。 |

`LocalManifestImportCommitFailure` 是以下 code 的闭集：`plugin_catalog_stale`、`plugin_catalog_invalid`、`plugin_catalog_generation_exhausted`、`plugin_state_unavailable`、`plugin_state_persist_failed`、`plugin_state_capacity_exceeded`、`plugin_revision_exhausted`、`plugin_import_id_conflict`、`plugin_import_discovery_unavailable`、`plugin_import_capacity_exceeded`、`plugin_import_staging_capacity_exceeded`、`plugin_import_write_failed`。不允许 publication_unconfirmed 混入 notImported。

非法源包括长度超限、链接和 URI；manifest_invalid 包括 JSON/schema/语义或 canonical 输出超限。字段验证细节不通过错误文本回显。

前端校验 imported snapshot 中存在准确 pluginId、localDeclarative、disabled、预览的所有 manifest 字段一致；不能仅按 ID 认定成功。importedNotVisible 不强求目标存在。notImported 无 pluginId 字段；其 disabledDecisionSaved 必须为布尔值，非写盘前置错误只能为 false。非法响应统一进入结果未知，不乐观变更目录。

## 11. 前端 UX 与可访问性

“已安装插件”和“插件管理”共享页头增加“导入本地清单”，市场页不提供该入口。已有“重新扫描本地插件”保留；导入操作不会跳到市场，也不会制造市场条目。

流程状态为 idle → choosing → preview → committing → result；本地视图维护同一份 Pinia 会话，不因分区切换重开选择器。

preview 使用宿主 Vue 对话框而非清单 HTML，显示名称、ID、版本、description、publisher、publisherId，以及固定说明：

> 发布者信息由清单作者填写，未经认证。本次只复制元数据，不运行代码或授予权限。导入后默认停用，启用仅记录宿主偏好。

“确认导入”与“取消”是独立按钮；不预先选中信任框，不提供“导入并启用”。确认发生时传 token 自带的 generation，store 的后来 generation 不能替换它。更高 generation 到达时将 preview 标记失效，禁用确认并要求重新选择；即使前端漏掉，后端仍拒绝。

- 对话框具有 `role=dialog`、`aria-modal`、标题与描述关联，焦点约束在对话框内；初始焦点放在标题或取消按钮，不默认执行确认。
- Escape 和取消在 preview 阶段清除 token 并恢复触发按钮焦点；committing 不提供伪取消，Escape 不关闭仍在处理的确认面板。
- choosing 的原生取消不显示错误；committing 用 `role=status`、`aria-busy`，禁用重复确认与导入入口。
- 所有字符串普通文本渲染，长 ID 可换行；方向隔离避免文案干扰邻近宿主控件，不使用 HTML 或自动链接。
- 到期倒计时仅为提示；不得用高频 aria-live 播报，每秒读数也不作为后端安全判断。
- 页面卸载时 ready 会话发 best-effort cancel 并丢弃迟到 preview；若 prepare 迟到返回 token，再 cancel 该 token。committing 的 store 后台 flight 继续，返回 snapshot 仍送交权威仲裁，但不强制移动当前页面焦点。
- imported：采用 snapshot 后显示“清单已导入，默认停用”；可以在已安装视图聚焦准确卡片，但不切换启用状态。
- notImported / true：明确说明“导入未完成；该清单的停用偏好已保存，旧启用不会恢复”。false 显示对应原因，不声称固定目录绝无变化。
- importedNotVisible 或传输结果未知：提示重新扫描，不自动重试导入。

新增 import flight 有独立所有权，但 snapshot 统一进入现有 generation/revision 仲裁入口；更旧 generation 整体忽略、同代逐项 revision 合并、迟到 mutation 不复活项目等 Phase 1A 保证不变。

## 12. 文件与接口责任

```text
src-tauri/src/plugin/import.rs               单会话、token、内容快照、领域结果
src-tauri/src/plugin/import/dialog.rs        可注入的原生单文件选择适配器
src-tauri/src/storage/local_plugin_import.rs 安全暂存、独占提升、有限清理
src-tauri/src/plugin/discovery/safe_fs.rs    共享 no-follow 原语及有界扫描统计
src-tauri/src/plugin/record.rs              复用 canonical bytes 与原有 fingerprint
src-tauri/src/storage/plugin_state.rs       次级候选本地停用规范化与 rewrite 标记
src-tauri/src/plugin/registry.rs            内部未发现身份的 explicit-disabled 事务
src-tauri/src/plugin/runtime.rs             operation gate 与完整导入发布协调
src-tauri/src/commands/plugin.rs            三个新增固定 IPC 与净化映射
src-tauri/build.rs                          app manifest 登记三个命令
src-tauri/src/lib.rs                        command handler、Rust dialog plugin、ACL 回归
src-tauri/capabilities/plugin-runtime.json   仅 local main 的新增 allow permission
src-tauri/Cargo.toml / Cargo.lock            Rust dialog 与必要平台 API 依赖
.github/workflows/ci.yml                    Windows/Linux/macOS 导入安全回归
src/types/plugin.ts                        导入协议 discriminated unions
src/services/pluginService.ts              精确 parser、命令适配、错误文案
src/stores/plugin.ts                       单会话 flight 与 snapshot 仲裁复用
src/components/plugins/PluginImportDialog.vue 内容确认、焦点与错误语义
src/components/plugins/PluginMarketplacePage.vue 入口与结果编排
docs/plugin-local-manifests.md              导入教程、结果语义、暂存人工维护边界
```

`LocalManifestSelector` 只返回 selected/cancelled/failed，其中 failed 限于宿主可观察的窗口、适配器、channel 或后台任务故障；`LocalManifestReader` 输出已验证 record 与 canonical bytes；`LocalManifestImportStorage` 提供 prepare-stage / promote-no-replace / cleanup-owned-stage。测试通过这些边界注入故障，不使用真实用户选择器或配置目录。

存储 adapter 的 staging handle 是带对象身份的内部 ownership token，不是裸路径字符串；只用于本次创建内容。manifest fingerprint 不承担文件系统所有权职责。

`AtomicFile` 是 state 的替换原语，不是安全包安装器；不能把其普通路径打开／替换逻辑直接用于不受信任源或包目录。

## 13. ACL 与安全回归

三个新应用命令同时在 build app manifest 和 invoke handler 注册，由自动生成的最小 allow permission 引用。`plugin-runtime` 仍仅 `webviews: ["main"]`、本地内容，无远程 URL、通配符或其他窗口继承。

不授予 `dialog:default`、`dialog:allow-open`、文件系统 scope、动态 command 或新事件频道。后端 dialog plugin 作为构建期 Tauri framework 依赖不属于用户应用插件，不因此扩张用户清单能力。

ACL 测试检查所有 capability 的联合权限，不只断言单一 JSON 文件。前端不存在路径参数、原始 manifest 提交方法或可绕过 preview 的 direct-install 命令。

## 14. 验收测试矩阵

| 层级 | 必须覆盖的情形 |
| --- | --- |
| 严格输入 | 16 KiB 边界与 +1；UTF-8/BOM；顶层数组；重复／未知／缺失字段；非空贡献和能力；规范输出超限；错误类型与有效 SemVer |
| 平台读取 | 普通单链接文件；文件／祖先 symlink、hard link、Windows junction/reparse、UNC/设备/ADS；Unix 无写端 FIFO 有界类型拒绝；打开后类型检查 |
| 原生选择 | 单文件；`None` 取消无 token；可观察的窗口／适配器／channel 故障；URI 拒绝；后台 callback；同时第二个 prepare busy；UI 离开后迟到 token 清理 |
| 内容绑定 | preview 后源修改／删除仍导入准确捕获内容；JSON 排版差异同 fingerprint；任何语义变化不同 fingerprint；源路径不进入响应、日志或持久化 |
| 令牌 | malformed/unknown/replay；299 秒有效、300 秒失效；单调时钟不受系统时间倒退影响；单 ready 容量；cancel 幂等；commit 取得所有权后不可取消 |
| generation | token 绑定 generation 不可替换；等待期间 reload 导致 stale；preflight 检出磁盘变化先发布后拒绝；MAX 拒绝，MAX-1 可完成一次发布 |
| 冲突与容量 | built-in/local 同 ID；同内容重复导入；local/local 冲突导致 degraded 拒绝；128 包、256 条目、2 MiB 上限；staging 16 项；所有碰撞重试上限 |
| 默认停用 | 全新身份；相同 orphan enabled；旧 publisher 与旧 fingerprint enabled；已有 disabled no-op；移除旧身份后容量成功；保存失败保留所有旧状态；主状态有效时保留本地 enabled；从 `.tmp` / `.bak` 恢复时只清除本地 enabled、保留 built-in；恢复读取本身不写盘；相同 disabled 决定仍触发 rewrite；rewrite 失败保留规范化内存与 rewrite 标记；revision 为 `u64::MAX` 时 rewrite-only 仍不递增并可成功持久化 |
| 暂存与提升 | 每个创建、写、sync、rename 故障；同卷约束；原子 no-replace；未知目标不覆盖；清理拒绝替换对象和额外文件；暂存永不进入扫描 |
| 崩溃恢复 | 在 disabled 前后、提升前后、publication 前后模拟进程终止；重建 runtime 后不恢复 token 或暂存；完整包与 state 恢复符合第 8 节 |
| 并发 | barrier 控制 commit/reload/toggle/get 顺序；toggle 不穿过 disabled/提升窗口；等待取消不打乱 FIFO；调用方消失后接受的 commit 仍完成；不复制旧 state 发布 |
| 发布与失败 | 成功返回完整 v2 snapshot；后扫失败清空 local 但保留 built-in；外部变更导致 importedNotVisible；postcommit sync 失败不报告 notImported；未知传输结果不自动重发 |
| 前端 parser/store | 所有 union 精确 keys；未知状态/code；token/generation 格式；success manifest 精确匹配；旧 snapshot 与迟到 mutation；不同操作 flight 不误合并 |
| UI/a11y | 两个本地入口与市场排除；标题／焦点恢复／Escape；非 HTML 文案；无导入并启用；各失败文案；结果未知；导航后不抢焦点；状态不是仅靠颜色 |
| ACL | 六个插件 command 最小注册与授权；仅 local main；所有 capability 联合无 dialog/fs/remote 放宽；无前端路径接口 |

全部测试必须使用注入临时根与测试夹具。平台安全测试在对应 Windows、Linux、macOS CI 运行；不以单平台通过代替其他平台 no-replace 和 no-follow 验证。

回归执行项目现有命令：`pnpm test`、`pnpm exec vue-tsc --noEmit`、`pnpm build`、`pnpm lint`，以及在 `src-tauri` 执行 `cargo test --locked --all-targets`、`cargo clippy --locked --all-targets`；仓库没有独立 `typecheck` script，不能写成 `pnpm typecheck`。全仓 clippy 必须退出成功，本次新增或修改的 Rust 路径不得产生新 warning；对新增与修改 Rust 文件单独检查格式，不把现有无关 warning 或格式债务混入本功能。桌面冒烟覆盖真实原生选择、取消、导入成功、重启默认停用和主业务页面仍可使用。

## 15. 交付完成判据

1. 用户不需要知道配置路径或生成 pkg 槽位即可导入一份合法清单。
2. 确认内容与写入内容严格相同，且没有源路径或任意文件读写 IPC。
3. 任一受支持的单进程失败路径均不把旧 enabled 继承给导入内容；允许的部分状态明确呈现。
4. catalog 只由权威扫描发布，不用 UI 乐观更新伪造已安装成功。
5. 原有手工包、来源显示、空权限、错误隔离、state 恢复与并发仲裁测试保持通过。
6. 没有引入卸载、更新、签名、执行或通用信任能力。

## 16. 技术依据

- [Tauri Dialog plugin](https://v2.tauri.app/plugin/dialog/)：原生选择器的 Rust 宿主集成。
- [Rust FileDialogBuilder](https://docs.rs/tauri-plugin-dialog/latest/tauri_plugin_dialog/struct.FileDialogBuilder.html)：callback 单文件选择接口。
- [Microsoft MoveFileExW](https://learn.microsoft.com/zh-cn/windows/win32/api/winbase/nf-winbase-movefileexw)：Windows 原生目录移动及覆盖／跨卷标志语义。
- [Linux rename(2)](https://man7.org/linux/man-pages/man2/rename.2.html)：`RENAME_NOREPLACE` 与文件系统不支持时的错误。
- [Apple exclusive renaming support](https://developer.apple.com/documentation/foundation/urlresourcekey/volumesupportsexclusiverenamingkey)：卷的 exclusive rename 支持不是所有文件系统均可假定的能力。
- 仓库现有 `plugin/discovery/safe_fs.rs`、`plugin/record.rs`、`plugin/runtime.rs`、`plugin/registry.rs`、`storage/atomic_file.rs` 与两份已提交插件设计：当前安全、持久化和并发基线。

本文对资源、确认、失败关闭和单进程范围作产品架构裁决；引用原生对话框能力不意味着平台自动提供本设计的路径校验、包原子提升或状态事务。
