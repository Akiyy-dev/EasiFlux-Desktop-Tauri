# EasiFlux 本地声明式插件发现设计

**日期：** 2026-09-07

**状态：** 已批准，进入实现

**所属 PRD：** PRD-11 插件系统 UI（Plugin Marketplace）

**基线：** `plugin/runtime-foundation` / `4720437`

**后续分支：** `plugin/local-manifests`

## 1. 目的与关系

本文细化《EasiFlux 插件系统设计》的阶段 1A。它在阶段 0 的严格内置目录、启用状态持久化、固定 IPC 与三分区 UI 之上，加入固定目录中的本地声明式清单发现。

本阶段的核心结果是：用户把符合约束的包目录放到 EasiFlux 固定配置目录后，插件页可以发现、展示并记录该清单的启用偏好。宿主不会加载或执行包内代码，也不会解释任何贡献、脚本、HTML、原生库或资源。

本文覆盖阶段 1 的“发现与管理”部分。真正的声明式贡献执行、安装器、签名市场与代码沙箱仍属于后续独立设计。

## 2. 目标与验收结果

1. 从唯一、宿主决定的目录发现本地清单；前端不能传入路径。
2. 对目录项、文件形状、字节数、JSON 结构和清单语义执行有界验证。
3. 单包错误只隔离该包；目录级错误不阻断内置插件或主程序。
4. 本地来源不能覆盖内置来源；重复本地 ID 没有隐式赢家。
5. 本地包内容变化不能静默继承旧启用选择。
6. 重新扫描与启停操作具有明确并发协议，旧响应不能复活已移除或已替换的包。
7. 状态文件从严格 v1 向严格 v2 无损迁移，并继续拒绝未来未知 schema 的降级读取。
8. UI 明确标注“已发现，未执行”，并且只显示聚合、已净化的发现状态。

## 3. 明确非目标

- 不下载、安装、更新、卸载或回滚插件。
- 不接收 URL、文件路径、目录选择结果或拖放路径。
- 不扫描任意目录，不递归，不启用文件监听器。
- 不读取清单以外的图片、样式、脚本、HTML、DLL、dylib、so、WASM、压缩包或其他资源。
- 不放宽 `contributions`、`requestedCapabilities` 或 `grantedCapabilities`；三者仍必须为空。
- 不注册动态 Tauri command，不建立插件事件频道，不调用插件生命周期。
- 不认证本地清单自报的发布者；`publisherId` 只是未验证的命名字段。
- 不把“启用”解释为执行。它只是一条与已验证清单身份绑定的宿主偏好。

## 4. 方案选择

### 4.1 每次读取目录时扫描

实现较短，但会把文件系统延迟和错误耦合到普通读取，并让刷新与启停之间的竞态缺少明确边界，因此不采用。

### 4.2 首次读取惰性发现，加显式重新扫描

本方案被采用。首次 `get_plugin_catalog` 在串行刷新门内完成一次发现；之后普通读取不重复扫描。用户通过固定的 `reload_plugin_catalog` 命令显式重新扫描。

### 4.3 先导入到宿主管理的安装仓库

它能提供更强的包不可变性，但同时需要事务式安装、所有权、清理、恢复和签名策略，超出本阶段范围，留到市场安装阶段。

## 5. 固定目录与包格式

沿用阶段 0 已经落盘的应用配置根，不能无迁移地切换到 bundle identifier 目录：

```text
<platform config_dir>/EasiFlux Desktop/plugins/
├── state.json
├── state.json.tmp
├── state.json.bak
└── local/
    └── <slot>/
        └── manifest.json
```

生产路径仍由后端通过 `dirs::config_dir()/APP_NAME` 解析；测试通过依赖注入使用临时目录。前端 IPC 没有路径参数。

`<slot>` 只是发现槽位，不是插件身份，也不要求等于 manifest ID。它必须精确匹配 `pkg-<32 lowercase hex>`，例如 `pkg-550e8400e29b41d4a716446655440000`。未来安装器可用 UUID v4 的 simple 表示生成它；阶段 1A 的手工包同样遵守这个格式，但不校验 UUID version/variant bit。

这个不透明 slug 不进入 IPC、状态身份或指纹。它天然排除点、路径分隔符、ADS、尾随点/空格、Unicode、大小写归一化冲突，以及 Windows 把 `CON`、`COM1`、`NUL` 与带扩展形式仍视作设备名的问题。合法 reverse-domain 插件 ID 仍可能包含这些设备名前缀，因此不能直接拿 manifest ID 当目录名。

每个槽位必须是一个真实的直接子目录，且直接内容必须恰好是一个大小写精确的普通文件 `manifest.json`。额外文件、嵌套目录、链接、重解析点或任何其他内容都会让该槽位被拒绝。

## 6. 资源上限

| 资源 | 上限 | 超限语义 |
|---|---:|---|
| `local/` 的直接项总数，包括垃圾项 | 256 | 整次发现 `unavailable` |
| 可接受的本地包数 | 128 | 整次发现 `unavailable` |
| 单个 `manifest.json` | 16 KiB | 拒绝该包 |
| 一次发现读取的 manifest 总字节数 | 2 MiB | 整次发现 `unavailable` |
| 状态文件 | 保持 256 KiB | 状态子系统 `unavailable` |
| 保留的状态身份 | 保持 512 | 拒绝该次状态修改 |

文件大小从已经打开的句柄按 `limit + 1` 读取验证，不能只相信打开前 metadata。目录项达到第 257 个时立即停止；不能先无界收集再截断。目录级上限不允许“接受前 N 个”，避免枚举顺序决定结果。

## 7. 发现与验证算法

1. 解析唯一固定根。`local/` 不存在表示一次成功的空发现。
2. 对配置链、`plugins/` 和 `local/` 做不跟随检查；存在时都必须是真实目录。
3. 只枚举 `local/` 的直接子项，在有界计数内收集并按 slot 的 ASCII 字节序排序。
4. 校验 slot 名称；无效直接项作为一个被拒绝包计数。
5. 以不跟随语义打开 slot，并确认打开后的对象仍是目录且不是 symlink/reparse point。
6. 只枚举 slot 的直接内容；必须恰好得到普通文件 `manifest.json`。
7. 以不跟随语义打开 manifest，并从打开句柄再次确认对象类型和平台链接属性；Unix 打开必须同时使用非阻塞语义，确保伪装成 manifest 的 FIFO 在类型验证前不会等待写端。
8. 有界读取；要求严格 UTF-8，拒绝 BOM，并要求恰好一个 JSON 文档。
9. 使用 map-only 严格解码器反序列化 `PluginManifestV1`：拒绝顶层数组、重复字段、未知字段、缺失字段与错误类型。
10. 执行阶段 0 的全部语义验证：schema 1、规范 ID、SemVer、显示文本字节上限，以及空贡献与空请求能力。
11. 宿主创建 `PluginRecord` 并指定来源；manifest 文件不能声明或覆盖 `source` 与 fingerprint。
12. 对本地重复 ID 拒绝全部竞争包；按 canonical `PluginId` 排序候选。
13. 在 registry 合并时拒绝与 built-in ID 冲突的本地包；内置项保留。
14. 将完整候选作为一次原子 local slice 发布，绝不与上次结果增量拼接。

在 Unix 上生产读取使用 handle-relative 的 `O_NOFOLLOW`；manifest flags 至少包含 `RDONLY | NOFOLLOW | CLOEXEC | NONBLOCK | NOCTTY`，打开后先 `fstat`，只有普通文件才允许读取。在 Windows 上拒绝所有 `FILE_ATTRIBUTE_REPARSE_POINT`，并使用重解析点感知的打开语义。禁止把“先 metadata 检查，再普通 `File::open(path)`”当作安全保证。

这套边界防止普通链接逃逸和意外越界，但不宣称能抵抗已经拥有同一用户配置目录写权限的攻击者持续竞态改写。阶段 1A 从已验证内存快照展示元数据，之后不再打开、更不会执行包内容；一旦未来加入执行能力，必须升级为内容不可变安装与执行时校验。

## 8. 信任记录与本地指纹

Registry 不再只保存 manifest，而保存宿主构造的记录：

```rust
struct PluginRecord {
    manifest: PluginManifestV1,
    source: PluginSource,
    approval_fingerprint: String,
}
```

来源闭集扩展为：

```text
builtIn
localDeclarative
```

身份仍为：

```text
id + source + publisherId + approvalFingerprint
```

- built-in 使用 `v1:none`。
- localDeclarative 使用 `v1:sha256:<64 lowercase hex>`。

本地指纹不是签名，也不证明 `publisherId` 的真实性。它通过域分隔 SHA-256 绑定已验证清单的全部语义字段：

```text
domain = "EasiFlux.localDeclarative.manifest.v1\0"
input  = domain || canonical_manifest_v1_json
```

`canonical_manifest_v1_json` 由字段顺序冻结的 Rust DTO 序列化，不能直接哈希原始文件。因此仅空白和 JSON 键顺序变化不会改变身份；任一语义字段变化都会得到新身份并默认停用。路径和 slot 名不进入指纹。

## 9. 状态文件 v2 与迁移

新写入格式为：

```json
{
  "schemaVersion": 2,
  "revision": "4",
  "entries": [
    {
      "id": "com.example.plugin",
      "source": "localDeclarative",
      "publisherId": "com.example",
      "approvalFingerprint": "v1:sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "enabled": true
    }
  ]
}
```

迁移规则：

1. v1 使用独立的严格 wire decoder，仍只接受 `builtIn + v1:none`；扩展公共 `PluginSource` 不能让 v1 意外接受本地来源。
2. 合法 v1 在内存中无损转换为 v2，保留 revision、built-in 选择与所有有界 orphan。
3. 读取本身不主动写盘。下一次成功的显式状态决策以原子语义写回 v2；只发生格式改写时保留 revision。
4. v2 对 source 与 fingerprint 组合做严格验证，并拒绝重复复合身份。
5. 任一更高 schema 的高优先级恢复候选都是终止性错误，不能回退旧 `.tmp` 或 `.bak` 绕过未来版本。
6. 写盘仍遵守“构造下一状态 -> 校验大小与条目数 -> 原子保存 -> 提交内存”。保存失败不能提交状态、revision、身份清理或迁移标记。

显式启停当前本地记录时，先在候选状态中删除同 `(id, source)` 的所有旧 publisher/fingerprint 身份，再写入当前身份和目标布尔值。即使当前新身份表面上已经是 disabled，用户的显式 disabled 决策也要清理旧 enabled 身份，防止旧文件回退时复活旧授权。只有规范候选与当前状态完全相同且没有待迁移格式时，才允许 no-op。

未发生新显式决策时，暂时消失后又以完全相同内容身份恢复的包可以恢复原启用选择。这表示同一内容身份恢复，不表示替换内容继承授权。

## 10. Transport v2

Manifest 保持 schema 1；catalog snapshot 与 mutation envelope 升为独立的 schema 2。

```ts
interface PluginCatalogSnapshotV2 {
  schemaVersion: 2
  revision: string
  catalogGeneration: string
  availability: 'available' | 'unavailable'
  availabilityReasonCode: 'stateUnavailable' | 'catalogInvalid' | null
  localDiscovery: {
    status: 'available' | 'degraded' | 'unavailable'
    rejectedPackageCount: number
  }
  plugins: PluginCatalogItem[]
}

interface PluginCatalogMutationResultV2 {
  schemaVersion: 2
  revision: string
  catalogGeneration: string
  plugin: PluginCatalogItem
}
```

`revision` 继续只描述持久化启用状态。`catalogGeneration` 是进程内单调递增的 `u64` 字符串，描述已发布目录的成员、内容身份和本地发现健康摘要。两者都必须是规范十进制字符串。

- 内部初始 generation 为 0；首次发现尝试发布 generation 1，即使结果是成功的空目录。
- local records 或发现摘要变化时递增一次；完全相同的重扫不递增。
- 启停只改变 revision，不改变 generation。
- generation 溢出返回 `plugin_catalog_generation_exhausted`，不发布部分状态。

本地发现摘要约束：

- `available` 必须有 `rejectedPackageCount = 0`；缺失根属于此状态。
- `degraded` 必须有 1 至 256 个被拒绝包；有效包仍发布。
- `unavailable` 使用 0；目录无法完整枚举或目录级预算超限时不能声称完整拒绝数。

## 11. Registry 合并与降级

Registry 分开保存 `builtins` 与 `locals`，最终目录按 canonical ID 字节序合并。

- 内置清单无效或内置重复继续使全局 catalog `unavailable / catalogInvalid`。
- 状态存储故障继续使全局 catalog `unavailable / stateUnavailable`，并阻止全部启停。
- 单个本地包错误产生 `degraded`，只排除对应包。
- 本地与内置冲突只排除本地项，并增加聚合拒绝数。
- 多个本地包同 ID 时排除全部竞争项，并按包数增加聚合拒绝数。
- 根目录权限、枚举、链接或目录级预算错误产生 local `unavailable`，清空当前 local slice，但保留 built-ins 与持久化选择。
- local `unavailable` 不改变顶层 availability，也不阻止 built-in 启停。

恢复扫描后，只有完整身份相同的本地项恢复原选择；变化后的内容保持默认停用。

## 12. Runtime 与并发协议

`PluginRuntime` 负责 registry、发现器与刷新串行化：

```rust
struct PluginRuntime {
    registry: RwLock<PluginRegistry>,
    discovery: Arc<dyn LocalPluginDiscovery>,
    reload_gate: Mutex<()>,
    initial_discovery_attempted: AtomicBool,
}
```

具体约束：

1. 首次 `get_catalog` 在 reload gate 内二次检查初始化标记并发现一次；失败结果也算已尝试，之后只能由显式 reload 重试。
2. 同步文件系统扫描在 blocking worker 中执行；扫描期间不持有 registry 锁。
3. 多个 reload 通过 gate 串行；每次构造完整候选后，在短暂 registry 写锁中一次发布并基于当前状态生成 snapshot。
4. `set_plugin_enabled` 在同一个 registry 写锁下解析并比较 `expectedCatalogGeneration`，再完成 clone、原子持久化和内存提交。
5. generation 不匹配返回 `plugin_catalog_stale`，不得自动把旧用户操作重放到新 generation。
6. 发现结果不能携带扫描开始时复制的状态；发布时必须使用 registry 中当前已提交状态。

该协议覆盖已经发布的目录快照之间的 reload/toggle 竞态。它不把磁盘目录变成不可变安装仓库；由于本阶段不执行磁盘内容，这一限制是明确的非执行边界。

## 13. 固定 IPC 与 ACL

仅保留三个固定命令：

```text
get_plugin_catalog()
reload_plugin_catalog()
set_plugin_enabled(id, enabled, expectedCatalogGeneration)
```

三者只授权给 local `main` WebView。不能加入 wildcard、动态命令名、任意 path scope 或 event subscription。普通 get 可以继续尝试恢复状态存储，但不会隐式重复扫描本地目录。

新增稳定错误码：

- `plugin_catalog_stale`
- `plugin_catalog_generation_exhausted`
- `plugin_state_capacity_exceeded`

目录和单包常规失败通过 snapshot 的 `localDiscovery` 表示，不把 OS 错误、路径或原始 JSON放入 IPC。blocking task 异常等无法形成合法 snapshot 的内部故障仍映射为已有净化的插件目录错误。

## 14. 前端状态仲裁

TypeScript 将所有 v2 IPC 数据继续视为不受信任输入，并对 key、枚举、整数、排序及状态组合做精确校验。

Pinia store 同时保存 `revision` 与 `catalogGeneration`：

- 更旧 generation 的 snapshot 整体忽略。
- 更高 generation 的 snapshot 整体替换成员、manifest、source、发现摘要和每项确认 revision；不能保留旧 generation 项。
- 相同 generation 才沿用阶段 0 的逐项 revision 合并与 mutation owner 仲裁。
- mutation 必须返回调用时的 generation；只有它仍等于 store 当前 generation 且请求 owner 仍有效时才能更新项目。
- reload 后迟到的旧 mutation 不能重新插入被移除项，也不能覆盖同 ID 的新内容。
- 初次 `load()` 调用普通 get；显式 `reload()` 调用 reload command。两类 flight 分别合并，不能误把不同语义的请求当作同一次调用。

错误文案继续只按闭集 code 映射，不能回显后端 message。

## 15. UI 语义

- 已安装：显示 built-in 与已发现的 localDeclarative。
- 市场：仍只显示 built-in 可信目录；本地清单不是市场条目。
- 管理：显示两种来源与相同的权威状态。
- 来源文案集中映射：
  - builtIn：`内置 · 随应用提供`
  - localDeclarative：`本地声明式包 · 已发现，未执行`
- 页面各分区都提供“重新扫描本地插件”，扫描中禁用，不移动焦点。
- degraded 使用 `role="status"`，只显示聚合数量。
- unavailable 使用 `role="alert"`，说明本地目录暂不可用且内置插件仍可使用。
- 本地项明确说明开关只记录宿主偏好，不会运行代码。
- 状态不能只通过颜色表达；现有键盘开关、错误关联与标题焦点行为保持不变。

## 16. 测试策略

### 16.1 Rust

- map-only manifest、重复/未知字段、BOM、UTF-8、非空贡献或权限。
- slot、形状、直接子项、大小和总预算边界。
- Unix symlink/FIFO 与 Windows reparse-point 拒绝；无写端 FIFO 必须有界返回，打开前后对象检查。
- 单包隔离、local/local 全部拒绝、built-in/local 低信任方拒绝。
- 指纹规范化、语义变化、来源与发布者隔离。
- state v1 -> v2、orphan 保留、future schema 终止、容量与原子失败。
- 首次发现、重复 get、显式 reload、相同结果 generation 不变。
- barrier 控制的 reload/toggle 排序、stale generation 与保存失败。
- local unavailable 清空 local，但 built-in 和核心启动保持可用。
- 三条命令只授权给 local main WebView。

### 16.2 TypeScript 与 Vue

- 严格接受 v2、拒绝 v1/额外 key/未知 source/非法 generation/非法发现摘要。
- mutation payload 必须含 expected generation，reload 使用精确命令名。
- 新 generation 替换、旧 snapshot 忽略、旧 mutation 不复活项目。
- 相同 generation 下的 revision 与并发 owner 回归。
- 两种来源文案、三个分区成员规则、reload loading 与可访问性。
- degraded/unavailable 只显示净化聚合信息。
- 页面仍不存在安装、下载、更新、卸载或执行控件。

## 17. 交付顺序

1. 锁定 trust record、transport v2 与 state v2。
2. 实现有界、平台感知的不跟随发现器。
3. 接入 registry 合并、generation 与身份状态机。
4. 加入 runtime、惰性首扫、显式 reload 与 ACL。
5. 升级严格前端 parser/store。
6. 完成来源、刷新和发现健康 UI。
7. 运行全量 Rust、前端、类型、构建、lint 与安全回归。

## 18. 参考资料

- [Tauri PathResolver](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html)
- [Rust `read_dir`](https://doc.rust-lang.org/std/fs/fn.read_dir.html)
- [Rust Unix `OpenOptionsExt`](https://doc.rust-lang.org/std/os/unix/fs/trait.OpenOptionsExt.html)
- [Rust Windows `OpenOptionsExt`](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html)
- [Microsoft file attribute constants](https://learn.microsoft.com/windows/win32/fileio/file-attribute-constants)
- [Microsoft reparse-point file operations](https://learn.microsoft.com/windows/win32/fileio/reparse-points-and-file-operations)
- [RFC 8259: JSON](https://www.rfc-editor.org/rfc/rfc8259)
