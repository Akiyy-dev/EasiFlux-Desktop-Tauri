# EasiFlux 插件系统设计

日期：2026-09-07  
基线：`main@8aa8d1c`  
设计分支：`plugin/system-design`

## 1. 背景与目标

EasiFlux 已经具备插件导航入口、插件页的三个分区（已安装、市场、管理）以及 Rust 侧的最小 `Plugin`/`PluginRegistry` 骨架，但当前页面仍是占位内容，后端也没有可供前端调用的插件目录、状态持久化或安全边界。

本设计的目标是建立一个能逐步演进、默认安全的应用级插件系统：

1. 让用户能查看插件目录、启用或停用已经受信任的插件，并看到明确的状态与错误。
2. 为未来的本地声明式插件包和签名市场插件保留稳定的数据模型。
3. 将 EasiFlux 业务插件与 Tauri 框架插件严格分开。
4. 在任何第三方代码执行能力出现前，先建立清单校验、能力声明、持久化和 UI 管理面。
5. 插件子系统失败时不影响行情、交易、设置、通知等核心功能启动。

## 2. 非目标

阶段 0 明确不支持：

- 动态加载 DLL、dylib、Rust crate 或 Tauri plugin。
- 执行第三方 JavaScript、HTML、模板、Shell 或任意本地程序。
- 使用现有 `Plugin` trait、`Box<dyn Plugin>` 或 `on_init` 生命周期来激活用户可见插件；该占位骨架在阶段 0 删除。
- 从远程地址下载、安装或静默更新插件。
- 向插件暴露任意 Tauri IPC、文件系统、网络、数据库、密钥环或交易指令权限。
- 让插件直接修改应用路由、全局样式、窗口安全策略或 CSP。
- 在插件清单中接受未知字段并“尽量运行”。
- 把现有业务模块为了展示效果而伪装成插件。

## 3. 名词与边界

### 3.1 Tauri 框架插件

Tauri 框架插件属于应用编译依赖，通常由 Rust crate 和可选的前端 API 组成，可以接入生命周期并暴露 IPC 命令。它们只能由 EasiFlux 开发者在构建时引入，不属于用户可安装的市场插件。

### 3.2 EasiFlux 应用插件

本设计中的“插件”是 EasiFlux 自己定义和管理的应用扩展。宿主只暴露固定的、经过审计的能力集合；插件不能自行注册 Tauri 命令，也不能绕过宿主访问系统资源。

### 3.3 插件目录、安装与启用

- 目录：宿主已知的插件元数据集合。
- 安装：某个插件版本的受信任内容已经存在于本机。阶段 0 只有编译进应用的内置条目，因此没有安装操作。
- 启用：用户允许宿主激活该插件声明的贡献。启用状态不等于安装状态。

## 4. 方案比较

### 方案 A：直接加载原生动态库

优点是性能高、能复用 Rust/C/C++ 生态；缺点是 ABI、升级、崩溃隔离和供应链风险都很高，插件获得的进程权限几乎等同宿主。该方案不适合作为第一版。

### 方案 B：WebView 内执行第三方前端代码

优点是开发门槛低、UI 扩展自由；缺点是需要同时解决 CSP、依赖隔离、IPC 权限、XSS、资源限制和升级可信度。直接在主 WebView 执行远程或本地 JS 会扩大攻击面，不作为第一版。

### 方案 C：宿主解释声明式清单，代码能力按阶段引入

插件先以严格版本化清单描述身份、能力和贡献，由宿主渲染 UI 并执行固定动作。阶段 0 只接受编译时内置清单；后续可加入只读本地清单、签名市场包，最后再单独评估沙箱化 WASM。

**决策：采用方案 C。** 这是能够尽早交付可用管理体验，同时保持权限最小化和后续兼容性的路径。

## 5. 分阶段路线

### 阶段 0：运行时基础与管理 UI（本次实现）

- Rust 中定义严格的 v1 插件清单、标识符和状态模型。
- 使用编译时内置目录；允许目录为空，不制造虚假演示插件。
- 使用独立文件持久化启用状态。
- 暴露固定 IPC：获取目录、设置启用状态。
- 完成已安装、市场、管理三个前端视图及加载、空态、错误态、搜索和筛选。
- 所有核心逻辑通过测试夹具验证，即使生产目录暂时为空。

### 阶段 1：本地声明式插件包

- 只扫描固定目录的直接子项，不递归，不跟随符号链接。
- 限制清单与资源的大小、数量和文件类型。
- 本地包仍然不能携带可执行代码；只能使用宿主允许的声明式贡献。
- 发现错误按单插件隔离，不阻断其他插件或主程序。

### 阶段 2：签名市场

- 市场索引和插件包均使用版本化、可验证的签名元数据。
- 下载到临时位置，校验摘要、签名、身份、版本和兼容性后原子安装。
- 签名封套独立于显示清单，至少包含 `publisherId`、`keyId`、算法、规范化内容摘要、包文件清单、索引版本、过期时间和撤销状态；显示名不能充当密码学身份。
- 解包前限制压缩包元数据、文件数和总展开尺寸，逐项拒绝绝对路径、父目录穿越、Windows reparse point 和越界规范化路径；全部摘要与签名通过后才能原子提升为已安装版本。
- 信任根、密钥轮换、撤销、重放防护、降级策略和更新确认需要独立安全评审。
- 安装、更新、卸载和回滚均具备审计记录。

### 阶段 3：沙箱化代码扩展评估

- 如果声明式能力不足，再独立评估 WASM 或独立进程沙箱。
- 权限按能力授予，默认拒绝；执行时间、内存、网络和调用频率都受限。
- 原生动态库和主 WebView 任意 JS 仍不作为默认扩展机制。

## 6. v1 数据模型

### 6.1 插件标识符

`PluginId` 使用小写 ASCII 反向域名格式，例如 `com.easiflux.analytics.overview`：

- 由至少两个点分段组成。
- 每段以字母或数字开头和结尾，中间只允许字母、数字或连字符。
- 总长度最多 128 字节，每段最多 63 字节。
- ID 按字节精确匹配；大写输入直接判定无效，不做静默规范化。

### 6.2 清单

`PluginPublisherId` 使用与 `PluginId` 相同的小写 ASCII 反向域名语法和 128/63 字节限制，但表示发行者而不是具体插件，例如 `com.easiflux`。

Rust 领域模型：

```rust
pub struct PluginManifestV1 {
    pub schema_version: u32,
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub publisher_id: PluginPublisherId,
    pub publisher: String,
    pub contributions: Vec<serde_json::Value>,
    pub requested_capabilities: Vec<String>,
}
```

约束：

- `schemaVersion` 第一版必须严格等于 `1`。
- `publisherId` 是稳定、不可伪装的发行者身份；`publisher` 只是显示名。两者与 `name`、`description` 都有长度上限，去除首尾空白后不能为空。
- `version` 必须是合法的 SemVer。
- `contributions` 描述插件提供什么：它是闭集枚举，每项有稳定的 `contributionId`，只能引用宿主已注册的 `actionId`，参数使用严格 schema。
- `requestedCapabilities` 描述插件申请什么权限；它与 contribution 分离，只能来自宿主枚举，不能由插件自行授予。
- 从磁盘或网络反序列化时使用严格模式，未知字段立即拒绝。
- 显示文案作为普通文本渲染，不能解释为 HTML。

阶段 0 没有贡献执行器，因此 `contributions` 和 `requestedCapabilities` 必须为空；任何非空值都使清单无效。将来支持真实贡献时升级 schema，再引入明确的 contribution 变体和 capability 枚举。宿主根据来源、用户确认和管理策略计算 `grantedCapabilities`；它永远不能由 manifest 指定。

### 6.3 目录条目

前后端传输对象采用 camelCase：

```ts
type PluginSource = 'builtIn'
type PluginStatus = 'enabled' | 'disabled' | 'blocked'

interface PluginCatalogItem {
  manifest: {
    schemaVersion: 1
    id: string
    name: string
    version: string
    description: string
    publisherId: string
    publisher: string
    contributions: []
    requestedCapabilities: []
  }
  source: PluginSource
  grantedCapabilities: []
  status: PluginStatus
  canToggle: boolean
  statusReasonCode: 'stateUnavailable' | 'catalogInvalid' | null
}

interface PluginCatalogSnapshot {
  schemaVersion: 1
  revision: string
  availability: 'available' | 'unavailable'
  availabilityReasonCode: 'stateUnavailable' | 'catalogInvalid' | null
  plugins: PluginCatalogItem[]
}

interface PluginCatalogMutationResult {
  revision: string
  plugin: PluginCatalogItem
}
```

`revision` 仅表示启用状态版本，在 Rust 中为单调递增的 `u64`，通过字符串传给 JavaScript，避免超过 JavaScript 安全整数范围。应用重启保留 revision；从备份恢复时以备份 revision 为准；内置目录变化本身不改变它。状态修改命令必须同时返回新 revision，避免前端保留过期 snapshot 版本。

### 6.4 独立状态文件

插件启用状态不写入通用设置文件，避免插件状态损坏影响设置中心，也便于未来迁移和回滚。

建议路径：应用配置目录下的 `plugins/state.json`。

```json
{
  "schemaVersion": 1,
  "revision": "4",
  "entries": [
    {
      "id": "com.example.plugin",
      "source": "builtIn",
      "publisherId": "com.easiflux",
      "approvalFingerprint": "v1:none",
      "enabled": true
    }
  ]
}
```

状态身份至少绑定 `id + source + publisherId + approvalFingerprint`。阶段 0 的 fingerprint 固定为 `v1:none`，因为贡献与权限均为空；未来 fingerprint 必须覆盖安全相关的贡献和权限集合。来源、发行者或安全相关内容变化时，旧授权不匹配，插件恢复为停用并要求重新确认。全目录 ID 全局唯一，较低信任来源不得 shadow 内置 ID；任何重复 ID 都被拒绝而不是覆盖。

写入提取或复刻通知存储已有的原子语义：同目录临时文件、文件同步、主文件与备份替换、目录同步和失败恢复。只有写盘成功后才更新内存状态。合法但暂时不在目录中的状态可有界保留，以便插件重新出现时恢复用户选择。

状态文件在解析前限制为 256 KiB，最多 512 个条目。每个 ID 和字符串字段执行各自长度校验；revision 必须是十进制 `u64` 字符串。使用数组并在校验阶段拒绝重复复合身份，同时依靠严格结构反序列化拒绝重复对象字段、非布尔值和未知字段。主文件、临时文件、备份依次作为恢复候选；只接受完整通过 schema 与资源限制校验的候选。状态不可恢复时进入 unavailable。阶段 0 的重试会重新读取候选，但绝不自动覆盖损坏证据；受控重建作为后续显式用户操作。

## 7. 后端架构

建议模块：

```text
src-tauri/src/
  commands/plugin.rs          固定 IPC 命令与错误映射
  plugin/
    mod.rs                    模块边界与导出
    manifest.rs               PluginId、manifest、contribution/capability 校验
    builtin.rs                编译时内置目录
    registry.rs               目录合并、状态转换、并发语义
  storage/atomic_file.rs      与业务模型无关的原子文件原语
  storage/plugin_state.rs     独立状态文件与原子持久化
```

阶段 0 删除当前的 `Plugin`、`PluginContext`、`Box<dyn Plugin>` 和 `on_init` 机制。`PluginRegistry` 只管理清单与用户状态；启用不会执行任意 trait 生命周期。未来贡献也只能由固定的宿主执行器解释，不能把不受信任实现注入宿主进程。

`AppState` 继续持有 `Arc<RwLock<PluginRegistry>>`。读取目录使用读锁；修改启用状态使用写锁，并遵循“校验 -> 构造下一状态 -> 写盘 -> 提交内存”的顺序。

固定命令：

```rust
get_plugin_catalog() -> AppResult<PluginCatalogSnapshot>
set_plugin_enabled(id: String, enabled: bool) -> AppResult<PluginCatalogMutationResult>
```

命令永远不接受文件路径、URL、命令名或动态 IPC 名称。插件无法注册新的 Tauri 命令。

Tauri 默认允许已注册的应用命令被所有应用窗口调用，因此阶段 0 还要在 application manifest 中登记这两条命令，并生成最小 allow permission；capability 只绑定本地 `main` WebView，不配置远程 URL或通配窗口。新的窗口和远程内容默认没有这些 IPC 权限，且不得通过另一个 capability 合并获得。

### 7.1 启用规则

1. ID 必须先通过格式校验。
2. ID 必须存在于当前目录。
3. `canToggle` 必须为 true。
4. 阶段 0 的 contribution 和 requested capability 必须为空；后续阶段则必须完成宿主实现、策略授权和用户确认。
5. 持久化成功后才能返回成功。
6. 重复设置相同值为幂等操作，不增加 revision，也不产生无意义写盘。

### 7.2 启动与降级

- 内置清单由 typed Rust constructor 创建，并由单元测试逐项校验；无效或重复清单不能进入运行时目录，也不能通过错误传播阻止核心应用启动。此时目录降级为 unavailable，原因码为 `catalogInvalid`。
- 状态文件不存在等同默认状态。
- 状态文件损坏或读取失败时，核心应用继续启动；插件目录可读，snapshot 顶层 availability 为 unavailable，所有可选插件为 blocked 且启停写操作被阻止，UI 显示已净化的子系统错误。空目录时也能通过顶层 availability 呈现故障。
- 无法解析应用配置目录、创建状态 store 或访问其父目录同样只会让插件 runtime unavailable，不能从 `AppState::new` 传播、`expect` 或 panic。后续显式重试会重新解析路径并尝试恢复 store。
- 错误信息不能泄露配置目录、原始 JSON、签名材料或内部堆栈。

## 8. 前端架构

```text
src/
  types/plugin.ts                         领域类型和严格运行时解析
  services/pluginService.ts               Tauri IPC 适配
  stores/plugin.ts                        请求状态、筛选和按 ID 修改所有权
  components/plugins/
    PluginMarketplacePage.vue             分区编排、标题、搜索与空态
    PluginCard.vue                         单插件展示与启停操作
```

`AppShell.vue` 把当前 `PluginSection` 传入 `PluginMarketplacePage`，不在 Shell 中保存插件业务状态。

### 8.1 三个视图

- 已安装：展示本机已有的内置或未来本地插件，支持搜索、状态筛选和启停。
- 市场：展示当前可信目录。阶段 0 不提供下载按钮；条目明确标记来源和“已随应用提供”。目录为空时说明市场安装将在后续阶段提供。
- 管理：汇总已启用、已停用、被阻止数量，以及每个插件请求和实际授予的能力；不复制第二份状态。

### 8.2 状态管理

- 首次进入插件页时加载一次，用户可显式重试。
- store 为每个插件维护独立 pending 标记，避免一个开关锁住整个页面。
- 同一插件的新请求取得所有权；旧请求晚返回时不能覆盖新状态。
- 操作失败恢复服务端确认状态，并在卡片附近显示可访问的错误。
- service 把 IPC 返回值视为 `unknown` 并严格解析，禁止无校验类型断言。

### 8.3 可访问性

- 页面标题可聚焦，进入分区后聚焦主标题。
- 搜索框有显式 label。
- 开关使用原生按钮/复选语义，提供插件名和当前状态。
- 加载状态使用 `role="status"`，错误使用 `role="alert"`。
- 颜色不是状态的唯一表达方式。

## 9. 错误模型

后端复用项目的 `AppError`/`AppResult`，增加插件领域错误代码或稳定消息映射：

- `plugin_invalid_id`
- `plugin_not_found`
- `plugin_not_toggleable`
- `plugin_manifest_not_activatable`
- `plugin_state_unavailable`
- `plugin_state_persist_failed`

前端展示短、可操作的中文信息，并保留结构化错误供日志记录。目录加载失败与单插件启停失败分开呈现。

## 10. 安全模型

### 10.1 信任层级

1. 内置：随 EasiFlux 二进制发布，继承应用发布信任。
2. 本地声明式：用户显式放入固定目录，不含代码，仍视为不受信任输入。
3. 市场签名包：通过索引与包签名验证后安装，内容仍按最小能力运行。

来源可信不代表拥有无限权限。每个贡献仍必须通过宿主能力检查。

### 10.2 发布者与授权连续性

- 显示用 `publisher` 与稳定 `publisherId` 分离；市场签名再把 `publisherId` 绑定到受信任 key chain。
- 签名覆盖规范化 manifest 和包内每个文件的摘要，而不是下载 URL。
- 状态中的 `approvalFingerprint` 覆盖来源、发布者身份以及安全相关贡献/权限；这些内容扩大时不能静默继承旧启用决定。
- 内置目录是发布构建输入，随应用制品完整性一起保护；若未来从可替换资源文件读取，不能自动视为内置信任。

### 10.3 主要威胁

- 恶意市场、CDN 或发布者密钥泄露。
- 旧版本重放、降级和撤销失效。
- 本地路径穿越、符号链接逃逸和压缩炸弹。
- 重复 ID、发布者冒充和依赖混淆。
- 清单文案触发 XSS 或界面仿冒。
- 插件借宿主实施 confused-deputy 攻击。
- 资源耗尽、无限重试或高频 IPC。

阶段 0 通过“无第三方代码执行、无下载、严格清单、固定命令、独立状态文件、默认拒绝能力”消除或推迟大部分高风险面。

## 11. 测试策略

### 11.1 Rust 单元与集成测试

- PluginId 合法、大小写、边界长度和非法字符。
- manifest schema、SemVer、空白文案、阶段 0 非空贡献/能力和重复 ID。
- 目录排序稳定、默认停用、未知持久化 ID 保留。
- 启停成功、幂等、未知 ID、不可切换、阶段 0 清单不可激活。
- 写盘失败时内存不提交。
- 状态文件缺失、损坏、版本未知和恢复路径。
- 并发读取与串行修改不产生丢失更新。
- IPC DTO 使用 camelCase，revision 使用字符串。

### 11.2 前端测试

- service 对正确与畸形 IPC 响应的解析。
- store 的加载、重试、筛选、按 ID pending 和旧请求隔离。
- 三个分区、空态、错误态、请求/授予能力摘要和来源标签。
- 开关成功、失败恢复和键盘/可访问语义。
- `AppShell` 不再渲染旧占位页，并正确转发 section。

### 11.3 回归验证

- `pnpm test`
- `pnpm typecheck`
- `pnpm build`
- `cargo fmt --check`
- `cargo test --locked`
- `cargo clippy --locked --all-targets -- -D warnings`（若命中已确认的历史 warning，需区分基线债务与本次新增）

## 12. 分支与交付策略

1. `plugin/system-design`：本设计与可执行实施计划。
2. `plugin/runtime-foundation`：阶段 0 的 Rust 目录、状态持久化、IPC、前端 store 与 UI 垂直切片。
3. 后续建议：
   - `plugin/local-manifests`
   - `plugin/signed-marketplace`
   - `plugin/sandbox-runtime`

每个后续分支都从已经验证的前一阶段切出，不并行引入互相依赖的状态格式或安全模型。

## 13. 兼容性与迁移

- 所有落盘结构都有 `schemaVersion`，未知大版本默认拒绝。
- 只在升级代码能够完整验证并原子提交时迁移。
- 清单 API 允许增加可选、版本化字段；v1 严格解析意味着新增字段必须通过新 schema 版本引入。
- 删除插件时保留少量启用状态不会影响运行；未来提供显式清理策略。
- 阶段 0 目录为空是合法生产状态，UI 和后端不得把它当错误。

## 14. 参考资料

- Tauri 官方插件文档：https://v2.tauri.app/develop/plugins/
- Tauri 官方 capability 文档：https://v2.tauri.app/security/capabilities/
- Tauri 官方 permission 文档：https://v2.tauri.app/security/permissions/
- Serde 容器属性与 `deny_unknown_fields`：https://serde.rs/container-attrs.html
