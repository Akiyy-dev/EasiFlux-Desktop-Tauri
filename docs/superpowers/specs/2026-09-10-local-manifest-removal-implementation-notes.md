# 受管本地插件包移除：实现补充与裁决记录

日期：2026-09-10

分支：`plugin/local-manifest-removal`

原设计：[Phase 1C 设计](2026-09-09-local-manifest-removal-design.md)

本文记录实现中验证得到的补充约束；与原设计或原计划的字面 API、测试步骤冲突时，以本文为准。它不扩大插件执行权限或移除范围，也不表示全部验证已完成。

## 已落实的接口与安全边界

- 历史单清单包保持 external；新导入包必须同时具备 receipt、index 和实际对象身份匹配才可受管移除。
- Windows 文档替换使用持有源文件和父目录句柄的 `NtSetInformationFile(FileRenameInformationEx)`，标志为 `FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS`。原计划中的 Win32 包装调用在本机探测失败，不能作为回退。
- Windows 包提升、隔离使用 `NtSetInformationFile(FileRenameInformation)`，持有源目录及目标父目录句柄，`ReplaceIfExists=false`。包重命名与文档替换是不同操作，不共享覆盖权限。
- Windows cleanup 不把 disposition 成功等同于文件已经消失：先释放内部持有句柄，再相对父目录证明名称不存在。删除前验证全部残存对象及内容，清单先于 receipt 删除。
- 移除只执行一次隔离后的物理扫描，然后最多一次精确清理和索引更新；最后根据已验证结果做不读盘、不写盘的确定性 reconciliation，并一次发布。启动和 reload 的安全索引收敛不能在此被重复调用。
- 导入提升结果区分未提交、已提交和提交未知。未知提升不能被误报为未导入，不能清理可能已经提升的包，也不能用未确认的扫描覆盖原完整快照。
- 底层文档与公开快照暂时不一致时，保留旧完整快照用于展示，但关闭生命周期修改权限，直到权威扫描与发布完成。
- Unix/macOS 的单写入者、调用期间不人工改目录边界仍成立；Windows 只声明进程崩溃一致性，不声明突然断电耐久性。

## 容量测试的可达性

生产限制不变。16 个 removal 项每项最多读取 16,385 字节清单探测和 4,097 字节 receipt 探测，最大合计 327,712 字节，低于 328 KiB 的总预算。预算失败测试使用仅测试可注入的较低上限，不放宽其他限制来制造不可达输入。

同样，严格字段约束下 160 条 ownership entry 的最大合法序列化不足 256 KiB；字节容量顺序测试使用较低的测试上限，生产条目和字节限制均保留。

## 前端协调

移除具有独立的确认、请求和结果所有者，不复用导入或启停的 pending 状态。请求次序在提交时分配，而非响应时分配。相同 generation 不能改变成员、清单、来源、管理属性或 ownership summary；更高 revision 只能承载状态及其 canRemove 投影变化。卡片只随采纳的权威快照消失，不做乐观删除。

管理视图复用 PluginCard；未知结果只能重新加载观察，不能自动再次执行移除。移除视图占用期间禁用其他移除入口，但导入和启停保持独立，由后端 FIFO 和确认失效规则处理竞态。

## 验证安排

用户于 2026-09-10 明确要求减少中途测试。后续实现只运行直接相关的定向检查，完整 frontend、Rust、类型、lint 和 build 在集成完成后统一运行一次。未修改的已通过测试不重复运行。

Windows 包存储的有界串行检查复现了拒绝访问（OS 5）：38 项中 37 项通过，失败发生在初始导入的 BeforePromotion 之后、AfterPromotion 之前。旧日志不能区分最后一次内容检查与原生重命名；已增加仅测试使用的精确操作标签和提交状态诊断。只读句柄分析没有找到已证实的应用侧生命周期缺陷，也没有证实外部进程干扰。随后一次全量 Rust 测试通过不等于问题已解决，因此本阶段仅提交草稿 PR，不合并。

截至 `9203c4c` 的验证组合如下；前端小修正后只检查受影响部分，没有重复全套测试：

| 检查 | 结果 |
| --- | --- |
| 全量前端 | 78 个文件、1,242 项通过 |
| 焦点/规范修正 | dialog/card/page 75 项通过；后续引用比较小修正的 dialog 6 项通过 |
| 全量 Rust all-targets | 1,099 项通过；2 个 ignored 是由父测试显式启动的崩溃辅助进程 |
| TypeScript + Vite build | 通过；保留原有大 chunk 提示 |
| ESLint | 0 错误、78 个既有警告 |
| Clippy all-targets | exit 0；项目警告仍在，未压制警告 |
| Rust 格式 | 39 个已修改且仍存在的 Rust 文件通过 |

本机验证为 Windows。Linux/macOS 的原生文件系统行为仍由三系统 CI 检查；尚未运行隔离配置下的原生整机界面 smoke test，绝不为此对真实用户配置目录执行删除测试。

## Rulings I made

以下是执行账本中截至本补充记录创建时的完整裁决，按原顺序保留，包括判断错误时的代价。后续新增裁决应追加到本节，不能只保存在忽略目录。

Ruling: Unix/macOS lifecycle rename and unlink are supported only with one EasiFlux writer and no manual edits during the call; checks at injected boundaries are best effort, while Windows must bind rename and child deletion to held handles — POSIX name-based primitives cannot guarantee complete detection against a same-user adversary — if wrong, the implementation or documentation must be reworked and affected platform safety claims withdrawn.

Ruling: A status-only toggle may change `canRemove` while advancing only plugin-state revision and keeping catalog generation fixed; ownership eligibility changes still advance generation — this preserves the single-item mutation protocol while keeping equal `(revision,generation)` snapshots equivalent — if wrong, toggle must migrate to a full-snapshot protocol and every caller/parser/store test must change.

Ruling: Removal performs exactly one final public reconciliation after quarantine and the one cleanup/index attempt; the earlier post-rename scan remains private — one generation headroom is sufficient and no half-cleaned snapshot becomes observable — if wrong, removal must reserve multiple generations and define adoption of every intermediate snapshot.

Ruling: Each remove request records one removal slot and makes one rename attempt; a proven collision rolls back and ends rather than changing slots — this keeps the persisted state machine and retry semantics closed — if wrong, a new persisted `removing(old)->removing(new)` transition and crash matrix are required.

Ruling: Full, receipt-only, empty-directory, and both-absent recognized post-commit shapes are cleanup pending; manifest-only, extra, orphaned, identity-mismatched, or commit-uncertain shapes are conflict — this matches the manifest-first/receipt-last monotonic cleanup protocol without authorizing automatic recovery deletion — if wrong, reconciliation counts and result cross-field parsers can misclassify retained data.

Ruling: Local discovery keeps an independent 2 MiB budget and removal reconciliation gets an independent 328 KiB budget — quarantine residue must not make readable local plugins unavailable — if wrong, hostile or stale removal content could deny core plugin discovery or the scanner could read without a bound.

Ruling: Both plugin-state and ownership destructive saves use the secure five-name document adapter with explicit commit outcomes; Linux/macOS require directory durability, while Windows explicitly promises only process-crash consistency — the existing path-based AtomicFile post-sync semantics cannot serve as a destructive precondition — if wrong, a crash or unsafe reserved work object can separate deletion authority from the package transition.

Ruling: Expected pre-rename removal failures are a closed structured result union; `identity_changed` alone may be false or true by phase, `write_failed` is true-only after the disabled barrier, and uncertain rename outcomes are never `notRemoved` — exact frontend parsing needs a unique truth table — if wrong, the UI can claim no deletion after a committed operation or accept an impossible response.

Ruling: On Windows, the held-source object-bound replacement uses user-mode `NtSetInformationFile(FileRenameInformationEx)` with `FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS`, the same verified destination-parent handle, and `FILE_RENAME_INFORMATION`, superseding the plan's literal `SetFileInformationByHandle(FileRenameInfo)` call — controlled same-buffer testing on this host returned Win32 error 87, native basic rename succeeded only for an absent target but returned access denied for a held target, and native Ex replacement preserved both held handles and reopened source identity; Microsoft documents the Nt entry point for user-mode callers — if wrong, Windows lifecycle persistence/import/removal must be disabled until another held-handle primitive is proven; basic/path fallback and closing validation handles remain forbidden.

Ruling: The production removal scanner retains the 328 KiB hard cap, but its overflow/independence regression uses a test-only lower aggregate ceiling — with at most 16 items and 16,385 + 4,097 probe bytes per item, the literal maximum is 327,712 and the plan's requested 335,873rd byte is unreachable without weakening another cap — if wrong, removal-budget tests could give false confidence and the scanner/accounting contract must be redesigned; production caps are not relaxed to manufacture the case.

Ruling: An owned remove performs one physical authoritative post-rename scan, one cleanup attempt, one index attempt, then a no-load/no-write deterministic final reconciliation from handle-proven outcomes; removal never invokes reload/startup's safe-write convergence again — `BothAbsent` is physical cleanup complete with only stale-index work pending, a committed index-delete error is adopted but returns `removedCatalogUnconfirmed`, a fully proven conflict candidate may publish once and return unconfirmed, and ambiguous evidence preserves the prior complete snapshot — if wrong, the result union needs a new committed-conflict branch or a second bounded scan and generation/publication rules must change.

Ruling: A slot already occupied during initial pre-disabled prepare maps to `plugin_remove_storage_unavailable/false`; after the barriers, only target `AlreadyExists` plus exact source proof and a distinct target proves `plugin_remove_write_failed/true`, while every other rename error is commit-unconfirmed — rollback adopts any committed ownership-document outcome without requiring a destructive durability barrier because no later destructive action follows, and only `NotCommitted` is rollback failure — if wrong, callers could replay a committed rename or misreport a safely stopped removal, so the failure union and rollback recovery matrix must be revised.

Ruling: The refactored import stage exposes monotonic `NotCommitted | Committed | CommitUnconfirmed` promotion knowledge in addition to an exact successful `PromotedManagedPackage`; post-rename verification/sync failure and an uncertain rename never permit cleanup or `notImported`, and Task 4 conservatively reuses the existing imported-not-visible response until Task 5 upgrades the protocol — if wrong, a package whose rename happened could be deleted/replayed or an uncommitted stage could be stranded, requiring import crash semantics and result parsing to change.

Ruling: Windows no-replace package promotion and quarantine use user-mode `NtSetInformationFile(FileRenameInformation)` with a held DELETE-capable source directory, held destination-parent handle, relative single-component name, and native `ReplaceIfExists=false`; a controlled real directory probe passed, while the Win32 wrapper remains forbidden by the earlier host evidence — if wrong, managed import/removal must fail closed on Windows until another object-bound exclusive rename is proven, never falling back to MoveFileEx/path/copy.

Ruling: Windows exact cleanup is successful only after disposition is issued on verified child/source handles, every internal duplicate handle is dropped, and a parent-relative reopen proves the name absent; disposition success alone is not absence because deletion waits for all handles — if wrong, collision retry could act while an old stage is still reachable, so retry must be disabled and evidence retained.

Ruling: Initial removal eligibility maps an absent, built-in, externally managed, or already-removal-pending target to `plugin_remove_not_managed/false`, while an exact ownership mismatch or duplicate managed claim maps to `plugin_remove_ownership_conflict/false`; removal does not perform an implicit persistence reload or convergence write before preflight and requires the current complete snapshot — if wrong, the closed result union needs new ineligible codes or removal admission must gain a separately specified initialization transaction.

Ruling: If the production 160-entry ownership model cannot physically reach its 256 KiB serialized-byte ceiling under all valid field bounds, removal capacity-order tests use a test-only lower sizing limit while retaining the production byte and entry caps — invalid model construction or weakened field bounds would test an impossible state — if wrong, a reachable production byte-cap case may be missed and the preflight fixture must be replaced with a literal maximum-size index.

Ruling: Task 8 treats absence of the generated removal permission and capability entry as a one-time RED precondition, not a permanent test assertion; GREEN must create and grant those exact artifacts — if wrong, the test suite would require the feature to be simultaneously absent and present and cannot converge.

Ruling: Task 8 keeps the concrete `Arc<PluginRuntime>` command helper and tests forwarding through injected lifecycle services, verifies branch serialization without constructing a full Tauri `State` harness, and bans `opener:` together with filesystem, dialog, and shell permissions while testing remote authority through capability resolution — if wrong, a new runtime abstraction or Tauri integration harness and its maintenance cost must be added.

Ruling: Removal snapshot request order is allocated when `confirmRemoval()` submits, before awaiting IPC, matching existing request-start arbitration; allocating on resolution would make completion timing choose equal-counter winners — if wrong, removal must adopt a distinct completion-order contract and all cross-flight tie tests must change.

Ruling: The manage view reuses `PluginCard` for removal entry instead of duplicating eligibility and button logic in bespoke page markup; stable removal/result/ownership test hooks are part of the component contract — if wrong, the manage layout needs a second independently maintained removal control and duplicate accessibility/eligibility tests.

Ruling: While any removal confirmation, accepted flight, or result owns the removal state, additional removal-entry buttons are disabled, but import and toggle ownership remain independent as specified by the store; the modal and backend FIFO still arbitrate races — if wrong, cross-flight UI disabling must be broadened and the independence guarantees/tests revised.

Ruling: Task 12 adds a narrow `cfg(test)` import-runtime hook after the reconciled candidate is built and before publication; it does not relabel the earlier discovery return as `candidate-built` — if wrong, the crash matrix either needs a production-visible observability seam or must drop that claimed boundary.

Ruling: The reproduced Windows package-test flake is a branch verification risk but not evidence of replacement or a Task 8 regression; Task 12 must add test-only operation/raw-OS diagnostics and a bounded parallel probe, with no weakened assertions or blind production retries, and must report the risk if no root cause is proven — if wrong, deferring it could hide a production handle-lifetime defect and the lifecycle branch must be held until a deterministic fix exists.

Ruling: Task 9 may minimally extend `PluginMarketplacePage.vue` only to complete its existing exhaustive fixed-copy map for the three new ownership preflight import codes; type checking cannot pass while the public union grows but its total consumer is left incomplete — if wrong, those mappings must move behind a shared presentation API and the page change be reverted.

Ruling: Task 12 may add a narrow cfg(test) diagnostic seam at storage/local_plugin_import.rs's I/O error mapping, where raw OS errors are otherwise erased — assertion-only diagnostics cannot recover operation errors after conversion to WriteFailed — if wrong, this increases test-only coupling and should be replaced with lower-level per-operation evidence; production messages and behavior remain unchanged.

Ruling: Task 11 UI implementation and Task 12 backend/CI proof run concurrently with disjoint owned files; Task 12 waits for controller permission before staging or committing — the active parallel-delegation instruction and user latency preference favor independent work, while serialized commits keep task review ranges isolated — if wrong, any shared-file or fixture dependency requires pausing the affected worker and reconciling before commit; no simultaneous index mutation is allowed.

Ruling: Preserve the pre-existing default capability's opener:default, while static union checks prohibit new dialog/fs/shell/remote authority and allow no additional opener grant; plugin-runtime remains exactly seven fixed commands with no opener or scope — default.json is unchanged from the import baseline and built-in link opening is outside plugin lifecycle authority — if wrong, an independent application-wide capability migration is required; claiming the entire app has no opener permission would be inaccurate.

Ruling: Treat the reproduced Windows OS 5 failure as unresolved reliability risk even though the later full suite passes; publish this stage only as a draft PR and do not merge — bounded tests and read-only handle analysis establish neither a code-level root cause nor external interference — if wrong, the draft may delay an otherwise usable feature, while prematurely declaring stability could hide intermittent failed imports/removals; no blind retry, permission broadening, or weaker assertion is accepted.
