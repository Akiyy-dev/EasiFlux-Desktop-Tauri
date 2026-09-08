# EasiFlux 受管本地插件包移除设计

**日期：** 2026-09-09

**所属 PRD：** PRD-11 插件系统 UI（Plugin Marketplace）

**阶段：** Phase 1C，受管所有权与本地声明式包移除

**基线：** remote PR head `plugin/local-manifest-import@4a316ac`；当前 worktree 的 tree-equivalent parent 为 `42dcfea`（包含同一 CI 最小权限修复）

**设计分支：** `plugin/local-manifest-removal`

**状态：** 已批准（用户授权自主继续）；本文只定义架构与验收边界，不在本提交编写生产代码。

## 1. 背景与目标

Phase 1A 能从固定目录发现只含元数据的本地声明式包；Phase 1B 能通过原生文件选择器预览并导入严格清单。导入后的包默认停用，不执行代码、贡献或资源。

Phase 1B 的 `pkg-<32 hex>` 只是随机目标名。它没有持久记录“这个包确实由 EasiFlux 完整构造并登记”，也没有绑定当前目录和文件的操作系统对象身份。因此槽位名、manifest fingerprint、disabled 状态或合法包形状都不能单独授权删除。手工放置的包必须始终由用户自己管理，不能因为看起来像导入包而出现移除能力。

本阶段目标是：

1. 为新导入包写入严格的包内 ownership receipt，并建立独立、严格、有界、原子持久化的 managed ownership index。
2. 只在包内 receipt、managed index、当前槽位、清单安全身份和文件系统对象身份全部精确匹配时，把包标记为 managed。
3. 只允许用户移除当前权威目录中已经停用的 managed 本地包。
4. 移除前失败关闭地保存 disabled 决定，再把包原子隔离出发现根，最后权威重扫与准确清理。
5. 明确处理崩溃、传输未知、索引损坏、内容替换和清理失败，绝不伪称跨文件原子性。
6. 所有权子系统失败不影响核心应用与插件只读发现；只关闭新的受管导入和移除。

本阶段仍不会让插件执行任何行为。“移除”只删除 EasiFlux 能证明由自己管理的本地元数据包，不等于卸载可执行插件。

## 2. 核心安全裁决

### 2.1 三方证明，而非槽位推断

以下信息均不足以授权删除：

- 名称匹配 `^pkg-[0-9a-f]{32}$`；
- manifest ID、publisherId 或 approval fingerprint 与目录快照相同；
- plugin-state 中存在对应 disabled 决定；
- 目录当前只有一个 `manifest.json`；
- 槽位看起来由 UUID v4 生成；
- 包内存在一个孤立 receipt；
- managed index 中存在一个孤立条目。

managed 判定必须同时满足：

1. 包内 `ownership-receipt.json` 严格有效；
2. managed index 中存在精确匹配的 entry；
3. 两者绑定同一 receiptId、目标槽位和插件安全身份；
4. index 保存的目录、manifest、receipt 三个 OS 对象身份都与当前句柄一致；
5. 当前 manifest 重新严格解析后仍产生相同 approval fingerprint。

任何一项不满足，当前包就是 external、ownershipConflict 或 ownershipUnavailable，不能移除。这里的防误删保证以单个 EasiFlux 写入者和生命周期调用期间没有人工修改插件目录为边界；第 13 节进一步说明 Unix/macOS 名称式 rename/unlink 只能在可观察检查点 best-effort 发现偏差，不能从内核层阻止或保证检测同一 OS 用户在最后窗口交换对象。

### 2.2 历史包迁移

升级时 ownership index 不存在等同安全空索引，绝不等同“认领所有 `pkg-*`”。因此：

- PR #29 或更早版本导入、但没有包内 receipt/index entry 的包归类为 `external`；
- 手工放置的合法单 manifest 包同样归类为 `external`；
- 只有包内 receipt 而没有 index entry，或只有 index entry 而包内 receipt 不匹配，也不是 managed；
- 不根据随机槽位、mtime、状态记录或 fingerprint 猜测来源；
- 不自动补领 receipt，不提供本阶段 adoption；
- external 包继续可发现、可启停，但应用不显示移除能力。

无法自动移除历史导入包是有意的迁移代价，优于误删用户手工内容。

### 2.3 导入登记不是跨文件原子事务

新导入按以下边界建立管理关系：

1. 宿主在完整 import stage 内同时写 canonical `manifest.json` 与不可变 `ownership-receipt.json`；
2. 保存精确 disabled 决定；
3. 将完整双文件包 exclusive promotion 到 receipt 绑定的目标槽位；
4. 从目标重新打开并验证三个对象身份；
5. 最后原子登记 managed index。

只有第 5 步成功才是 managed。若包提升成功但 index 登记失败、进程退出或结果未知，包保留在 `local`，但安全降级为 external/non-removable。不得为了制造“全回滚”而删除已经提升、但尚未登记的包，也不得在重启时仅凭包内 receipt 自动补登记。

这不是状态文件、包目录和 index 的伪原子提交；它是明确的单向提交与失败降级。

### 2.4 移除提交点

移除的提交点是把精确 managed 包从 `local/pkg-*` exclusive rename 到 `removal-staging/remove-*`。提交点之前失败，包仍在目录；之后即使 index 清理、物理清理、权威重扫或响应失败，也必须报告 committed-but-unconfirmed/cleanup-pending，而不能说“未移除”并自动重试整个操作。

## 3. 方案比较

| 方案 | 收益 | 风险 | 决策 |
| --- | --- | --- | --- |
| 仅凭槽位 + generation 删除 | 改动少 | 无法区分历史导入和手工包，也不证明当前对象 | 拒绝 |
| 仅使用包内 marker | 包自描述 | marker 可随包复制；没有宿主登记和对象绑定 | 拒绝 |
| 仅使用外部 index | 不改变包形状 | stale/伪造条目可能误认重建或手工包 | 拒绝 |
| 包内 receipt + 原子 index + 当前对象身份 | 所有权可证明、可失效、可安全降级 | 需要导入协议升级、存储和崩溃矩阵 | 采用 |
| 直接做签名市场 | 长期生命周期完整 | 尚无服务端、信任根、轮换、撤销和审计策略 | 后续独立设计 |

## 4. 非目标

- 在受支持的单写者、无人工并发修改边界内，不删除、移动或认领没有完整所有权证明的 external 包；若同一 OS 用户越过该边界在 Unix/macOS 最后窗口交换名称，防误删保证失效。实现只在可观察点 best-effort 复验，检测到偏差就保留/降级，但不承诺发现所有交换，也不伪称 replacement 从未被移动或 unlink。
- 不提供历史包 adoption、批量删除或“一键清空插件目录”。
- 不更新、覆盖、降级、回滚或替换包。
- 不清除 orphan plugin-state；移除后保留精确 disabled 身份。
- 不执行贡献、脚本、HTML、JavaScript、WASM、动态库或本地程序。
- 不增加下载、签名、发布者认证、信任根、远程索引或能力授权。
- 不允许前端提交路径、槽位、receiptId、对象 ID、fingerprint 或原始 JSON。
- 不递归清扫 import/removal staging，不删除未知或身份不匹配对象。
- 不在 reload、启动或前端结果未知时自动重试 destructive rename/cleanup。
- 不实现跨进程锁；多个 EasiFlux 写入者或生命周期调用期间人工改动 `plugins` 树均不受支持。应用内所有写者仍由 FIFO gate 串行。
- 不抵御已经控制宿主进程或当前 OS 用户账户并持续篡改配置目录的攻击者。
- 不承诺跨 plugin-state、包目录与 ownership index 的单次原子耐久提交。

## 5. 目录、包形状与预算

```text
<platform config_dir>/EasiFlux Desktop/plugins/
├── state.json
├── state.json.tmp / state.json.bak
├── state.json.pending / state.json.bak.pending
├── managed-ownership.json
├── managed-ownership.json.tmp / managed-ownership.json.bak
├── managed-ownership.json.pending / managed-ownership.json.bak.pending
├── local/
│   └── pkg-<32 lowercase hex>/
│       ├── manifest.json
│       └── ownership-receipt.json       # 仅新受管导入包
├── import-staging/
│   └── stage-<32 lowercase hex>/
│       ├── manifest.json
│       └── ownership-receipt.json
└── removal-staging/
    └── remove-<32 lowercase hex>/
        ├── manifest.json
        └── ownership-receipt.json
```

legacy/manual external 包继续保持 Phase 1A 的唯一磁盘形状：只有 `manifest.json`。managed candidate 必须恰好有上述两个固定文件；双文件 candidate 在没有成功登记 entry 时，其 transport management 分类仍是 external。任何其他额外项、大小写变体或子目录都拒绝。

`local`、`import-staging` 和 `removal-staging` 必须从同一个受验证 `plugins` 父句柄打开或创建并位于同一 volume；不得跨卷复制降级。

| 资源 | 上限与策略 |
| --- | --- |
| ownership index | 256 KiB；读取 262,145 字节探测超限 |
| index entries | 160，包括 managed 与 removal-pending |
| 包内 receipt | 4 KiB；读取 4,097 字节探测超限 |
| local 根直接项 | 继续为 256；结构合格包继续最多 128 |
| import staging 直接项 | 16，包含未知与崩溃遗留项 |
| removal staging 直接项 | 16，包含未知与崩溃遗留项 |
| 单 manifest | 继续为 16 KiB |
| local discovery 单次总读取 | 继续为 2 MiB；local manifest 与 managed-candidate receipt 的实际读取字节都计入 |
| removal reconciliation 单次总读取 | 独立 328 KiB；最多 16 个 direct item，每项 manifest 的 16,385 字节 probe 与 receipt 的 4,097 字节 probe 都计入 |
| promotion/removal 名称尝试 | import promotion 最多 4 个完整 stage/receipt；removal 每个请求只尝试 1 个已登记目标 |
| receiptId | UUID v4 simple，32 个小写十六进制字符 |
| 对象身份 | volume 固定 16 个、object 固定 32 个小写十六进制字符 |

达到任一预算时拒绝当前生命周期写操作，不覆盖旧 index，不改变其他插件。

## 6. 包内 Ownership Receipt

receipt 由宿主在 import stage 中创建，不来自源清单或前端：

```json
{
  "schemaVersion": 1,
  "receiptId": "91a76dcf6dfb4d44a61b34b876ae486d",
  "packageSlot": "pkg-b99c92da6ef54d1f942c1ec706892a99",
  "source": "localDeclarative",
  "pluginId": "com.example.notes",
  "publisherId": "com.example",
  "approvalFingerprint": "v1:sha256:<64 lowercase hex>"
}
```

规则：

- exact object、`deny_unknown_fields`、重复字段拒绝；
- schema 1 只接受 `localDeclarative`；
- receiptId 与 packageSlot 必须规范且不从 manifest 派生；
- pluginId、publisherId 与 fingerprint 复用现有严格规则；
- canonical bytes 固定字段顺序，最终文件最多 4 KiB；
- receipt 不包含路径、用户文案、权限、密钥或 OS 对象 ID；
- receipt 不是签名或 MAC；它依靠宿主原子登记的 index 与实时对象身份配对，威胁模型仍排除已控制当前 OS 账户的攻击者；
- receipt 一旦 stage 完整就不可修改。目标槽位发生碰撞时，当前 stage/receipt 安全清理后，用新 receiptId、stage 和 packageSlot 重新开始，最多四次；禁止就地改 receipt 后继续。

只有 promotion 明确未发生、原 stage 的目录与两个文件身份仍匹配、且非递归清理完整成功时才允许碰撞重试；否则保留证据并结束本次导入。

receipt 只是所有权证明的一半。复制 receipt 或完整包不会复制 managed index 和当前对象身份，因此不会获得移除能力。

## 7. Managed Ownership Index

### 7.1 文件封套

```json
{
  "schemaVersion": 1,
  "revision": "7",
  "entries": []
}
```

`revision` 是 index 内部 checked-increment 的 `u64` 十进制字符串，不进入 IPC，也不替代 catalog generation。无实际变化不增加；恢复后的等价规范重写可保留 revision。

文件严格拒绝未知/重复/缺失字段、非规范 revision、非法字符串和重复 identity。entries 按 receiptId 规范排序，并拒绝：

- 重复 receiptId；
- 重复 packageSlot 或 removalSlot；
- 同一 `(pluginId, localDeclarative)` 的多个活动 entry；
- 相同对象身份被多个 entry 引用；
- managed/removing 分支不符合各自 exact shape。

### 7.2 对象身份

现有平台层已使用 `{ volume: u64, object: u128 }`：Unix 对应 `st_dev + st_ino`，Windows 对应 `FILE_ID_INFO.VolumeSerialNumber + 128-bit FileId`。index 以固定宽度小写十六进制存储目录、manifest 和 receipt 三个对象：

```json
{
  "directoryIdentity": {
    "volume": "000000000000002a",
    "object": "00000000000000000000000000000123"
  },
  "manifestIdentity": {
    "volume": "000000000000002a",
    "object": "00000000000000000000000000000456"
  },
  "receiptIdentity": {
    "volume": "000000000000002a",
    "object": "00000000000000000000000000000789"
  }
}
```

对象身份不进入 IPC、日志或用户错误。复制、重建、跨 volume 移动或无法取得稳定身份都会使管理关系失效。

### 7.3 Entry 状态机

index 只记录已经提升并验证的包，不记录 import stage：

```text
managed
  receiptId, lifecycle, packageSlot,
  pluginId, source, publisherId, approvalFingerprint,
  receiptSha256,
  directoryIdentity, manifestIdentity, receiptIdentity

removing
  managed 的全部字段 + removalSlot
```

`receiptSha256` 是规范 receipt bytes 的 SHA-256 小写十六进制，用来绑定 index 与包内 receipt；它不替代逐字段比较。

转换只有：

```text
verified promoted package -> managed -> removing -> entry deleted
                                    \-> managed  # rename 明确未发生时回退
```

每个 remove 请求只有一个 removalSlot；exclusive rename 的目标碰撞若能证明 rename 未发生，就先回退 managed 并结束该请求，不把 removing entry 改到第二个槽位，也不在同一请求重试。每次转换先 clone index、严格校验预算与唯一性、原子持久化，再提交内存。

### 7.4 原子存储与恢复

index 使用独立 `ManagedOwnershipStore`，只复用现有 `AtomicFile` 的“clone -> persist -> commit”逻辑，不调用其路径式 `replace`。store 必须建立安全原子文档适配器，并把 main、tmp、bak、pending、bak.pending 全部纳入同一协议：每个名字都是受验证 `plugins` 父句柄下的固定单组件；创建必须 exclusive；已有对象只能通过父句柄 no-follow/reparse 打开，验证为单链接普通文件、有界且大小写精确；截断、写入、sync 与身份复验都作用于已验证句柄，禁止验证后再按路径重开。任何预存 link/reparse/hardlink、对象交换、未知工作文件形状或工作槽清理失败都使 ownership unavailable，绝不 truncate、覆盖或跟随它。

`pending` 与 `bak.pending` 只属于一次 save 的写侧工作对象，永不作为 authority recovery candidate；main/tmp/bak 才按既有候选优先级读取。安全适配器必须定义每个 rename/replace、旧工作对象准确清理和崩溃残留的单调状态，禁止普通绝对路径 `OpenOptions(create, truncate)`、先 unlink 再覆盖或递归清理。Unix rename 依赖第 13 节的单写者边界；Windows 工作文件 promotion/replace 必须绑定已持有 source/parent handle。移除的 disabled 前置屏障也必须让 `PluginStateStore` 通过这套适配器覆盖 `state.json` 的同五类名字；不能因为 state store 早已存在就把不安全 working-file open 带到 destructive 路径。

一次 save 的固定协议是：先安全读取 authority candidates；把上次崩溃留下的 bounded、单链接普通 pending/bak.pending 作为非 authority 工作残留按句柄身份准确清理并 sync parent，任何不安全形状则失败关闭；随后 exclusive 创建 pending、写满并 flush；若有 previous bytes，再 exclusive 创建 bak.pending、写满/flush 后以对象绑定 source handle 原子替换 bak；备份确认后，准确删除旧 tmp recovery candidate 并 sync parent，防止它在新 main 损坏时越过刚保存的 previous；最后以 pending 的对象绑定 handle 原子替换 main，禁止先 unlink main。Linux/macOS 在单写者边界内使用 parent-relative rename 并 sync parent；Windows 使用 `SetFileInformationByHandle(FileRenameInfo)`，source 是已持有工作文件 handle、RootDirectory 是已持有父目录、ReplaceIfExists=true，且被替换的 main/bak 必须先验证为安全 reserved authority object。每次替换后从父句柄重开目标并证明 identity 等于原 source；tmp/work 残留 cleanup 同样遵守第 13 节的平台删除边界。

main replacement 是文档提交点。store 返回显式 `PersistOutcome::{NotCommitted, CommittedProcessCrashSafe, CommittedDurable}`，调用者据此决定内存是否采用新文档，不能把 post-commit sync 失败伪装成旧文档仍在。Linux/macOS 只有父目录 sync 成功才是 CommittedDurable；Windows flush 文件并完成 handle-relative replace 后最多是 CommittedProcessCrashSafe。`save_before_destructive_rename` 只在 Linux/macOS 得到 CommittedDurable、Windows 得到 CommittedProcessCrashSafe 时允许继续；任何 NotCommitted 或提交后屏障失败都停止 package rename，已提交的新文档仍进入内存/下一快照，并按实际阶段返回结构化失败。

普通 catalog/state 写可以保留现有“rename 成功即提交、父目录 sync 失败只记录”的语义；但移除步骤 7 的 disabled save 与步骤 8 的 removing save 必须调用单独的 `save_before_destructive_rename` 平台屏障。Linux/macOS 上文件 sync、原子替换和父目录 sync 任一步未确认都返回失败，绝不执行 package rename。Windows 必须 flush 新主文件句柄并使用安全 handle-relative replace；Microsoft 没有给普通应用一个与 Unix 目录 fsync 等价的目录项断电承诺，因此这里只承诺进程崩溃一致性，不承诺突然掉电后两个独立文档与 package rename 的共同耐久性。掉电后若 removing entry 丢失而 quarantine 保留，bounded removal-staging 扫描把它计为冲突/待维护并永久保留，不从 receipt 自动重建授权，也不自动删除。

读取主、tmp、bak 候选时：

- 主文件未来 schema 或超限是终止性 unavailable，不回退；
- 当前 schema 截断/损坏可读取后续合法候选；
- 从非主候选恢复时标记 requiresRewrite；在实时三方 reconciliation 和安全重写成功前，任何 entry 都不能授权移除；
- 没有任何候选等同空 index available；
- 候选耗尽、配置根不安全或重写失败时 ownership unavailable；catalog 仍可只读发现，只有 managed import/remove 暂停，set_enabled 与 plugin-state retry 继续按原规则工作。

不自动从包内 receipt 重建缺失 index。receipt 没有配对 entry 时始终 external。

## 8. 安全发现与 Reconciliation

### 8.1 内部 locator

扫描器在目录、manifest 和可选 receipt 句柄仍打开且第二次 shape check 完成时返回：

```rust
struct LocalPackageLocator {
    slot: LocalPackageSlot,
    directory_identity: FileIdentity,
    manifest_identity: FileIdentity,
    receipt: Option<VerifiedPackageReceipt>,
}

struct VerifiedPackageReceipt {
    model: OwnershipReceiptV1,
    canonical_sha256: [u8; 32],
    file_identity: FileIdentity,
}

struct DiscoveredLocalPlugin {
    record: PluginRecord,
    locator: LocalPackageLocator,
}
```

locator 不可从任意字符串构造，不公开路径，不进入 transport。`PluginRecord` 继续只表达内容安全身份，不能承载文件所有权。

同一 manifest 被移到新槽位、原位重建、receipt 增删或对象身份变化时，即使 UI 文案相同，catalog generation 也必须递增，让旧确认失效。generation 是完整权威 registry snapshot 的结构/ownership 版本，不只是 discovery 版本：locals locator/identity、任一 item 的 management、ownership eligibility basis/toggleBlockReasonCode，以及顶层 managedOwnership status/count 的任何已发布变化都必须 checked-increment generation。内部 index revision 不进入 IPC，也不能代替这条规则。

registry 先构造完整候选 snapshot，与当前已发布 snapshot 做语义比较，再一次性交换；ownership/index 的中间状态不得单独发布。若候选有上述变化而 generation 已为 `u64::MAX`，发布失败并保留旧 snapshot，生命周期写操作必须在访问 storage 前预检 generation headroom。同一 `(revision, catalogGeneration)` 的任意两个响应必须在所有 snapshot/item/summary 字段上语义完全相等；plugin-state 状态变化至少推进 revision，ownership/locator 变化推进 generation。

`canRemove` 是 management/ownership、status 与 plugin-state availability 的投影。只有其 management/ownership eligibility basis 改变时推进 generation；同一 managed 项单纯 enabled/disabled 导致的 `canRemove` 变化由同一次 plugin-state revision 覆盖，catalog generation 保持不变。这是唯一的派生字段例外，保证现有单-item toggle mutation 仍可安全仲裁；dialog 还必须因目标 status/revision 变化失效，后端也在 storage 前重验 disabled。

### 8.2 Managed 判定

初次发现、显式 reload、导入提交和移除提交都在 operation gate 内进行 reconciliation：

1. 安全扫描 local 中的 external/receipt 双形状包并生成 locator。
2. 以同样的固定根、独立 328 KiB aggregate、no-follow 规则只读扫描 removal-staging 直接项；未知/额外形状只计数并保留。该预算耗尽只让 managed ownership unavailable，不消耗或改变 local discovery 的 2 MiB 预算、status 或已发现内容。
3. 加载或重试 ownership index。
4. 对每个 local/removal 项查找唯一 entry。
5. 比较 receiptId、packageSlot/removalSlot、source、pluginId、publisherId、approvalFingerprint、receiptSha256 和三个 OS 对象身份。
6. 再从 manifest 严格模型计算 approval fingerprint，不信任 receipt 自报值。
7. 全部相等且 entry lifecycle=managed 时才标记 managed。

分类：

- 无 receipt 的合法单 manifest 包无需读取 index，始终是 external，并继续按 plugin-state 可用性规则启停；
- receipt 存在但无 entry：external，包括 index 登记失败的新包；
- entry 存在但包/receipt/对象不匹配：ownershipConflict，不删除；
- index unavailable 时，只有带有效 receipt、因无法查 index 而不能分类的 candidate 是 ownershipUnavailable；legacy/manual 单文件 external 不变；
- removing entry 的 source 精确存在且 target 不存在：`removalPending`，顶层 summary 报 rollback pending；
- removing entry 的 source 消失时，以下单调 cleanup 形状都报 cleanup pending 且绝不在 reload 自动继续删除：完整 target（目录、manifest、receipt 三身份匹配）、receipt-only target（目录与 receipt 身份匹配、manifest 缺失）、empty target directory（目录身份匹配、两个文件均缺失），以及 source/target 两侧均缺失但 index 删除失败；
- source/target 同时存在、目标身份不匹配、无 entry 的 removal-staging 项、未知形状或其他歧义全部报 conflict，既不回退也不 cleanup。

单条冲突不会让核心 plugin catalog unavailable，但会增加有界 ownership 警告并阻止相应生命周期写操作。

### 8.3 Catalog transport v3

ownership 会改变用户可执行操作，因此显式升级 transport：

```ts
type PluginManagement =
  | 'builtIn'
  | 'managed'
  | 'external'
  | 'removalPending'
  | 'ownershipConflict'
  | 'ownershipUnavailable'

type PluginToggleBlockReason = 'removalPending'

interface ManagedOwnershipSummary {
  status: 'available' | 'degraded' | 'unavailable'
  conflictingEntryCount: number
  rollbackPendingCount: number
  cleanupPendingCount: number
}

interface PluginCatalogItemV3 {
  // 既有 manifest/source/status/statusReasonCode/canToggle/grantedCapabilities
  management: PluginManagement
  canRemove: boolean
  toggleBlockReasonCode: PluginToggleBlockReason | null
}

interface PluginCatalogSnapshotV3 {
  schemaVersion: 3
  // 既有 revision/catalogGeneration/availability/localDiscovery/plugins
  managedOwnership: ManagedOwnershipSummary
}

interface PluginCatalogMutationResultV3 {
  schemaVersion: 3
  revision: string
  catalogGeneration: string
  plugin: PluginCatalogItemV3
}
```

mutation v3 仍是单-item 协议：set_enabled 必须在 gate 内先确认 expected/current catalogGeneration 相等且 ownership/locator 未变化，返回值也必须保持同一 generation；它只推进 revision，并允许 status/canToggle 及“仅因 status 改变”而变化的 canRemove。前端要求返回、请求与当前 generation 三者相等，并验证 manifest/source/management/toggleBlockReason 等结构/ownership 字段不变。若操作观察到需要推进 generation 的 reconciliation，必须在 state write 前以 stale/invalid 拒绝并要求完整 reload，不能塞进 mutation response。

不变量：

- builtIn source 只能是 `management=builtIn`，不可移除；
- managed 只能属于 localDeclarative 且三方证明成立；
- 只有 managed + disabled + plugin-state available 才 `canRemove=true`；
- readable removing entry 的 source 尚在、target 不存在时使用 `removalPending`，保持 disabled、`toggleBlockReasonCode=removalPending`、禁止 toggle/remove，直到无删除副作用的 index 回退成功；
- enabled、blocked、external、removalPending、ownershipConflict、ownershipUnavailable 全部不可移除；
- blocked 当且仅当 `statusReasonCode` 非空；除此之外 `statusReasonCode=null`。`canToggle` 当且仅当 status 不是 blocked 且 `toggleBlockReasonCode=null`；toggleBlockReasonCode 当且仅当 management=removalPending；
- managed/external/ownershipConflict/ownershipUnavailable 的 enabled 或 disabled 项在 plugin-state available 时都可 toggle；ownership 故障只关闭 managed import/remove，不关闭 set_enabled 或 state retry。builtIn 沿用相同 plugin-state 规则；
- ownership unavailable 不改变顶层 plugin-state availability，也不把无 receipt 的 external 项改成 ownershipUnavailable；
- receipt、槽位、fingerprint 和对象 ID 永不进入 DTO；
- `rollbackPendingCount` 只统计 source 精确存在、target 不存在、entry=removing 且 index 回退未完成的项；`cleanupPendingCount` 只统计 full/receipt-only/empty target 中每个存活对象都与 removing entry 匹配的单调子集，或两侧已空但 entry 清理失败的已确认提交状态；manifest-only、额外对象、无法确认提交点、无 entry quarantine、身份不匹配或其他歧义进入 conflictingEntryCount，三个计数对同一 entry/object 互斥；
- available 时三个 summary count 都为 0；degraded 时至少一个 count 非零；unavailable 时三个 count 都为 0。所有 count checked 转为安全整数并受 index/staging 上限约束。

## 9. 新导入的所有权登记

Phase 1B 的原生选择、token、TTL 和内容捕获保持不变；commit 改为以下顺序。

### 9.1 事务顺序

1. 取得 token 所有权与 FIFO operation gate。
2. pre-scan 要求 plugin-state、本地发现和 ownership store 足以安全写入；校验 generation、ID 冲突及预算。
3. 生成 receiptId 与一个精确 packageSlot。目标碰撞时整次 stage/receipt 重建，最多四次。
4. 在 import staging 中 exclusive 创建 stage，写 canonical manifest 与 canonical ownership receipt；分别 sync 两文件，再 sync stage 和 staging 父目录。
5. 捕获 stage directory、manifest 与 receipt 的对象身份并再次验证 exact two-file shape 和内容。
6. 在 registry 状态事务中清理同 `(id, localDeclarative)` 的历史 fingerprint，持久化当前精确身份为 disabled。失败时不提升。
7. 将完整 stage 同卷 exclusive rename 到 receipt 中唯一 packageSlot，不覆盖、不复制降级。
8. sync staging/local 父目录；从 local 父句柄重新打开目标，要求三个对象身份保持不变，并重验两文件与语义身份。
9. 构造 managed index entry，并用一次原子 index save 登记。只有该步骤成功才建立 managed 关系。
10. 权威重扫、reconcile、按第 8.1 节比较完整 ownership-derived DTO 并推进 generation 后发布。只有快照出现 exact localDeclarative + managed + disabled 才返回完整成功。

### 9.2 登记失败

包提升是文件系统提交点。其后 index save、index revision、worker、重验、publication 或响应失败时：

- 不回滚删除已提升包；
- 不自动再次登记 index；
- 没有完整 entry 的包按 external/non-removable 展示；
- disabled 决定保持；
- 用户 reload 只观察权威状态，不重放 import token。

如果目标重验发现对象或内容变化，绝不登记 index。即使包内 receipt 看起来正确，也降级 external/冲突。

### 9.3 Import 结果 schema 2

```ts
type CommitLocalManifestImportResultV2 =
  | {
      schemaVersion: 2
      status: 'imported'
      pluginId: string
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 2
      status: 'importedExternal'
      pluginId: string
      reasonCode: 'plugin_import_ownership_not_registered'
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 2
      status: 'importedNotVisible'
      pluginId: string
      reasonCode: 'plugin_import_publication_unconfirmed'
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 2
      status: 'notImported'
      disabledDecisionSaved: boolean
      reasonCode: LocalManifestImportCommitFailureV2
      snapshot: PluginCatalogSnapshotV3
}
```

`LocalManifestImportCommitFailureV2` 保留 Phase 1B 的 pre-promotion 闭集，并只增加可在提升前确定的 `plugin_ownership_unavailable`、`plugin_ownership_capacity_exceeded` 与 `plugin_ownership_revision_exhausted`。实际 index save 的 `plugin_ownership_persist_failed` 发生在 promotion 后，只能收敛为 `importedExternal`；`plugin_import_ownership_not_registered` 不能伪装成 `notImported`。

- `imported`：包提升、managed index 与 disabled catalog 均确认；
- `importedExternal`：包已提升但 managed index 未建立，当前包保留为 external/non-removable；UI 明确说明不要重新导入同一 ID，后续只能手工管理或等待独立 adoption 能力；
- `importedNotVisible`：包或登记已跨提交点但最终 catalog publication 未确认；
- `notImported / disabledDecisionSaved=false|true`：包尚未提升；true 时不能恢复旧授权；
- transport/parser unknown：只 reload，不自动重试。

import preflight 在 ownership unavailable 时返回 `plugin_ownership_unavailable`，不得悄悄绕过 index 后声称 managed 成功。

## 10. 受管包移除

### 10.1 UI 前置条件

用户只能对 `management=managed` 项发起移除，并且必须先明确停用。本阶段不提供“停用并移除”组合操作。

确认面板展示名称、ID、版本、publisher 与来源，并明确：

- 删除 EasiFlux 管理目录中的副本，不影响最初选择的源文件；
- 操作不可撤销；
- disabled 偏好保留，重新导入不会自动启用；
- external 包必须退出应用后手工管理，应用不会删除。

### 10.2 固定 IPC

```rust
remove_managed_local_plugin(
    id: String,
    expected_catalog_generation: String,
) -> AppResult<RemoveManagedLocalPluginResult>
```

前端不传 receiptId、locator、路径、槽位、publisher、fingerprint、对象身份或原始 manifest。

### 10.3 后端顺序

1. 入口同步保留 FIFO operation reservation；owned task 在调用方消失后继续。
2. gate 内完成必要 discovery/reconciliation，再严格比较 canonical expected generation，并在任何状态、index 或 package storage 前预检 catalog generation 仍有一次 publication headroom、plugin-state revision 仍有一次 disabled rewrite headroom、ownership revision 仍有两次转换 headroom（removing 后只能二选一地 rollback 或 delete）；任一耗尽都结构化拒绝。
3. 要求项目是唯一 localDeclarative、status=disabled、management=managed，plugin-state 与 ownership 均可用。
4. 从 registry 取得 record、opaque locator 和唯一 managed entry，先在内存比较所有字段。
5. storage 通过固定父句柄重开 package，重验 exact two-file shape、receipt、manifest fingerprint 和三个对象身份。
6. 检查 removal staging 上限并生成唯一 `remove-*` 槽位；每个请求只选一个并写入 index，绝不覆盖或在碰撞后换槽重试。
7. 通过 `save_before_destructive_rename` 再次持久化当前身份为 disabled，清除同 ID/source 的所有历史 enabled fingerprint。即使表面已 disabled，也不能省略；平台耐久屏障未满足时不继续。
8. 通过同一耐久屏障原子把 index entry 转换为 `removing` 并记录唯一 removalSlot。该 save 未确认时包不移动。
9. 在 rename 紧邻前再次通过固定父句柄验证 source exact shape、receipt、manifest、三个对象身份与目标缺失。任何变化都证明 rename 尚未发生，先尝试把 index 回退 managed 后结束；不得访问第二个 removalSlot。
10. 把精确 package 从 local exclusive rename 到 removal staging。rename 是移除提交点。明确 EEXIST 且 source 三身份仍匹配、target 是另一对象时，证明未提交，回退 managed 并结束；其他不确定结果一律保守进入 committed-unconfirmed，不能换槽或自动重发。
11. sync 两侧父目录并重开 quarantine，要求 receipt、内容和三个对象身份继续精确匹配。
12. 权威重扫并构造尚未公开的候选结果；目标 managed identity 必须不再出现。并发出现的 external 同 ID 只按现有冲突规则处理，不能算作被移除目标。此时不发布中间快照。
13. 当前 owned task 只尝试一次非递归 cleanup：重验两个文件，先 unlink manifest、最后 unlink receipt，再确认空目录后 rmdir，sync removal staging。若中途失败，允许的剩余形状只能是同一目录身份下、由 index 记录的文件子集；任何未知项都停止。
14. cleanup 成功后原子删除 index entry；失败时保留可收敛的 removing entry。最后再按实际磁盘/index 结果完成 reconciliation，与旧完整 snapshot 比较并恰好发布一次；removed、cleanup-pending、rollback-pending 或 conflict 的最终变化共同消耗步骤 2 预留的一次 generation，绝不先暴露半清理快照。

权威重扫失败不能把控制流改写成 notRemoved；同一个已接受 owned task 仍可完成至多一次准确 cleanup 尝试，但最终响应优先使用 `removedCatalogUnconfirmed`。之后的 reload 不重试 unlink。

### 10.4 不自动重试

- rename 前明确失败（包括唯一 removalSlot 碰撞）：当前调用先尝试原子恢复 managed entry；恢复失败时项目以 `removalPending`/ownership degraded 发布。后续 reconciliation 可在 source 仍精确存在且 target 不存在时安全回退 managed，但不会自动再执行 rename，也不会替换 removalSlot 后重试。
- rename 后失败：绝不自动重新调用移除或重复 rename。quarantine/entry 保留并展示 cleanup pending。
- reload/启动只观察并分类对象位置；可以在“两侧都不存在”时删除 stale index entry，或在“source 精确存在、target 不存在”时回退 managed，因为两者都不删除文件。
- reload/启动不得 unlink quarantine。后续显式清理能力或退出应用后的人工维护另行处理。

### 10.5 结果协议

```ts
type RemoveManagedLocalPluginFailure =
  | 'plugin_catalog_stale'
  | 'plugin_catalog_invalid'
  | 'plugin_catalog_generation_exhausted'
  | 'plugin_state_unavailable'
  | 'plugin_state_persist_failed'
  | 'plugin_state_capacity_exceeded'
  | 'plugin_revision_exhausted'
  | 'plugin_ownership_unavailable'
  | 'plugin_ownership_persist_failed'
  | 'plugin_ownership_capacity_exceeded'
  | 'plugin_ownership_revision_exhausted'
  | 'plugin_ownership_conflict'
  | 'plugin_remove_discovery_unavailable'
  | 'plugin_remove_not_managed'
  | 'plugin_remove_requires_disabled'
  | 'plugin_remove_storage_unavailable'
  | 'plugin_remove_identity_changed'
  | 'plugin_remove_staging_capacity_exceeded'
  | 'plugin_remove_write_failed'

type RemoveManagedLocalPluginResult =
  | {
      schemaVersion: 1
      status: 'removed'
      pluginId: string
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 1
      status: 'removedCleanupPending'
      pluginId: string
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 1
      status: 'removedCatalogUnconfirmed'
      pluginId: string
      reasonCode: 'plugin_remove_publication_unconfirmed'
      snapshot: PluginCatalogSnapshotV3
    }
  | {
      schemaVersion: 1
      status: 'notRemoved'
      disabledDecisionSaved: boolean
      reasonCode: RemoveManagedLocalPluginFailure
      snapshot: PluginCatalogSnapshotV3
    }
```

- `removed`：权威快照确认目标不存在，quarantine 与 index 已清理。
- `removedCleanupPending`：rename 已提交且快照确认，物理清理或 index 删除待处理。
- `removedCatalogUnconfirmed`：rename 已提交，或结果无法排除已经提交，但最终目录发布未确认；只 reload。
- `notRemoved`：后端已经证明未跨 rename；`disabledDecisionSaved=true` 表示本平台的 destructive 前置 disabled 屏障已确认（Unix 为目录耐久确认，Windows 为文档所述进程崩溃一致性），false 表示未确认。
- transport/parser unknown：显示结果未确认，只 reload，不重发命令。

所有可预期的 pre-rename 业务拒绝都使用 `notRemoved` 和上述完整 literal union；`plugin_remove_publication_unconfirmed` 只允许出现在 `removedCatalogUnconfirmed`。已经发生 rename 或无法排除 rename 的错误绝不能落入 `notRemoved`。Tauri command 的 `AppResult::Err` 只保留给参数反序列化/规范化失败、worker join/internal invariant 或响应运输失败；前端把它们全部视为 unknown，不根据任意后端 message 自动重试。

`disabledDecisionSaved` 交叉不变量：catalog/state/generation/discovery/ownership preflight、not-managed、requires-disabled、storage-unavailable、staging-capacity 与 state-save 失败只能是 false；`plugin_ownership_persist_failed` 和 `plugin_remove_write_failed` 只能是 true，因为它们只能发生在本平台 disabled 屏障之后。`plugin_remove_identity_changed` 是唯一允许 false/true 的 code：步骤 5 初检失败为 false，步骤 9 紧邻 rename 的复检失败为 true。ownership capacity/revision headroom 必须在步骤 7 前精确计算，因此相应 code 只能是 false。`plugin_remove_write_failed` 专用于步骤 10 rename 已证明未提交的失败；步骤 5/6 的根、source 或 staging 准备 I/O 失败使用 `plugin_remove_storage_unavailable / false`。若步骤 8 成功后回退 index 又失败，使用 `plugin_ownership_persist_failed / true`；若已无法证明 rename 未发生，则改走 `removedCatalogUnconfirmed`，不能靠 boolean 猜测。

所有带 pluginId 的分支都必须等于规范请求 ID；`notRemoved` 按定义不带 pluginId。每个 snapshot 必须通过完整 v3 parser。`removed` 的 snapshot 不得再含同 ID 的 managed/removalPending 项；可以含后来出现的 external 同 ID。`removedCleanupPending` 还要求 `managedOwnership.cleanupPendingCount > 0`。`removedCatalogUnconfirmed` 因定义上无法确认 publication，不对目标是否仍出现在所带最后完整 snapshot 中作断言。`notRemoved` 不声称项目一定仍存在，但其 code/boolean 必须满足上述交叉表。任何不合法组合整体作为 unknown，不部分采用 snapshot。

## 11. 崩溃与故障矩阵

### 11.1 导入

| 最后完成点 | 磁盘状态 | 恢复/展示 |
| --- | --- | --- |
| stage 前 | 无新增 | 未导入 |
| 双文件 stage 完整、disabled 前 | 只有当前调用拥有的 stage | 当前调用可准确清理；崩溃遗留无 index，不自动清理 |
| disabled 已保存、promotion 前 | stage + disabled | 未导入；旧 enabled 不恢复 |
| promotion 已执行、父目录 sync 前 | source 或目标可能存在 | 只按实际句柄观察；不宣称耐久，不删除目标 |
| 目标存在、index 登记前 | 双文件包，无 entry | external/non-removable；绝不自动补登记 |
| index save 失败 | 同上 | `importedExternal` 或未确认；包保留 |
| managed entry 已保存、scan 前 | 三方证明完整 | 下次权威扫描显示 managed disabled |
| publication 后、响应前 | 完整 managed 包 | 客户端 reload，不重放 token |

### 11.2 移除

| 最后完成点 | 磁盘状态 | 恢复/展示 |
| --- | --- | --- |
| disabled 重写前 | 原 managed 包 | 未移除 |
| 平台 disabled 屏障已确认、removing save 前 | 原包 + false | 未移除，历史 enabled 已清理 |
| 平台 removing 屏障已确认、rename 前 | source 精确存在 | 可回退 managed；不自动执行 rename |
| rename 已执行、sync 前 | source 或 removal target | 以 exact identity 判定是否跨提交点 |
| quarantine 存在、scan 前 | 包已退出发现根 | committed；不自动重试移除 |
| scan 后、cleanup 前 | catalog 不含目标，entry 持有 quarantine | cleanup pending；启动/reload 不 unlink |
| cleanup 后、entry 删除前 | 两侧无对象，removing entry | 可安全删除 stale entry |
| publication 后、响应前 | 已移除或 cleanup pending | 客户端 reload，不重发命令 |

### 11.3 冲突

source 与 target 同时存在、对象身份不一致、额外文件、多个 entry、index/receipt 内容不一致或 worker 无法确认提交点时：

- 在受支持的单写者边界内不删除任何对象；Unix/macOS 若越界交换被 best-effort 检查发现，且 replacement 已被 rename，只保留其 quarantine，不继续 cleanup；未检测到的恶意并发不在保证范围；
- 保留 entry 与文件作为证据；
- ownership summary degraded；
- 返回净化的冲突/未知结果；
- 日志不包含配置路径、槽位、receipt、manifest 或对象 ID。

cleanup 若在删除 manifest 后中断，receipt 必须仍留在 quarantine；若两个已知文件都已删除但 rmdir/index delete 未完成，则只记录空目录或 stale entry。恢复可以识别这些单调减少的已知形状，但启动/reload 不继续 unlink/rmdir。Windows 突然掉电后若独立文档目录项与 package rename 的耐久顺序发生回退，reconciliation 只能按 source/target/index 的实际安全形状分类；无 entry 的 quarantine 进入 conflict/cleanup-pending 维护计数并永久保留，不能从 receipt 重建 entry。

## 12. 并发与取消

- 初次发现、reload、state retry、set_enabled、commit import、ownership reconcile 和 remove 共用现有 FIFO `operation_gate`。
- 快速只读 get 只能看到前一个完整 registry snapshot，不能看到半登记 index 或半隔离包。
- remove 在 gate 内重新检查 generation；前端不能把旧确认换成 store 的新 generation。
- toggle 不能穿过 disabled 与 rename 窗口；即使启停只推进 revision、不推进 catalog generation，remove 也必须在 gate 内重新读取当前 status，已经恢复为 enabled 时以 `plugin_remove_requires_disabled` 拒绝且不访问移除 storage。
- import/remove/reload 串行，同 ID 新内容不能被旧响应删除或复活。
- reservation 后的 owned task 不因调用方取消而中断；取消只影响响应传递。
- 前端 remove 使用独立 flight owner，但 snapshot 统一进入现有 BigInt generation/revision 仲裁。
- 多进程写入仍不支持；文档要求只保留一个 EasiFlux 插件写入者，并且生命周期调用期间不得人工编辑 `plugins` 树。违反此边界时实现只在可观察检查点 best-effort 检测并停止；Unix/macOS 不承诺检测所有交换，也不承诺 name-based rename/unlink 从未作用于被同时交换的对象。

## 13. 跨平台文件系统契约

应从现有 import platform 层提取共享 package filesystem adapter，不能在 removal 中退化为普通路径 `exists` 检查后删除。

### 13.1 通用

- 固定绝对根逐组件打开，拒绝 `.`、`..`、空组件、URI、非法槽位和 Windows ADS。
- 子项只能通过持有父句柄按单个固定组件打开。
- 目录不得为 link/reparse；manifest/receipt 必须普通、单链接、精确大小写；无额外项。
- rename 前后比较 index 中三个对象身份，并重新解析 manifest/receipt。
- 需要 rename 时按平台要求关闭会自阻塞的子句柄，但保持完整祖先链。
- cleanup 先重验两文件，按 manifest -> receipt 的顺序 unlink，再确认空目录后 rmdir；禁止 `remove_dir_all`。中断后只接受 index 所绑定对象的单调子集，不接受新文件。Windows 必须优先使用句柄绑定 disposition；Unix/macOS 的 `unlinkat` 依赖下述单写者边界。
- 身份变化、replacement 或额外项立即停止并保留对象；post-rename 身份不匹配时 index 保持 removing、quarantine 保持原样、禁止 cleanup/index delete，并推进 conflict/cleanup-pending publication。

### 13.2 Linux

- 使用 `openat`/`unlinkat`、`O_NOFOLLOW | O_DIRECTORY` 等现有句柄相对模式；
- promotion 与 quarantine 使用 `renameat2(RENAME_NOREPLACE)`；不支持则失败；
- rename/cleanup 后 sync source 与 destination 目录。
- `renameat2` 与 `unlinkat` 仍按名称解析最后组件，不能把已经打开的 source/child inode 作为操作对象；因此支持边界要求唯一应用写者且调用期间没有同一用户人工换名。紧邻操作前和操作后重验只是 best-effort 检查，不宣称阻止或检测全部违规竞态。
- 若 post-rename 检查发现 identity 不同，可能已经把 replacement 移入固定 removal-staging；必须保留该目录与 removing entry、发布 conflict/unconfirmed，绝不继续 unlink 或删除 index。测试只验证可控注入点能被发现并 fail closed，不要求无法由 POSIX API 实现的“replacement 从未移动”或持续恶意竞态全检测。

### 13.3 macOS

- 使用 `openat`/`unlinkat` 句柄相对与 no-follow 校验；
- 使用 `renameatx_np(RENAME_EXCL)`；不支持 exclusive rename 的 volume 上失败；
- 同步父目录，不扩大为 OS 未保证的断电事务。
- 与 Linux 相同，`renameatx_np`/`unlinkat` 的最后组件是名称式操作；同一用户并发交换超出支持边界。若 best-effort post-operation 检查发现不匹配，只触发保留 quarantine、保留 entry 和 degraded publication，不继续 cleanup；不承诺发现所有交换。

### 13.4 Windows

- 每个祖先和子项继续通过相对父句柄的单组件 `NtCreateFile` 打开；
- 拒绝 reparse point、大小写混淆、ADS、非普通文件和 identity 查询失败；
- 使用 `FILE_ID_INFO` 的 volume serial + 128-bit FileId；
- source package 目录句柄以 `DELETE` 权限持有；验证用子句柄在 rename 前关闭，祖先链与 source 目录对象继续固定；
- 使用 `SetFileInformationByHandle(FileRenameInfo)` 直接重命名已持有的 source 目录句柄，`FILE_RENAME_INFO.ReplaceIfExists=false`、`RootDirectory` 为已持有的目标父目录句柄、`FileName` 为经过校验的单组件；
- 不使用绝对路径 `MoveFileExW`，也不在句柄相对 rename 不可用时降级为路径 rename、覆盖或跨卷复制；
- rename 后从固定目标父句柄重开并比较三个对象身份；
- source rename 绑定已持有目录句柄；manifest/receipt cleanup 也必须通过已验证 child handle 的 disposition 或等价对象绑定操作完成，不能关闭后按绝对路径删除；
- 不宣称抵御同一 OS 用户持续控制宿主账户，但实现与测试必须证明最后 source 名称交换不会让 rename 作用于 replacement。

## 14. 错误闭集与 ACL

除既有 catalog/state 错误外，新增精确 code：

| code | 用户语义 |
| --- | --- |
| `plugin_ownership_unavailable` | 所有权记录暂不可用，不能受管导入或移除。 |
| `plugin_ownership_persist_failed` | 无法保存 managed index。 |
| `plugin_ownership_capacity_exceeded` | managed index 已达上限。 |
| `plugin_ownership_revision_exhausted` | managed index 版本耗尽。 |
| `plugin_ownership_conflict` | index、receipt 与当前包不一致，不会删除。 |
| `plugin_import_ownership_not_registered` | 包已导入，但没有建立可移除的受管所有权。 |
| `plugin_remove_discovery_unavailable` | 当前本地发现状态不足以安全确认移除目标。 |
| `plugin_remove_not_managed` | 当前包不是 EasiFlux 受管包。 |
| `plugin_remove_requires_disabled` | 请先停用再移除。 |
| `plugin_remove_storage_unavailable` | 无法安全打开或准备当前包与移除暂存根。 |
| `plugin_remove_identity_changed` | 确认后包内容或对象已变化。 |
| `plugin_remove_staging_capacity_exceeded` | removal staging 需要人工检查。 |
| `plugin_remove_write_failed` | 未能完成包隔离。 |
| `plugin_remove_publication_unconfirmed` | 已越过或无法排除越过移除提交点，目录结果未确认。 |

前端只接受精确 `{ code, message }`，仅按 code 映射固定中文，不回显后端 message。`disabledDecisionSaved` 必须来自真实持久化步骤。

Tauri app manifest、invoke handler 和自动 permission 只新增 `remove_managed_local_plugin`。`plugin-runtime` capability 继续只绑定本地 `main` WebView，无 remote URL、wildcard window、filesystem、shell、dialog 或动态 command。ACL 测试检查所有 capability 的联合权限。

## 15. 前端 UX 与可访问性

### 15.1 管理来源

- builtIn：`内置 · 随应用提供`，无移除入口；
- managed：`本地声明式包 · EasiFlux 管理`；
- external：`本地声明式包 · 外部放置，应用不会删除`；
- ownershipConflict：说明管理记录不一致，要求人工检查；
- ownershipUnavailable：显示所有权子系统警告，但不遮蔽目录浏览。
- removalPending：说明尚未跨删除提交点但 index 回退待完成，禁用启停/移除；顶层分别展示 rollback pending、cleanup pending 与 conflict 数量。

历史无 receipt/index 包必须稳定显示为 external，不能隐藏，也不能显示灰色但可触发的删除按钮。

### 15.2 移除交互

- 入口只在“已安装插件”和“插件管理”，不在“插件市场”。
- enabled managed 项显示“请先停用”；disabled managed 项显示“移除本地包”。
- 独立 `PluginRemovalDialog` 使用当前 catalog item 与捕获的 generation，不从 DOM 重建身份。
- dialog 具有 `role=dialog`、`aria-modal`、标题/描述关联与焦点约束；初始焦点不放在破坏性确认。
- Escape 在提交前取消并恢复触发按钮；提交中不可伪取消，使用 `role=status`、`aria-busy`。
- 新 generation 到达会使未提交确认失效；同 generation 的更高 revision 若改变目标 status，也会使确认失效。前端必须按当前 store 中的目标条目重新判断，不能只比较 generation 或继续使用 dialog 打开时捕获的 disabled 值。
- 不乐观删除卡片；严格结果中的 snapshot 通过统一仲裁后才更新。
- cleanup pending 与 publication/transport unknown 只提供 reload/维护指导，不自动重发移除。
- 页面卸载不取消已接受任务；迟到 snapshot 可仲裁但不抢新页面焦点。
- manifest 文案普通文本渲染，长 ID 换行并方向隔离，不解释 HTML 或链接。

## 16. 模块与文件责任

建议新增：

```text
src-tauri/src/plugin/ownership.rs
  package receipt、index entry、管理分类与严格领域校验

src-tauri/src/plugin/removal.rs
  remove 结果 union 与失败闭集

src-tauri/src/plugin/runtime/removal.rs
  disabled -> removing index -> quarantine -> scan -> cleanup 协调

src-tauri/src/storage/managed_plugin_ownership.rs
  有界 index、候选恢复与原子持久化

src-tauri/src/storage/local_plugin_package.rs
  import/removal 共用固定目录、对象身份、exclusive rename、准确清理

src/components/plugins/PluginRemovalDialog.vue
  确认、焦点、提交和部分结果语义
```

主要修改：

```text
src-tauri/src/plugin/discovery/safe_fs.rs   双包形状、receipt 读取与生产 locator
src-tauri/src/plugin/discovery.rs           DiscoveredLocalPlugin 与 slot/identity 绑定
src-tauri/src/plugin/record.rs              复用安全身份，不承载 locator
src-tauri/src/plugin/registry.rs            locals locator、catalog v3、disabled 事务
src-tauri/src/plugin/runtime.rs             ownership store/reconcile/remove 入口
src-tauri/src/plugin/runtime/import.rs      双文件 stage 与 promotion 后 index 登记
src-tauri/src/storage/local_plugin_import.rs 迁移到共享 package adapter
src-tauri/src/commands/plugin.rs             固定 remove command
src-tauri/src/state.rs                       ownership 初始化失败隔离
src-tauri/src/lib.rs / src-tauri/build.rs    command 与 ACL
src-tauri/capabilities/plugin-runtime.json   local main 最小 permission
src/types/plugin.ts                          catalog v3、import v2、remove union
src/services/pluginService.ts                严格 parser 与 command adapter
src/stores/plugin.ts                         remove flight 与统一 snapshot 仲裁
src/components/plugins/PluginCard.vue        管理来源与移除入口
src/components/plugins/PluginMarketplacePage.vue 结果与 ownership summary
docs/plugin-local-manifests.md               新包形状、历史 external 与维护说明
.github/workflows/ci.yml                     三平台安全矩阵
```

不要把 receipt/locator 塞进 `PluginRecord`，也不要把更多平台代码继续堆入现有 import 文件。内容身份、文件对象和管理登记必须是独立边界。

## 17. 测试与 CI

### 17.1 Receipt 与 index

- receipt/index 的 duplicate、unknown、missing 字段与非规范 ID/slot/hex/fingerprint；
- index 160/161 entries、256 KiB、revision `MAX-2/MAX-1/MAX`；receipt 4 KiB；
- 重复 receiptId/slot/plugin/object identity；
- missing index 为空 available，历史包 external；
- main/tmp/bak 候选；pending/bak.pending 永不成为 authority；非主恢复 rewrite 前不授权删除；未来 schema/主超限终止；
- ownership 与 destructive plugin-state 的 main/tmp/bak/pending/bak.pending symlink/reparse/hardlink、预存工作对象、大小写和交换测试；任何写侧对象都不会被路径式 create+truncate 跟随；
- 安全文档 store 每个写入/替换/sync 注入故障保持旧内存/磁盘边界；Linux/macOS 父目录 sync 未确认时 durable-before-rename 屏障拒绝 package rename；Windows 覆盖进程崩溃恢复及文档化掉电边界。

### 17.2 发现与管理分类

- 合法 legacy 单文件 external 与合法双文件 managed candidate；其他形状拒绝；
- receipt 无 index、index 无 receipt、字段不一致、digest 不一致、对象 ID 不一致均不可管理；
- 同内容复制/重建因对象 ID 改变而 external/conflict；
- 相同 manifest 换槽位、receipt 增删、对象重建、management/ownership eligibility basis/toggleBlockReason 或 ownership summary 任一变化都推进 generation；同一 `(revision,generation)` 的完整 DTO 必须等价，MAX 时在 storage 前拒绝；status-only toggle 导致的 canRemove 变化只推进 revision，mutation 保持同 generation 且结构/ownership 字段不变；
- legacy/manual 单文件在 index unavailable 时仍 external 且可按 state toggle；有效 receipt candidate 才 ownershipUnavailable；removalPending 才有 toggleBlockReason；
- local 2 MiB 与 removal-staging 328 KiB 独立记账；quarantine 超限/未知内容只关闭 ownership 写，不把 localDiscovery 改成 unavailable；
- receipt/slot/object/fingerprint 不进入 transport。

### 17.3 文件系统安全

- 根、package、manifest、receipt 的 symlink/junction/reparse、hardlink、FIFO/device、ADS、大小写、额外项；
- 验证与 rename 边界对象交换；Windows 证明 source handle 绑定，Unix/macOS 只在可控注入点证明 best-effort post-check 发现后保留 quarantine/entry 且不 cleanup，不声称覆盖持续恶意竞态；
- import 四次完整 stage 碰撞、remove 单槽单次碰撞后回退、同卷检查、exclusive rename 不覆盖；
- cleanup 只处理精确两个文件和空目录；额外项停止；Windows child-handle disposition 不按路径重开，Unix/macOS 在单写者边界下使用 unlinkat；
- Linux `RENAME_NOREPLACE`、macOS `RENAME_EXCL`、Windows handle-relative `SetFileInformationByHandle(FileRenameInfo, ReplaceIfExists=false)`；
- Windows 子项均为单组件相对 `NtCreateFile`；Unix 持有 parent handle 并 sync；
- 生命周期生产路径没有 `remove_dir_all`、通用绝对删除或先 exists 后覆盖 rename。
- full target、manifest 已删/receipt 保留、两文件均删/空目录保留、目录已删/index 保留等 cleanup 中断形状逐一绑定存活 identity 并计 cleanup pending；manifest-only/额外项计 conflict；reload 不继续删除。

### 17.4 Runtime 崩溃与并发

- 第 11 节每个检查点用 `current_exe` child 和只终止 child 的 watchdog；
- import 必须 disabled -> promotion -> target verify -> index；index 失败保留 external，不删包；
- remove 必须 disabled -> removing index -> rename -> scan/cleanup；顺序不可颠倒；
- stale、builtIn、external、enabled、blocked、unknown 在 storage 前拒绝；
- import/remove/reload/toggle FIFO 与 caller cancellation；
- dialog 打开后同 generation 的 toggle 将目标重新启用时，前端确认失效，后端即使收到旧 UI 请求也在移除 storage 前拒绝；
- post-rename index cleanup、physical cleanup、scan、response 故障均返回 committed 语义；
- 完整 `RemoveManagedLocalPluginFailure` 闭集及 disabledDecisionSaved false-only/true-only/identity-before-or-after 交叉组合；未知 rename 结果绝不进入 notRemoved；
- reload/startup 不自动 retry rename 或 unlink quarantine；
- 迟到 remove snapshot 不删除同 ID 新 generation 项。

### 17.5 前端

- catalog/mutation v3 exact keys、枚举、三个 summary count、toggleBlockReason 和跨字段不变量；
- import v2 与 remove v1 所有分支，unknown/extra/wrong types 拒绝；
- command 只发送 id + expectedCatalogGeneration；
- external/builtIn 无入口，enabled managed 先停用；
- dialog 焦点、Escape、提交中不可关闭、普通文本；
- BigInt generation/revision、remove flight owner、unmount、迟到响应；
- unknown/committed pending 只 reload，不自动重试。

### 17.6 三平台 CI 与完整回归

保留现有 `plugin-security` Windows/Linux/macOS matrix，并新增实际非零 filter：

```text
storage::managed_plugin_ownership::tests
storage::local_plugin_package::tests
plugin::ownership::tests
plugin::runtime::removal::tests
```

现有 safe_fs、import、plugin_state、runtime import 过滤继续运行。每个 filter 必须实际执行测试，不只是编译。

完整验证：

```text
pnpm test
pnpm exec vue-tsc --noEmit
pnpm lint
pnpm build
cargo test --locked --all-targets
cargo clippy --locked --all-targets
changed Rust paths rustfmt --check
git diff --check
```

ACL 联合回归与静态检查还要证明：frontend 无路径/receipt/object-ID 参数；Rust 无下载、执行、动态 command、`remove_dir_all` 或覆盖 rename。

原生冒烟只在一次性配置根、VM 或可销毁 OS 账户中执行：新导入为 managed、重启仍 managed、历史包 external、enabled 必须先停用、移除后不再发现、主业务页面可用。没有隔离 profile 时明确标为未验证，不能拿真实用户配置目录试验删除。

## 18. 兼容性与迁移

- manifest schema 保持 1；contributions/requestedCapabilities 继续为空。
- plugin-state schema 保持 2；移除保留 disabled identity。
- catalog snapshot 与 mutation transport 都从 2 升为 3，前后端同分支升级并严格拒绝混用。
- import commit transport 从 1 升为 2，用于表达 `importedExternal`。
- ownership receipt/index schema 均从 1 开始；index 缺失是安全空状态。
- 旧版本不会读取顶层 index，但其 Phase 1A 单文件扫描会把带 receipt 的双文件 managed 包拒绝为额外文件；因此降级时这些包可能暂时不可见，而不是继续显示为 managed。重新升级且对象未变时可重新建立三方匹配。
- 旧版本降级期间新导入的单文件包没有 receipt/index，重新升级后必为 external。
- 配置目录复制、备份还原或跨 volume/平台迁移通常改变对象身份；管理关系安全失效，不自动重绑。
- malformed receipt 的双文件包被发现层拒绝，而不是退化成 managed；用户可退出应用后人工修复。

## 19. 完成判据

1. 无完整 receipt/index 的任何包，包括历史 PR #29 包，都没有移除能力。
2. 新导入仅在双文件包提升、目标对象重验、index 原子登记和 disabled snapshot 全确认后报告 managed success。
3. promotion 后 index 登记失败保留包并显示 external/non-removable；不回滚删除、不自动补登记。
4. 移除只接受 receipt + index + 当前 slot + 三个对象 identity + manifest fingerprint 全匹配的 disabled localDeclarative 包。
5. quarantine exclusive rename 是唯一移除提交点；其后任何失败都是 committed pending/unconfirmed，绝不自动重试整个操作。
6. cleanup 非递归且仅处理 index/receipt 持有的精确对象；启动/reload 不自动 unlink quarantine。
7. ownership 损坏最多关闭 managed import/remove，不阻止核心应用和插件只读发现。
8. 所有崩溃点、竞态、ACL 和三个平台真实文件系统安全过滤通过。
9. ownership store 的五个 authority/work 名全部使用句柄相对安全协议；Linux/macOS destructive 前置文档的父目录 sync 未确认时绝不 rename package。
10. 任一已发布 ownership/locator/management 能力变化推进 catalog generation；相同 revision/generation 对应完全等价 DTO，耗尽在写盘前拒绝。
11. rollback pending、cleanup pending 与 conflict 有互斥可解释的计数；完整 remove failure union 和 disabledDecisionSaved 组合由前后端严格拒绝非法响应。
12. Unix/macOS 明示单写者、无人工并发修改边界；越界后所有防误删保证失效，best-effort 检查发现偏差才保留/降级。Windows source rename 与 child cleanup 使用对象绑定句柄操作。

## 20. 后续阶段

本阶段之后才可独立设计：

1. 历史/external 包 adoption，必须重新预览和明确确认；
2. cleanup-pending 的显式安全清理 UI；
3. 同 ID 更新与回滚；
4. 签名市场、真实信任根、撤销与审计；
5. 第一个声明式 contribution/capability executor；
6. 多进程单实例或跨进程写锁。

## 21. 裁决摘要

1. `pkg-*` 永远不是删除授权。
2. 所有权必须由包内 receipt、独立 managed index 和当前对象身份三方证明。
3. 历史无证明包全部 external，不自动认领。
4. 导入先写双文件 stage、保存 disabled、exclusive promotion，目标重验后才登记 index。
5. promotion 后 index 失败保留 external 包，绝不为了伪原子性回滚删除。
6. 移除先重写 disabled，再保存 removing entry，最后 quarantine rename。
7. rename 后的 scan/index/cleanup 失败都属于 committed pending/unconfirmed。
8. destructive 操作不因 reload、启动或未知响应自动重试。
9. cleanup 只处理精确对象且非递归。
10. 所有生命周期写操作共用 FIFO gate；跨进程仍不支持。
11. ownership eligibility 所派生的任何已发布能力或 summary 变化都推进 catalog generation；status-only toggle 引起的 canRemove 投影变化按第 8.1 节只推进 revision。
12. removal 每个请求只使用一个已登记槽位；碰撞先回退并结束，不换槽重试。
13. index 的 main/tmp/bak/pending/bak.pending 全部属于句柄相对安全存储协议，destructive rename 前使用更强耐久屏障。
14. Unix/macOS 的 name-based 操作依赖单写者边界；越界交换不保证检测，只有 best-effort 检查发现时才保留、降级并停止清理。

## 22. 技术依据

- 仓库现有 `plugin/discovery/safe_fs.rs`：固定根、no-follow、单链接普通文件和有界读取。
- 仓库现有 `storage/local_plugin_import/platform.rs`：句柄相对打开、`volume + object` 身份、exclusive rename 与准确清理。
- 仓库现有 `plugin/runtime.rs`：FIFO reservation、owned task 与权威发布。
- 仓库现有 `storage/atomic_file.rs`、`storage/plugin_state.rs`：只作为候选恢复与 clone -> persist -> commit 的逻辑参考；ownership 安全工作槽和 destructive 前置耐久屏障不得直接复用路径式 replace。
- Linux `renameat2(RENAME_NOREPLACE)`：https://man7.org/linux/man-pages/man2/rename.2.html
- Apple exclusive rename volume 能力：https://developer.apple.com/documentation/foundation/urlresourcekey/volumesupportsexclusiverenamingkey
- Microsoft `SetFileInformationByHandle`：https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle
- Microsoft `FILE_RENAME_INFO`（relative target 可绑定 `RootDirectory`，`ReplaceIfExists=false` 拒绝覆盖）：https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_rename_info
- Microsoft `FlushFileBuffers`（只承诺已打开文件句柄的数据 flush，本文不据此扩张为 Unix 目录 fsync 等价保证）：https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers
- Microsoft `FILE_ID_INFO`：https://learn.microsoft.com/windows/win32/api/winbase/ns-winbase-file_id_info
