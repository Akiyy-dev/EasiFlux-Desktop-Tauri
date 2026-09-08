# Local Manifest Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver single-file metadata manifest import with native selection, exact-content confirmation, durable disabled state and authoritative catalog publication, without executing plugins.

**Architecture:** Extend the existing Phase 1A runtime with one in-memory import session and a shared operation gate. Secure source reading captures canonical content once; storage stages it outside discovery, registry persists disabled, and an exclusive same-volume directory promotion precedes a full discovery publication. Secondary state recovery disables local identities without changing state or catalog schemas.

**Tech Stack:** Rust, Tauri 2, Rust-only tauri-plugin-dialog 2, Tokio, serde, existing SHA-256/UUID/rustix/windows-sys, Vue 3, Pinia, TypeScript, Vitest/Vue Test Utils; tempfile as a Rust test-only dependency and libc only for the macOS exclusive-rename adapter.

**Spec:** `docs/superpowers/specs/2026-09-08-local-manifest-import-design.md` — read the complete approved document before each task; this plan implements its 2026-09-08 approved revision, including §3.5 secondary recovery normalization.

## Global Constraints

- 用户从插件页选择一份 JSON 清单，查看宿主解析后的准确内容，确认后由宿主复制成固定目录中的合法包。导入结果默认停用，不执行代码、不解释贡献、不授予权限。
- 不执行插件或宿主贡献；`contributions`、`requestedCapabilities`、`grantedCapabilities` 仍为空。
- 不下载、市场安装、自动更新、覆盖更新、版本降级、卸载、回滚或删除手工包。
- 不认证自报的 `publisher` / `publisherId`，不创建通用 `trusted` 标记或授予未来权限。
- 不接收前端提供的路径、URL、原始 JSON、目标槽位、来源或 fingerprint。
- 不改变 manifest schema 1、catalog transport 2 或 state schema 2。
- 不实现跨进程插件状态协调；并发运行多个 EasiFlux 进程时的插件写操作不受支持。
- 采用“完整暂存 → 持久化停用 → 提升包 → 权威扫描发布”。不为一个清单引入跨文件事务日志。
- 现有 state store 在主文件无效时可以读取合法 `.tmp` / `.bak`。为避免导入前备份中的本地 `enabled=true` 在主文件损坏后复活，从任何非主候选恢复时都把所有 `localDeclarative` 条目规范为 `enabled=false`，并标记需要安全重写；built-in 决定保持原值。有效主文件仍完整保留本地启用状态，未来 schema、主文件超限等现有终止性错误仍禁止降级读取。
- 这项安全恢复不是新的用户决定，不单独增加 revision；首次目录发布仍提供新的进程内 generation。代价是状态文件损坏并从次级候选恢复后，用户需要重新明确启用本地插件。将备份手工复制成主文件属于当前 OS 用户主动修改配置，超出本文威胁模型。
- 沿用 `dirs::config_dir()/APP_NAME`，不切换到 bundle identifier 根：
- 目标与暂存槽位使用分别生成的 UUID v4 simple 字符串；不能来自 manifest ID、输入文件名或前端。槽位只在后端存活，不进入 IPC。`import-staging/` 与 `local/` 必须同卷，禁止跨卷复制降级。
- 一次成功 prepare 不写包、不创建 plugins 子目录、不更改状态或 revision；token 仅在内存保存。初始 get 的既有首扫不算安装行为。
- ready 会话占有 prepare admission；新的 prepare 返回 busy，不静默替换待确认内容。前端取消后才可重新选择。
- 不授予 `dialog:default`、`dialog:allow-open`、文件系统 scope、动态 command 或新事件频道。后端 dialog plugin 作为构建期 Tauri framework 依赖不属于用户应用插件，不因此扩张用户清单能力。
- 全部测试必须使用注入临时根与测试夹具。平台安全测试在对应 Windows、Linux、macOS CI 运行；不以单平台通过代替其他平台 no-replace 和 no-follow 验证。

Exact resource limits copied from the spec:

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

Exact confirmation copy:

> 发布者信息由清单作者填写，未经认证。本次只复制元数据，不运行代码或授予权限。导入后默认停用，启用仅记录宿主偏好。

---

## Workspace and execution rules

Run all commands from `D:\EasiFlux\EasiFlux-Desktop-Tauri\target\worktrees\plugin-local-manifest-import`; the branch is `plugin/local-manifest-import`, implementation baseline `2e519da9c35ab7ccdec4de2171cc81e429c7c437`. Do not switch/rebase this approved worktree or create a new branch while executing this plan. Before Task 1 capture `git status --short --branch` and preserve any unrelated edits. Stage only the exact files listed by the task; never `git add -A`.

Every RED command must run before production changes and show the stated assertion failure or missing new API. A build/environment failure unrelated to that test does not count as RED. After GREEN, run the task's regressions before its commit. Each task is a review boundary, not permission to implement subsequent tasks without their tests. The steps below give executable test kernels, exact interfaces, additional named cases and implementation logic; imports use the named files' existing imports unless shown.

Use `pnpm exec vue-tsc --noEmit`, not `pnpm typecheck` (there is no such script). Cargo commands retain `--locked` except the explicit dependency-resolution step when adding a dependency; inspect and commit that lockfile change with the owning task. Do not turn unrelated existing formatting or Clippy warnings into scope.

## File responsibility map and dependency order

| Responsibility | Files | First owner |
| --- | --- | --- |
| Secondary recovery safety | `src-tauri/src/storage/plugin_state.rs`, `storage/plugin_state/tests.rs` | Task 1 |
| Canonical content, DTOs, session leases | `src-tauri/src/plugin/record.rs`, `plugin/import.rs`, `plugin/import/session.rs`, `plugin/import/tests.rs` | Task 2 |
| Secure arbitrary selected-file read | `plugin/discovery/safe_fs.rs`, `plugin/import/source.rs`, `plugin/import/source/tests.rs` | Task 3 |
| Staging/promotion/owned cleanup | `src-tauri/src/storage/local_plugin_import.rs`, `storage/local_plugin_import/platform.rs`, `storage/local_plugin_import/tests.rs` | Task 4 |
| Scanner budgets, absent-identity disabled transaction | `plugin/discovery.rs`, `plugin/discovery/tests.rs`, `plugin/registry.rs`, `plugin/registry/tests.rs` | Task 5 |
| Serialized runtime mutations | `plugin/runtime.rs`, `plugin/runtime/tests.rs`, `commands/plugin.rs` helpers | Task 6 |
| End-to-end import orchestration | `plugin/runtime/import.rs`, `plugin/runtime/import/tests.rs` | Task 7 |
| Native selector/IPC/ACL | `plugin/import/dialog.rs`, `commands/plugin.rs`, `src-tauri/src/lib.rs`, `src-tauri/build.rs`, capabilities and generated permissions | Task 8 |
| Frontend unions/parsing | `src/types/plugin.ts`, `src/services/pluginService.ts`, `tests/frontend/pluginService.test.ts` | Task 9 |
| Frontend import session/arbitration | `src/stores/plugin.ts`, `tests/frontend/pluginStore.test.ts` | Task 10 |
| Accessible import UI and user guide | `src/components/plugins/PluginImportDialog.vue`, existing page/CSS, component tests, `docs/plugin-local-manifests.md` | Task 11 |
| Crash/platform regression and CI | `plugin/runtime/import/tests.rs`, storage tests, `.github/workflows/ci.yml` | Task 12 |

`src-tauri/src/plugin/mod.rs` and `storage/mod.rs` only declare new modules; do not reorganize unrelated modules. `Cargo.toml`/`Cargo.lock` changes belong to the first task actually needing the dependency. Tasks 1–5 supply primitives; Task 6 supplies operation ordering; Task 7 composes them; Task 8 exposes the backend; Tasks 9–11 expose the frontend; Task 12 completes cross-platform evidence.

### Task 1: Make secondary state recovery disable only local identities

**Files:**
- Modify: `src-tauri/src/storage/plugin_state.rs`
- Test: `src-tauri/src/storage/plugin_state/tests.rs`
- Test: `src-tauri/src/plugin/registry/tests.rs`

**Interfaces:**
- Consumes: existing `PluginStateStore::load_locked() -> AppResult<Option<PluginStateLoad>>`, `PluginStateLoad { state, requires_rewrite }`, `PluginSource`.
- Produces: unchanged interfaces; every valid non-primary recovery candidate has local `enabled=false`, unchanged revision and `requires_rewrite=true`.
- Test integration access: make existing `PluginStateStore::with_path(path: PathBuf) -> Self` `pub(crate)` so later same-crate temporary-root tests can construct the real store; it remains absent from IPC.

- [ ] **Step 1: Write the failing secondary-recovery test**

Append to the existing state test module, using its real `Fixture`, `state` and `local_entry` helpers:

```rust
#[test]
fn secondary_recovery_disables_local_but_preserves_builtin_and_revision() {
    for suffix in [".tmp", ".bak"] {
        let fixture = Fixture::new();
        let mut saved = state(u64::MAX);
        saved.entries[0].enabled = true;
        saved.entries.push(local_entry("com.easiflux.local", LOCAL_FINGERPRINT_A));
        fixture.write("", b"broken primary");
        fixture.write(suffix, serde_json::to_vec(&saved).unwrap());
        let loaded = fixture.store().load().unwrap();
        assert!(loaded.state.entries[0].enabled);
        assert!(!loaded.state.entries[1].enabled);
        assert_eq!(loaded.state.revision, u64::MAX);
        assert!(loaded.requires_rewrite);
        assert_eq!(fs::read(fixture.path()).unwrap(), b"broken primary");
    }
}
```

Add `primary_recovery_retains_local_enabled`, `secondary_with_already_disabled_locals_still_requires_rewrite`, and `future_or_oversized_primary_never_falls_back`; use the same document with primary `""` for the first, all local bools false for the second, and existing future/oversize fixtures for the third.

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml secondary_recovery_disables_local_but_preserves_builtin_and_revision --lib`

Expected: FAIL at the assertion requiring recovered local `enabled=false`.

- [ ] **Step 3: Implement candidate-index normalization after full validation**

In the `Ok(mut state)` decoded-candidate branch, before returning it:

```rust
if index != 0 {
    for entry in &mut state.state.entries {
        if entry.source == PluginSource::LocalDeclarative {
            entry.enabled = false;
        }
    }
    state.requires_rewrite = true;
}
return Ok(Some(state));
```

Do not move normalization before candidate schema/size validation, write during load, increment revision, disable built-ins or weaken future-schema termination. The same normalized document must be used as the store's `previous` backup when a later save begins.

- [ ] **Step 4: Add a preservation/characterization registry rewrite-failure test**

Use existing registry `MemoryPersistence`, `manifest` and `PluginRecord`; no new fake domain behavior:

```rust
#[test]
fn recovered_disabled_identity_rewrites_at_max_revision_after_save_failure() {
    let local = PluginRecord::local_declarative(manifest("com.easiflux.local")).unwrap();
    let identity = local.identity();
    let persisted = PluginStateFileV2 {
        schema_version: 2,
        revision: u64::MAX,
        entries: vec![PluginStateEntryV2 {
            id: identity.id, source: identity.source,
            publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint, enabled: false,
        }],
    };
    let memory = MemoryPersistence::failing_loaded_from_v1(persisted.clone());
    let mut registry = memory.registry(vec![]);
    registry.apply_local_discovery(LocalDiscoveryOutcome::available(vec![local])).unwrap();
    assert!(registry.set_enabled("com.easiflux.local", false, "1").is_err());
    assert_eq!(registry.catalog_snapshot().revision, u64::MAX.to_string());
    memory.allow_saves();
    registry.set_enabled("com.easiflux.local", false, "1").unwrap();
    assert_eq!(memory.last_save().unwrap(), persisted);
}
```

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml recovered_disabled_identity_rewrites_at_max_revision_after_save_failure --lib`

Expected: PASS if existing rewrite transaction is already correct; this is a preservation test, not a claimed RED. If it fails, record the failure and repair only the clone/persist/commit rewrite-marker handling before continuing.

- [ ] **Step 5: Verify GREEN and regression**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
```

Expected: both pass, including valid-primary local authorization and rewrite-only at `u64::MAX`.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/storage/plugin_state.rs src-tauri/src/storage/plugin_state/tests.rs src-tauri/src/plugin/registry/tests.rs
git commit -m "fix(plugin): fail closed when recovering local plugin state"
```

### Task 2: Define canonical import content, wire results and one-time session leases

**Files:**
- Modify: `src-tauri/src/plugin/record.rs`
- Modify: `src-tauri/src/plugin/mod.rs`
- Create: `src-tauri/src/plugin/import.rs`
- Create: `src-tauri/src/plugin/import/session.rs`
- Create: `src-tauri/src/plugin/import/tests.rs`

**Interfaces:**
- Consumes: `PluginManifestV1`, `PluginRecord`, `PluginCatalogSnapshot`, `AppResult`, `Instant`.
- Produces: `PluginRecord::canonical_manifest_bytes(&self) -> Result<Vec<u8>, String>`; `PreparedManifest: Clone` with `parse(bytes: &[u8]) -> AppResult<PreparedManifest>`, private record/bytes and accessors `record() -> &PluginRecord`, `bytes() -> &[u8]`.
- Produces: `ImportSessions::new() -> Arc<ImportSessions>`, `reserve_prepare(self: &Arc<Self>, now: Instant) -> AppResult<PrepareLease>`, `PrepareLease::publish(self, content: PreparedManifest, generation: u64, now: Instant) -> AppResult<ImportPreview>`, `claim_commit(self: &Arc<Self>, token: &str, now: Instant) -> AppResult<CommitLease>`, `cancel(&self, token: &str, now: Instant) -> AppResult<()>`.
- Produces: `CommitLease::{content() -> &PreparedManifest, generation() -> u64}`; lease Drop releases only its matching session owner. `ImportPreview` serializes the exact ready branch from spec §6, including `schemaVersion=1`, `status=ready`, token, `expiresInSeconds=300`, canonical generation and manifest.
- Produces: `PrepareImportResult`, `CancelImportResult`, `CommitImportResult`, `ImportCommitFailure` with precisely the spec §6/§10 variants; `ImportCommitFailure::as_code() -> &'static str`. No path-bearing type implements Serialize or Debug.
- Produces: `SelectedManifestSource::{Cancelled, Selected(PathBuf)}` and `LocalManifestSelector: Send + Sync` with `select(&self) -> Pin<Box<dyn Future<Output=AppResult<SelectedManifestSource>> + Send + '_>>`, a domain trait implemented by Task 8 and faked by Task 7.

- [ ] **Step 1: Write RED canonical-content and expiry tests**

In `import.rs`, expose only under `#[cfg(test)]` a `pub(crate) mod test_support` containing this fixture; all later Rust import tests reuse it:

```rust
pub(crate) const VALID: &[u8] = br#"{"schemaVersion":1,"id":"com.example.notes","publisherId":"com.example","publisher":"Example","name":"Notes","description":"Metadata only","version":"1.0.0","contributions":[],"requestedCapabilities":[]}"#;
```

In `import/tests.rs`:

```rust
#[test]
fn token_is_one_time_and_expires_at_exactly_300_seconds() {
    let start = std::time::Instant::now();
    let sessions = ImportSessions::new();
    let preview = sessions.reserve_prepare(start).unwrap()
        .publish(PreparedManifest::parse(test_support::VALID).unwrap(), 4, start).unwrap();
    assert_eq!(preview.token.len(), 32);
    assert!(sessions.reserve_prepare(start).is_err());
    let lease = sessions.claim_commit(&preview.token, start + Duration::from_secs(299)).unwrap();
    assert_eq!(lease.generation(), 4);
    assert!(sessions.claim_commit(&preview.token, start).is_err());
    assert!(sessions.cancel(&preview.token, start).is_err());
    drop(lease);
    let second = sessions.reserve_prepare(start).unwrap()
        .publish(PreparedManifest::parse(test_support::VALID).unwrap(), 4, start).unwrap();
    assert!(sessions.claim_commit(&second.token, start + Duration::from_secs(300)).is_err());
    assert!(sessions.reserve_prepare(start + Duration::from_secs(300)).is_ok());
}
```

Add named tests `canonical_bytes_match_fingerprint_input`, `semantic_change_changes_identity`, `invalid_utf8_bom_duplicate_fields_and_nonempty_contributions_are_rejected`, `cancel_unknown_is_idempotent_without_cancelling_current_ready`, `dropping_prepare_lease_releases_admission`, `dropping_old_owner_cannot_release_new_session`, `import_result_wire_keys_match_spec`. Assert error code equality, not only `is_err`, in each case table. Test direct `PreparedManifest::parse` for >16,384 bytes as defense in depth even though the source reader will bound input.

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::tests --lib`

Expected: missing `PreparedManifest` / `ImportSessions` until production definitions are added; do not count an unrelated build failure.

- [ ] **Step 3: Implement content without changing the fingerprint**

Reuse `CanonicalManifestV1` from `record.rs` for both serialized installation bytes and hashing. The bounded parse kernel is:

```rust
pub(crate) fn parse(bytes: &[u8]) -> AppResult<Self> {
    if bytes.len() > 16_384 || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(invalid_manifest());
    }
    let manifest: PluginManifestV1 = serde_json::from_slice(bytes).map_err(|_| invalid_manifest())?;
    let record = PluginRecord::local_declarative(manifest).map_err(|_| invalid_manifest())?;
    let bytes = record.canonical_manifest_bytes().map_err(|_| invalid_manifest())?;
    if bytes.len() > 16_384 { return Err(invalid_manifest()); }
    Ok(Self { record, bytes })
}
```

Define `invalid_manifest() -> AppError` locally with code `plugin_import_manifest_invalid`, diagnostic None. `from_slice::<PluginManifestV1>` preserves map-only duplicate-field rejection; never intermediate-parse to Value. Keep the existing domain string and frozen canonical field order byte-for-byte.

- [ ] **Step 4: Implement session state and exact wire constructors**

Use a short `std::sync::Mutex<SessionState>`; do not hold it across I/O. Define:

```rust
enum SessionState {
    Idle,
    Preparing { owner: uuid::Uuid },
    Ready { owner: uuid::Uuid, token: String, expires_at: Instant,
            generation: u64, content: PreparedManifest },
    Committing { owner: uuid::Uuid },
}
```

Generate unrelated owner and token UUIDs; only the lowercase simple token serializes. At each entry expire Ready when `now >= expires_at`. `claim_commit` validates token syntax, expires, then atomically moves content to a `CommitLease` and installs Committing. Unknown tokens do not consume the active Ready. `cancel` is harmless for unknown/expired Ready tokens and returns busy for Committing. Drop compares owner before clearing; neither lease destructor writes disk. Constructors embed schema 1/status discriminants; invalid schema values cannot be caller parameters. Define all spec commit-failure variants now so storage/runtime/TS share exact names.

- [ ] **Step 5: Verify GREEN**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::record::tests --lib
```

Expected: all session/canonical/wire tests pass; existing fingerprint golden behavior is unchanged.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/mod.rs src-tauri/src/plugin/record.rs src-tauri/src/plugin/import.rs src-tauri/src/plugin/import/session.rs src-tauri/src/plugin/import/tests.rs
git commit -m "feat(plugin): define single-use manifest import sessions"
```

### Task 3: Read the selected source through bounded no-follow handles

**Files:**
- Modify: `src-tauri/src/plugin/discovery/safe_fs.rs`
- Modify: `src-tauri/src/plugin/discovery.rs` (module visibility only)
- Modify: `src-tauri/src/plugin/import.rs`
- Create: `src-tauri/src/plugin/import/source.rs`
- Create: `src-tauri/src/plugin/import/source/tests.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (test-only tempfile)

**Interfaces:**
- Consumes: `PreparedManifest::parse`, current platform `Directory`/bounded read primitives.
- Produces: `LocalManifestReader: Send + Sync` with `read(&self, path: &Path) -> AppResult<PreparedManifest>`; `SystemLocalManifestReader` implementation.
- Produces: `safe_fs::read_manifest_source(path: &Path) -> Result<Vec<u8>, SourceReadError>` where `SourceReadError` is an opaque unit-like error, no path/OS details.
- Existing `read_package_candidates` behavior and no-follow flags remain unchanged.

- [ ] **Step 1: Write RED secure-source tests**

Add `tempfile = "3"` under dev-dependencies, resolve the lockfile using `cargo check --manifest-path src-tauri/Cargo.toml --tests`, and inspect only that dependency diff. In source tests:

```rust
#[test]
fn captures_selected_content_without_reopening_the_source() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("user-chosen-name.json");
    std::fs::write(&source, crate::plugin::import::test_support::VALID).unwrap();
    let captured = SystemLocalManifestReader.read(&source).unwrap();
    std::fs::write(&source, b"now invalid").unwrap();
    let reread: PluginManifestV1 = serde_json::from_slice(captured.bytes()).unwrap();
    assert_eq!(reread.id.as_str(), "com.example.notes");
}

#[test]
fn rejects_an_external_hard_link() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let first = root.join("a.json");
    let second = root.join("b.json");
    std::fs::write(&first, crate::plugin::import::test_support::VALID).unwrap();
    std::fs::hard_link(&first, &second).unwrap();
    assert!(SystemLocalManifestReader.read(&second).is_err());
}
```

Add parameterized `source_limit_accepts_16384_rejects_16385` (pad VALID with JSON whitespace), `relative_and_parent_paths_rejected`, `source_directory_rejected`; Unix `fifo_without_writer_does_not_block`, source/ancestor symlink; Windows source/ancestor junction/reparse, UNC/device/ADS, normal verbatim disk path accepted. Canonicalize the owned test root to avoid macOS `/var` fixture symlinks; do not silently canonicalize the production selected path.

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::source::tests --lib`

Expected: missing source-reader API before implementation.

- [ ] **Step 3: Implement safe file-source reading**

Extract a named-file opener from the existing directory adapter; retain `open_manifest` as `open_regular_file(OsStr::new("manifest.json"))`. Expose only:

```rust
pub(crate) fn read_manifest_source(path: &Path) -> Result<Vec<u8>, SourceReadError>;
```

Inside it reject non-absolute paths and parent/current components, split parent/file name without normalization, open the entire parent chain through the existing no-follow algorithm, then open the final component and verify a single-link ordinary file from the handle. Windows reject colons in Normal components (drive-prefix colon is not a Normal component), reject non-Disk prefixes; preserve its held ancestor handles. Unix retain `NONBLOCK` before `fstat`. Read at most 16,385 bytes with the current bounded loop, map any unsafe shape/overlimit/I/O to source_rejected, and drop every handle before returning bytes.

The reader wrapper is concrete:

```rust
impl LocalManifestReader for SystemLocalManifestReader {
    fn read(&self, path: &Path) -> AppResult<PreparedManifest> {
        let bytes = safe_fs::read_manifest_source(path).map_err(|_| source_rejected())?;
        PreparedManifest::parse(&bytes)
    }
}
```

Define `source_rejected() -> AppError` with fixed spec copy and diagnostic None. No log interpolation of `path`, IO error or bytes.

- [ ] **Step 4: Verify GREEN and existing discovery safety**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::source::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
```

Expected: pass on the current platform; Task 12 runs corresponding platform-only cases in all three CI environments.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/plugin/import.rs src-tauri/src/plugin/import/source.rs src-tauri/src/plugin/import/source/tests.rs src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/safe_fs.rs
git commit -m "feat(plugin): read selected manifests with bounded safe handles"
```

### Task 4: Stage exact bytes and promote without replacing any target

**Files:**
- Modify: `src-tauri/src/storage/mod.rs`
- Create: `src-tauri/src/storage/local_plugin_import.rs`
- Create: `src-tauri/src/storage/local_plugin_import/platform.rs`
- Create: `src-tauri/src/storage/local_plugin_import/tests.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (macOS libc / necessary Windows API feature)

**Interfaces:**
- Consumes: canonical bytes, `ImportCommitFailure`.
- Produces: object-safe `LocalManifestImportStorage: Send + Sync` with `prepare_stage(&self, bytes: &[u8]) -> Result<Box<dyn OwnedImportStage>, ImportCommitFailure>`.
- Produces: `OwnedImportStage: Send` with `promote(&mut self) -> Result<Promotion, ImportCommitFailure>` and `cleanup(&mut self) -> Result<(), ImportCommitFailure>`; `Promotion { pub(crate) object_identity_verified: bool }`.
- Produces: `SystemLocalManifestImportStorage::new() -> Self` (lazy system-root resolver), `with_plugins_root(root: PathBuf) -> Self` for injected tests. Constructors perform no filesystem writes.
- Produces: internal `ImportFsStep::{CreateStage, WriteManifest, SyncManifest, SyncStageDirectory, SyncStagingParent, BeforePromotion, AfterPromotion, SyncDestinationDirectory}`; optional test hook `Arc<dyn Fn(ImportFsStep) -> io::Result<()> + Send + Sync>` and deterministic UUID supplier for collision tests. Hooks are test-only; production uses UUID v4.

- [ ] **Step 1: Write RED filesystem contract tests**

```rust
#[test]
fn stage_is_invisible_until_exclusive_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap().join("plugins");
    let storage = SystemLocalManifestImportStorage::with_plugins_root(root.clone());
    assert!(!root.exists());
    let mut stage = storage.prepare_stage(crate::plugin::import::test_support::VALID).unwrap();
    assert_eq!(std::fs::read_dir(root.join("local")).unwrap().count(), 0);
    let promoted = stage.promote().unwrap();
    assert!(promoted.object_identity_verified);
    let target = std::fs::read_dir(root.join("local")).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(target.join("manifest.json")).unwrap(),
               crate::plugin::import::test_support::VALID);
    assert!(stage.cleanup().is_err());
    assert!(target.exists());
}
```

Additional test kernels use exact injection outcomes:

| Test name | Injection | Required assertion |
| --- | --- | --- |
| `stage_cap_counts_unknown_entries` | 16 files in staging root | staging_capacity_exceeded, existing bytes unchanged |
| `four_target_collisions_never_replace` | supplier returns four occupied pkg IDs | write_failed, no target modified, exactly four attempts |
| `four_stage_collisions_are_bounded` | four occupied stage IDs | no fifth creation attempt |
| `prepromotion_failure_preserves_stage_and_no_target` | BeforePromotion → PermissionDenied | Err, local empty, safe cleanup succeeds |
| `unix_staging_parent_sync_failure_is_prepared_error` | SyncStagingParent → PermissionDenied (`cfg(unix)`) | prepare_stage Err, no target; owned cleanup rules preserved |
| `postpromotion_sync_error_is_committed` | SyncDestinationDirectory → PermissionDenied | Ok(Promotion), local contains full manifest |
| `cleanup_refuses_unknown_file_or_replaced_identity` | add second file or replace directory identity | Err, preserve all unknown bytes |
| `restart_never_cleans_old_stages` | drop instance; create another | old stage unchanged and counted |
| `cross_volume_or_unsupported_exclusive_rename_has_no_copy_fallback` | adapter returns EXDEV/unsupported | Err, no copied target |

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import::tests --lib`

Expected: missing import-storage module/types before implementation.

- [ ] **Step 3: Implement no-follow fixed directories and owned staging**

Keep this ownership shape private, with platform handles and file IDs rather than raw path ownership claims:

```rust
enum StageState { Prepared, Promoted, Cleaned }
struct FileIdentity { volume: u64, object: u128 }
struct DiskStage {
    parents: platform::ImportDirectories,
    stage_name: String,
    stage_identity: FileIdentity,
    manifest_identity: FileIdentity,
    state: StageState,
}
```

`platform::ImportDirectories` owns fixed plugins/staging/local parent handles. Implement platform methods `open_or_create(root: &Path) -> io::Result<Self>`, `staging_count(limit: usize) -> io::Result<usize>`, `create_stage(name: &str, bytes: &[u8]) -> io::Result<(FileIdentity, FileIdentity)>`, `promote_exclusive(stage: &str, target: &str) -> io::Result<()>`, `verify_stage(name: &str, directory: &FileIdentity, manifest: &FileIdentity) -> io::Result<()>`, `verify_target(name: &str, directory: &FileIdentity, manifest: &FileIdentity) -> io::Result<()>`, `cleanup_stage(name: &str, directory: &FileIdentity, manifest: &FileIdentity) -> io::Result<()>`, `sync_after_promotion() -> io::Result<()>`.

Open/create parents one level at a time; existing links/reparse points fail. Count staging entries only up to 17. Exclusively create a UUID stage and a single `manifest.json`; open file create-new, write bytes and file sync. On Unix, sync the stage directory and then its held `import-staging` parent before returning the prepared stage; expose distinct `SyncStageDirectory` and `SyncStagingParent` checkpoints, and treat either failure as a pre-disabled error. No recursive cleanup, no deletion of old stages, no ordinary overwrite API. A partial prepare failure cleans only exact objects it successfully created; if safety verification fails, preserve the stage.

- [ ] **Step 4: Implement the three exclusive-rename adapters**

The commit-point kernel in `DiskStage::promote` is:

```rust
self.parents.verify_stage(&self.stage_name, &self.stage_identity, &self.manifest_identity)
    .map_err(|_| ImportCommitFailure::ImportWriteFailed)?;
let mut promoted_target = None;
for _ in 0..4 {
    let target = format!("pkg-{}", uuid::Uuid::new_v4().simple());
    match self.parents.promote_exclusive(&self.stage_name, &target) {
        Ok(()) => { promoted_target = Some(target); break; }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
        Err(_) => return Err(ImportCommitFailure::ImportWriteFailed),
    }
}
let target = promoted_target.ok_or(ImportCommitFailure::ImportWriteFailed)?;
self.state = StageState::Promoted;
let verified = self.parents.verify_target(&target, &self.stage_identity, &self.manifest_identity).is_ok();
let _ = self.parents.sync_after_promotion();
Ok(Promotion { object_identity_verified: verified })
```

Route the UUID expression through the test-only deterministic supplier when running collision tests. Retry only target-exists; any other precommit IO fails immediately. Postcommit validation/sync failure must never return the precommit Err branch. Log only a fixed message and error kind.

Linux uses held dirfds and `renameat2(RENAME_NOREPLACE)` through rustix; macOS uses held dirfds and `libc::renameatx_np(..., libc::RENAME_EXCL)`; Windows uses `MoveFileExW(..., 0)` without COPY_ALLOWED/REPLACE_EXISTING, held ancestor handles, and releases its stage/file no-delete handles before move. Validate UTF-16 arguments against interior NUL; paths are backend-generated under pinned ancestors. Fail if the filesystem rejects the exclusive operation; do not fallback to copy. Update target-specific Cargo requirements and resolve/review lockfile changes now.

- [ ] **Step 5: Verify GREEN**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
```

Expected: all current-platform cases pass, including real no-replace target preservation; hooks are not substitutes for that real filesystem test.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/storage/mod.rs src-tauri/src/storage/local_plugin_import.rs src-tauri/src/storage/local_plugin_import/platform.rs src-tauri/src/storage/local_plugin_import/tests.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(plugin): stage and exclusively promote local manifests"
```

### Task 5: Expose scan budgets and persist disabled for an undiscovered identity

**Files:**
- Modify: `src-tauri/src/plugin/discovery.rs`
- Modify: `src-tauri/src/plugin/discovery/safe_fs.rs`
- Test: `src-tauri/src/plugin/discovery/tests.rs`
- Test: `src-tauri/src/plugin/discovery/safe_fs/tests.rs`
- Modify: `src-tauri/src/plugin/registry.rs`
- Test: `src-tauri/src/plugin/registry/tests.rs`

**Interfaces:**
- Consumes: `PluginRecord`, `PluginStatePersistence`, `LocalDiscoveryOutcome`.
- Produces: `ScanUsage { root_entries: usize, packages: usize, bytes_read: usize }`, `ScanUsage::can_add_manifest(bytes: usize) -> bool`, new internal `LocalDiscoveryOutcome.usage: Option<ScanUsage>`; unavailable has None. Healthy production scan always has Some, including missing root with all zeroes. Usage does not affect generation equality or IPC.
- Produces: `PluginRegistry::validate_import(&self, record: &PluginRecord, usage: ScanUsage) -> Result<(), ImportCommitFailure>` (available state, healthy local discovery, all-source ID conflict, capacity and generation headroom).
- Produces: `PluginRegistry::persist_import_disabled(&mut self, record: &PluginRecord) -> AppResult<()>` and `PluginRegistry::state_requires_retry(&self) -> bool`; the former rejects nonlocal record, cleans same ID/source, persists before memory commit and never publishes catalog membership.
- Test integration access: add `#[cfg(test)] pub(crate) fn discover_from_root(root: &Path) -> LocalDiscoveryOutcome`, delegating to existing `discover_with_root(|| Some(root.to_path_buf()))`, for later real-disk runtime fixtures. Production `SystemLocalPluginDiscovery` continues to resolve only its fixed root.

- [ ] **Step 1: Write RED registry and budget tests**

```rust
#[test]
fn import_disabled_replaces_orphan_authorization_without_adding_catalog_item() {
    let local = PluginRecord::local_declarative(manifest("com.easiflux.local")).unwrap();
    let identity = local.identity();
    let memory = MemoryPersistence::new(PluginStateFileV2 {
        schema_version: 2, revision: 7,
        entries: vec![PluginStateEntryV2 {
            id: identity.id, source: identity.source, publisher_id: identity.publisher_id,
            approval_fingerprint: identity.approval_fingerprint, enabled: true,
        }],
    });
    let mut registry = memory.registry(vec![]);
    registry.persist_import_disabled(&local).unwrap();
    assert!(registry.catalog_snapshot().plugins.is_empty());
    let saved = memory.last_save().unwrap();
    assert_eq!(saved.revision, 8);
    assert!(!saved.entries[0].enabled);
}
```

In discovery tests assert `ScanUsage { root_entries:255, packages:127, bytes_read:2_097_151 }.can_add_manifest(1)` is true; change each field individually to 256, 128 or add 2 bytes and assert false. Assert malformed packages' consumed probe bytes remain counted. Add `budget_only_change_does_not_increment_catalog_generation` and `unavailable_never_reports_zero_usage_as_authoritative`.

- [ ] **Step 2: Verify RED**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml import_disabled_replaces_orphan_authorization_without_adding_catalog_item --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml budget_only_change_does_not_increment_catalog_generation --lib
```

Expected: missing new registry/budget interfaces.

- [ ] **Step 3: Implement bounded usage and the absent-record decision**

Add usage at the exact scan counters already maintained by safe_fs; do not recompute bytes from only accepted records. Carry it through strict parsing to the internal outcome. Existing fixture constructors `available(records)` / `degraded(records,count)` synthesize consistent fixture usage; use explicit usage in boundary tests. Registry's existing equality for records plus public summary must not start comparing usage.

```rust
impl ScanUsage {
    pub(crate) fn can_add_manifest(self, bytes: usize) -> bool {
        self.root_entries < 256 && self.packages < 128
            && bytes <= 16_384
            && self.bytes_read.checked_add(bytes).is_some_and(|n| n <= 2_097_152)
    }
}
```

Extract the existing set_enabled clone/remove-old-identities/push/sort/validate/persist/commit body into a private `persist_decision(&mut self, record: &PluginRecord, enabled: bool) -> AppResult<()>`. Both the existing catalog-bound set_enabled and new persist_import_disabled use it. Public set_enabled retains its ID/generation/catalog lookup validation; the internal import method receives only a validated local record, fixes enabled=false, and does not expose an absent-ID IPC. Preserve capacity-before-revision-exhaustion ordering, canonical no-op, rewrite marker and exact rollback behavior.

- [ ] **Step 4: Verify GREEN including rejection invariants**

Add and run explicit tests for: historical publisher/fingerprint cleanup; 513th retained identity rejection; save failure leaves all old identities and revision; same disabled no-op; same disabled with rewrite flag persists at MAX; builtin record cannot use import method; ID conflict includes builtin and current local; unhealthy local discovery fails validation; MAX generation rejected and MAX-1 accepted.

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
```

Expected: all pass and existing catalog transport JSON is unchanged.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugin/discovery.rs src-tauri/src/plugin/discovery/safe_fs.rs src-tauri/src/plugin/discovery/tests.rs src-tauri/src/plugin/discovery/safe_fs/tests.rs src-tauri/src/plugin/registry.rs src-tauri/src/plugin/registry/tests.rs
git commit -m "feat(plugin): validate import budgets and persist disabled identities"
```

### Task 6: Serialize every plugin mutation through cancellation-safe FIFO ownership

**Files:**
- Modify: `src-tauri/src/plugin/runtime.rs`
- Test: `src-tauri/src/plugin/runtime/tests.rs`
- Modify: `src-tauri/src/commands/plugin.rs` (Arc helper signatures/tests only)

**Interfaces:**
- Consumes: existing owned FIFO reservation and `PluginRegistry::state_requires_retry`.
- Produces: runtime `operation_gate: Arc<tokio::sync::Mutex<()>>`; `PluginRuntime::set_enabled(self: &Arc<Self>, id: &str, enabled: bool, expected_catalog_generation: &str) -> AppResult<PluginCatalogMutationResult>`.
- Produces: private `reserve_operation(&self) -> OperationReservation` and `OperationReservation::enter(self) -> Pin<Box<dyn Future<Output=OwnedMutexGuard<()>> + Send>>`; reservation registers FIFO position synchronously on first poll before spawning the owning worker. Task 7 uses this helper, not a second queue.
- Produces: read-only normal get after initialization; unavailable-state retry enters gate and loads from a blocking worker.

- [ ] **Step 1: Add RED operation-order tests**

Extend existing `ControlledDiscovery` / `ScanControl`, not a sleep-based fake. After a runtime with initial discovery complete, start a blocked reload, accept set_enabled behind it, and verify no state save occurs until releasing the scan. Then drop the caller JoinHandle via `abort()` and release the scan; assert the accepted toggle still commits once. The new exact tests are `toggle_waits_behind_accepted_reload`, `cancelled_toggle_caller_does_not_drop_reserved_operation`, `ordinary_get_reads_while_operation_gate_is_held`.

Add this complete test with the existing runtime MemoryPersistence and ControlledDiscovery fixtures:

```rust
#[tokio::test]
async fn toggle_waits_behind_accepted_reload() {
    let persistence = MemoryPersistence::default();
    let (discovery, mut scans) = ControlledDiscovery::new(vec![local("1.0.0"), local("1.0.0")]);
    let runtime = runtime_with(discovery, &persistence);
    let initial = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.get_catalog().await }
    });
    scans[0].wait_started().await;
    scans[0].release();
    initial.await.unwrap().unwrap();
    let reload = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.reload_catalog().await }
    });
    scans[1].wait_started().await;
    let before = persistence.0.lock().unwrap().saves;
    {
        let toggle = runtime.set_enabled("com.example.alpha", true, "1");
        tokio::pin!(toggle);
        assert!(futures_util::poll!(&mut toggle).is_pending());
        assert_eq!(persistence.0.lock().unwrap().saves, before);
    }
    scans[1].release();
    reload.await.unwrap().unwrap();
    let _after_toggle = runtime.operation_gate.lock().await;
    assert_eq!(persistence.0.lock().unwrap().saves, before + 1);
}
```

The direct first poll registers the owned operation before the scoped caller future is dropped; no guessed yield count is used. The final gate acquisition queues behind that toggle and proves it completed once. For RED, temporarily use the existing `reload_gate` field in this assertion; rename the test field alongside the production gate in Step 3 so the RED is the early-write assertion rather than a missing field.

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml toggle_waits_behind_accepted_reload --lib`

Expected: current direct set_enabled writes while reload is blocked; assertion fails.

- [ ] **Step 3: Implement shared reservation and blocking state mutation**

Rename reload_gate to operation_gate. Extract its current poll-before-spawn owned waiter code into `OperationReservation`, retaining the pinned pending waiter and ready owned guard cases. The synchronous reserve helper polls the pinned `tokio::task::unconstrained(lock_owned())` once using `Context::from_waker(futures_util::task::noop_waker_ref())`; a Pending future is retained and later awaited, which installs the owned task's live waker without forfeiting FIFO position. Each public mutating future owns copied arguments and transfers work to `tokio::spawn` before awaiting. Inside the owned task, enter the reservation, then execute synchronous state work with this ownership pattern:

```rust
let owned_runtime = Arc::clone(&runtime);
let result = tokio::task::spawn_blocking(move || {
    owned_runtime.registry.blocking_write()
        .set_enabled(&id, enabled, &expected_catalog_generation)
}).await.map_err(|_| runtime_error())?;
```

The registry lock is acquired and released inside the blocking closure, never passed across await. Discovery keeps scanning without a registry lock. Normal get returns read-guard snapshot when initialized and retry is unnecessary; retry checks again under gate and uses blocking_write/retry_state_load. Adapt command helper `set_plugin_enabled_from` to accept `&Arc<PluginRuntime>`; keep the public Tauri command payload unchanged. Preserve all existing cancelled reload FIFO tests, adjusting only tests that intentionally depended on toggles bypassing reload because the spec now forbids that behavior.

- [ ] **Step 4: Verify GREEN**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
```

Expected: FIFO and cancellation tests pass; ordinary get can inspect a confirmed snapshot while a scan or import gate is held.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/tests.rs src-tauri/src/commands/plugin.rs
git commit -m "refactor(plugin): serialize state mutations with catalog operations"
```

### Task 7: Compose prepare and fail-closed commit with authoritative publication

**Files:**
- Modify: `src-tauri/src/plugin/runtime.rs`
- Create: `src-tauri/src/plugin/runtime/import.rs`
- Create: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify: `src-tauri/src/plugin/import.rs` (module exports only)

**Interfaces:**
- Consumes: Tasks 2–6 interfaces.
- Produces: `PluginRuntime::prepare_import(self: &Arc<Self>, selector: Arc<dyn LocalManifestSelector>) -> AppResult<PrepareImportResult>`, `cancel_import(&self, token: &str) -> AppResult<CancelImportResult>`, `commit_import(self: &Arc<Self>, token: &str, expected_catalog_generation: &str) -> AppResult<CommitImportResult>`.
- Produces: `PluginRuntime::with_import_services(registry: PluginRegistry, discovery: Arc<dyn LocalPluginDiscovery>, reader: Arc<dyn LocalManifestReader>, storage: Arc<dyn LocalManifestImportStorage>) -> Self`; existing initialize supplies lazy System reader/storage and new sessions without touching disk.
- Produces: a test-only compile-time assertion `fn assert_send_sync<T: Send + Sync>() {}` instantiated for `PluginRuntime`, plus an owned `tokio::spawn` smoke that moves `Arc<PluginRuntime>` through prepare/commit. This locks in the Tasks 2–4 service-trait bounds before native IPC is added.

- [ ] **Step 1: Write RED prepare/commit tests using controlled boundaries**

Define test-only `FixedSelector(Option<PathBuf>)` implementing Task 2's selector, returning Cancelled for None and Selected(clone) otherwise. Define `RecordingStorage` implementing Task 4's trait as a logging wrapper around the real `SystemLocalManifestImportStorage`; its owned-stage wrapper delegates to the real disk stage after recording each call, so post-import discovery sees an actual promoted package. Share `Arc<std::sync::Mutex<Vec<&'static str>>>` events with a logging wrapper around the real `PluginStateStore`. Stage methods append `stage`, `promote`, `cleanup`; persistence save appends `disable`; the real-root discovery adapter appends `scan` then calls `discover_from_root`. The registry and final DTO remain production behavior. Inject failures only at those I/O boundaries.

```rust
#[tokio::test]
async fn commit_orders_disable_before_promotion_and_publishes_disabled_content() {
    let fixture = ImportFixture::new().await;
    let preview = fixture.prepare_valid().await;
    fixture.events.lock().unwrap().clear();
    let result = fixture.runtime.commit_import(&preview.token, &preview.catalog_generation).await.unwrap();
    let wire = serde_json::to_value(result).unwrap();
    assert_eq!(wire["status"], "imported");
    assert_eq!(wire["snapshot"]["plugins"][0]["status"], "disabled");
    assert_eq!(*fixture.events.lock().unwrap(), ["scan", "stage", "disable", "promote", "scan"]);
}
```

`ImportFixture` is a Task 7 test helper, defined here with `runtime: Arc<PluginRuntime>`, `events`, owned TempDir and source PathBuf; `async fn new() -> Self` performs the initial empty get; `async fn prepare_valid(&self) -> ImportPreview` calls real prepare with FixedSelector and destructures Ready. Use real `SystemLocalManifestReader`; VALID is the Task 2 fixture. Its real-root discovery adapter calls Task 5's `discover_from_root`, and its real state adapter uses Task 1's crate-visible `PluginStateStore::with_path`. Define all helper constructors in this test module, never in production.

Required focused tests: `runtime_and_import_services_are_send_sync`, `owned_runtime_import_future_can_be_spawned`, `prepare_cancellation_has_no_filesystem_writes`, `source_replaced_after_preview_does_not_change_import`, `stale_confirmation_is_consumed_without_stage`, `preflight_change_is_published_then_rejected`, `duplicate_identity_is_never_overwritten`, `state_failure_prevents_promotion`, `staging_parent_sync_failure_never_saves_state` (`cfg(unix)`), `promotion_failure_reports_disabled_saved`, `postscan_failure_reports_imported_not_visible`, `unrelated_degraded_postscan_still_allows_exact_imported_item`, `lost_commit_caller_still_finishes_once`, `expired_or_replayed_token_never_writes`, `toggle_cannot_enter_disable_promotion_window`, `generation_max_rejects_before_stage`.

- [ ] **Step 2: Verify RED**

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib`

Expected: missing runtime import methods before implementation.

- [ ] **Step 3: Implement prepare/cancel without holding the operation gate for user input**

Prepare reserves the session first, awaits existing get, checks top/local available, invokes selector, returns Cancelled on None, reads once in spawn_blocking, checks availability again and publishes the ready lease with current generation/Instant. If the caller disappears while selecting, an owned task retains the PrepareLease until callback completion; failed delivery releases or expires the ready lease rather than freeing admission while a picker is still open. Do not serialize source PathBuf into any response or error.

Cancel maps session `cancel(token, Instant::now())` to exact cancelled envelope; it never calls storage.

- [ ] **Step 4: Implement the complete commit sequence**

Claim the token synchronously within the running command future, reserve FIFO position, and spawn the owning task without an intervening await. Move CommitLease into that task. Compare canonical requested generation to both lease generation and registry generation; mismatch consumes the token and returns NotImported/false.

Follow this executable ordering contract; each named call resolves to an interface from earlier tasks:

```text
operation.enter
compare generation
discovery.discover in blocking worker
registry.apply_local_discovery; compare generation again
registry.validate_import(content.record, usage)
storage.prepare_stage(content.bytes) in blocking worker
registry.persist_import_disabled(content.record) in blocking worker
stage.promote in blocking worker
discovery.discover in blocking worker
registry.apply_local_discovery; registry.catalog_snapshot
classify exact identity and disabled status
drop CommitLease and gate
```

Translate this sequence into small private async methods in `runtime/import.rs`: `scan_for_import(&Arc<PluginRuntime>) -> LocalDiscoveryOutcome`, `snapshot_after_failure(&Arc<PluginRuntime>, ImportCommitFailure, bool) -> CommitImportResult`, `publish_import_outcome(&Arc<PluginRuntime>, LocalDiscoveryOutcome) -> AppResult<PluginCatalogSnapshot>`, `classify_import_result(record: &PluginRecord, promotion: Promotion, snapshot: PluginCatalogSnapshot) -> CommitImportResult`. Use fixed paths only through storage/discovery services.

Before disabled success, cleanup the owned stage on error and return false. After disabled success, precommit promotion failure returns true; never restore enabled. After successful rename, any postcheck/postscan failure returns ImportedNotVisible, never NotImported or cleanup. A scan-worker panic maps to LocalDiscoveryOutcome::unavailable and still publishes. Unexpected failures unable to form a legal snapshot return sanitized AppError; clients treat these as unknown. Generation headroom validation occurs before stage, and the held gate prevents another increment before final publication.

- [ ] **Step 5: Verify GREEN and full backend regression**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::registry::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
```

Expected: all pass; accepted commit executes at most once regardless of response delivery.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/runtime.rs src-tauri/src/plugin/runtime/import.rs src-tauri/src/plugin/runtime/import/tests.rs src-tauri/src/plugin/import.rs
git commit -m "feat(plugin): coordinate fail-closed manifest import publication"
```

### Task 8: Wire native dialog, fixed IPC and main-only ACL

**Files:**
- Create: `src-tauri/src/plugin/import/dialog.rs`
- Modify: `src-tauri/src/plugin/import.rs`
- Modify: `src-tauri/src/commands/plugin.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/build.rs`
- Modify: `src-tauri/capabilities/plugin-runtime.json`
- Create: `src-tauri/permissions/autogenerated/prepare_local_manifest_import.toml`
- Create: `src-tauri/permissions/autogenerated/cancel_local_manifest_import.toml`
- Create: `src-tauri/permissions/autogenerated/commit_local_manifest_import.toml`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`

**Interfaces:**
- Consumes: Task 7 runtime methods and Task 2 selector/result types.
- Produces: `NativeLocalManifestSelector::new(window: tauri::WebviewWindow) -> Self` implementing LocalManifestSelector.
- Produces: commands `prepare_local_manifest_import(window: WebviewWindow, state: State<'_, AppState>) -> AppResult<PrepareImportResult>`, `cancel_local_manifest_import(state, token: String) -> AppResult<CancelImportResult>`, `commit_local_manifest_import(state, token: String, expected_catalog_generation: String) -> AppResult<CommitImportResult>`; injected window/state are not frontend payload keys.

- [ ] **Step 1: Add and run the ACL-only RED before wiring new modules**

Extend existing runtime-authority tests in lib.rs to this exact list:

```rust
for command in [
    "get_plugin_catalog", "reload_plugin_catalog", "set_plugin_enabled",
    "prepare_local_manifest_import", "cancel_local_manifest_import", "commit_local_manifest_import",
] {
    assert!(authority.resolve_access(command, "main", "main", &Origin::Local).is_some());
    assert!(authority.resolve_access(command, "main", "secondary", &Origin::Local).is_none());
    assert!(authority.resolve_access(command, "secondary", "secondary", &Origin::Local).is_none());
    assert!(authority.resolve_access(command, "main", "main", &remote).is_none());
}
```

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml plugin_commands_are_available_only_to_the_local_main_webview --lib`

Expected: compile succeeds and the assertion that a new command has local-main access fails. At this boundary do not yet declare `dialog.rs`, add command helper tests, reference command symbols or add `tauri-plugin-dialog`; Cargo compiles all unit-test modules before filtering, so those changes would mask the intended ACL RED.

- [ ] **Step 2: Resolve the dialog dependency, then write and run its focused RED**

Add `tauri-plugin-dialog = "2"` to Rust dependencies and resolve/review the Cargo lockfile using `cargo check --manifest-path src-tauri/Cargo.toml --tests`. Then declare `plugin::import::dialog`, create `dialog.rs`, and add `native_none_is_cancelled_not_error`, `native_uri_is_source_rejected`, `selector_channel_close_is_dialog_unavailable` as pure adapter tests. Run:

`cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::dialog --lib`

Expected: FAIL because the tested native choice/channel adapter or `NativeLocalManifestSelector` is not implemented; dependency resolution itself is not the RED.

- [ ] **Step 3: Implement and GREEN the Rust-only native dialog**

Add `.plugin(tauri_plugin_dialog::init())` in the application builder. Do not add an npm package or any dialog capability. The adapter calls callback `pick_file` on a file dialog configured with parent main window, title `选择插件清单`, JSON filter and single selection. Wrap its callback in a oneshot sender:

```rust
let (sender, receiver) = tokio::sync::oneshot::channel();
window.dialog().file().set_parent(&window).add_filter("JSON", &["json"])
    .set_title("选择插件清单")
    .pick_file(move |choice| { let _ = sender.send(choice); });
let choice = receiver.await.map_err(|_| dialog_unavailable())?;
match choice {
    None => Ok(SelectedManifestSource::Cancelled),
    Some(tauri_plugin_dialog::FilePath::Path(path)) => Ok(SelectedManifestSource::Selected(path)),
    Some(tauri_plugin_dialog::FilePath::Url(_)) => Err(source_rejected()),
}
```

The parent association uses the documented [`FileDialogBuilder::set_parent`](https://docs.rs/tauri-plugin-dialog/latest/tauri_plugin_dialog/struct.FileDialogBuilder.html#method.set_parent) API. Define local `dialog_unavailable() -> AppError` and `source_rejected() -> AppError` with the exact domain codes and diagnostic None. Do not infer invisible OS errors from None: it is cancelled. The dialog error uses only the concise first sentence `无法打开文件选择器，请重试。`; the spec table's following explanation is implementation guidance, not UI text. Re-run the focused dialog test and require GREEN before adding commands.

- [ ] **Step 4: Add command-helper RED, then register commands and narrow permissions**

Add command helper tests proving expected generation is forwarded unchanged and no path/raw JSON/source field is accepted. Run `cargo test --locked --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib` before defining the new helpers and require a missing-new-helper/API RED rather than an unrelated build failure.

Public command bodies only construct the native selector/delegate to runtime. Add all three commands to build.rs app manifest and generate_handler. Generate their allow/deny TOML via the normal build; add only these exact permissions to plugin-runtime:

```json
[
  "allow-get-plugin-catalog", "allow-reload-plugin-catalog", "allow-set-plugin-enabled",
  "allow-prepare-local-manifest-import", "allow-cancel-local-manifest-import",
  "allow-commit-local-manifest-import"
]
```

Keep webviews `["main"]`, no windows/remote/scope. Extend resolved-authority assertions to deny `plugin:dialog|open` and scan the union of every capability for dialog/fs grants. Inspect generated schema and capability union after adding the dialog plugin; registration must not implicitly grant its frontend open command.

- [ ] **Step 5: Verify GREEN**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml commands::plugin::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import::dialog --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml capability_tests --lib
```

Expected: public command and runtime-authority tests pass, including dialog/fs denied and remote/secondary isolation. The inspected baseline module is `capability_tests`; require a nonzero selected-test count.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/import.rs src-tauri/src/plugin/import/dialog.rs src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs src-tauri/build.rs src-tauri/capabilities/plugin-runtime.json src-tauri/permissions/autogenerated/prepare_local_manifest_import.toml src-tauri/permissions/autogenerated/cancel_local_manifest_import.toml src-tauri/permissions/autogenerated/commit_local_manifest_import.toml src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(plugin): expose native manifest import with restricted IPC"
```

### Task 9: Parse exact frontend import protocols and fixed error codes

**Files:**
- Modify: `src/types/plugin.ts`
- Modify: `src/services/pluginService.ts`
- Test: `tests/frontend/pluginService.test.ts`

**Interfaces:**
- Consumes: spec §6 discriminated unions, all existing strict v2 parsers.
- Produces: exported `PrepareLocalManifestImportResult`, `CancelLocalManifestImportResult`, `CommitLocalManifestImportResult`, `LocalManifestImportCommitFailure`; `ReadyLocalManifestImport = Extract<PrepareLocalManifestImportResult, {status:'ready'}>`.
- Produces: `prepareLocalManifestImport(): Promise<PrepareLocalManifestImportResult>`, `cancelLocalManifestImport(token: string): Promise<CancelLocalManifestImportResult>`, `commitLocalManifestImport(preview: ReadyLocalManifestImport): Promise<CommitLocalManifestImportResult>`.
- Produces: `pluginErrorCode(error: unknown): PluginErrorCode | null`, which accepts only the existing exact `{code,message}` transport shape and the closed known-code set; callers use the code, never the untrusted backend message.

- [ ] **Step 1: Write RED service tests with existing real fixture builders**

```ts
it('commits only a preview token and its bound generation', async () => {
  const manifest = validManifest('com.example.notes', 'com.example')
  const preview = {
    schemaVersion: 1, status: 'ready', token: 'a'.repeat(32),
    expiresInSeconds: 300, catalogGeneration: '2', manifest,
  }
  const parsed = await parseReadyThroughPrepare(preview)
  vi.mocked(tauriInvoke).mockResolvedValueOnce({
    schemaVersion: 1, status: 'notImported', disabledDecisionSaved: false,
    reasonCode: 'plugin_import_id_conflict', snapshot: validSnapshot(),
  })
  await commitLocalManifestImport(parsed)
  expect(tauriInvoke).toHaveBeenLastCalledWith('commit_local_manifest_import', {
    token: 'a'.repeat(32), expectedCatalogGeneration: '2',
  })
})
```

Define the test helper exactly; never cast an unvalidated fixture directly to the ready type:

```ts
async function parseReadyThroughPrepare(value: unknown): Promise<ReadyLocalManifestImport> {
  vi.mocked(tauriInvoke).mockResolvedValueOnce(value)
  const parsed = await prepareLocalManifestImport()
  if (parsed.status !== 'ready') throw new Error('ready preview required')
  return parsed
}
```

Add table tests rejecting: extra keys on each union; cancelled with token; ttl other than 300; token uppercase/nonhex/31/33 chars; generation `00`, `+1`, MAX+1; nonempty capabilities; imported with wrong manifest/name/version/source/status; notImported with true on stale/conflict/budget/state failure; publication_unconfirmed in notImported; unknown reason code; invalid nested snapshot. Valid tests cover all four result shapes including cancel. Add exact `pluginErrorCode` tests for a known code, an unknown code, extra keys, missing keys and a non-string message; only the first returns a code.

- [ ] **Step 2: Verify RED**

Run: `pnpm test -- tests/frontend/pluginService.test.ts`

Expected: missing import exports/new parser behavior, not a generic module failure unrelated to those exports.

- [ ] **Step 3: Implement exact unions and service parsers**

Copy the exact spec union fields into types/plugin.ts. Parse unknown through existing `requireExactObject`, `parseManifest`, `parseSnapshot`, `requireCanonicalRevision`. Define `parsePrepareImport`, `parseCancelImport`, `parseCommitImport(value, expectedManifest)` privately in service; add import error codes to the existing closed mapping using spec's fixed concise messages. Refactor the existing exact error-shape/code check behind `pluginErrorCode`; keep `pluginErrorMessage` behavior compatible by mapping a non-null returned code to the local fixed message and returning the generic local message otherwise.

```ts
export async function commitLocalManifestImport(
  preview: ReadyLocalManifestImport,
): Promise<CommitLocalManifestImportResult> {
  const value = await tauriInvoke<unknown>('commit_local_manifest_import', {
    token: preview.token,
    expectedCatalogGeneration: preview.catalogGeneration,
  })
  return parseCommitImport(value, preview.manifest)
}
```

For imported, require nested snapshot top availability available; find exact pluginId once in its sorted catalog; require localDeclarative, disabled and all nine manifest fields equal to preview (compare fields, not object insertion order). For notImported, only `plugin_import_write_failed` may accompany disabledDecisionSaved=true; every other failure happens before a successful disabled decision. For importedNotVisible require correct pluginId equal to preview ID, but do not require presence. Unknown transport/invalid response is an Error, not a fabricated notImported result. No retry in service.

- [ ] **Step 4: Verify GREEN**

```powershell
pnpm test -- tests/frontend/pluginService.test.ts
pnpm exec vue-tsc --noEmit
```

Expected: strict parser tests and typecheck pass; existing catalog methods/payloads are unchanged.

- [ ] **Step 5: Commit**

```powershell
git add src/types/plugin.ts src/services/pluginService.ts tests/frontend/pluginService.test.ts
git commit -m "feat(plugin): validate manifest import frontend protocols"
```

### Task 10: Add owned import flights without bypassing catalog arbitration

**Files:**
- Modify: `src/stores/plugin.ts`
- Test: `tests/frontend/pluginStore.test.ts`

**Interfaces:**
- Consumes: Task 9 services and existing private `adoptSnapshot(snapshot, requestOrder)`.
- Produces: refs `importStatus: 'idle'|'choosing'|'preview'|'committing'|'result'`, `importPreview: ReadyLocalManifestImport|null`, `importPreviewStale: boolean`, `importError: string|null`, `importResult: CommitLocalManifestImportResult|null`, `importOutcomeUnknown: boolean`.
- Produces: actions `prepareImport(): Promise<void>`, `cancelImport(): Promise<void>`, `commitImport(): Promise<void>`, `releaseImportView(): void`, `clearImportResult(): void`. Imported snapshots use the existing snapshotSequence/adoptSnapshot path; no separate catalog cache.

- [ ] **Step 1: Write RED generation and late-flight tests**

Extend `serviceMocks` with `prepareImport`, `cancelImport`, `commitImport`, mocking the corresponding real service exports. Reuse existing typed `item`, `snapshot`, `deferred` helpers. Define typed ready fixture inline:

```ts
const preview = {
  schemaVersion: 1, status: 'ready', token: 'a'.repeat(32), expiresInSeconds: 300,
  catalogGeneration: '1', manifest: item('com.example.notes').manifest,
} satisfies ReadyLocalManifestImport

it('does not replace a preview generation after reload', async () => {
  serviceMocks.getCatalog.mockResolvedValueOnce(snapshot('0', [], '1'))
  serviceMocks.prepareImport.mockResolvedValueOnce(preview)
  const store = usePluginStore()
  await store.load()
  await store.prepareImport()
  serviceMocks.reloadCatalog.mockResolvedValueOnce(snapshot('0', [], '2'))
  await store.reload()
  expect(store.importPreviewStale).toBe(true)
  await store.commitImport()
  expect(serviceMocks.commitImport).not.toHaveBeenCalled()
})
```

Add `late_prepare_after_view_release_cancels_returned_token`, `delayed_ready_after_newer_reload_is_stale`, `release_during_commit_preserves_background_snapshot_adoption`, `late_old_import_snapshot_cannot_restore_removed_member`, `same_generation_import_snapshot_obeys_revision_and_request_order`, `import_is_first_confirmed_snapshot_and_rejects_older_same_generation_response`, `expired_token_is_known_no_write_failure`, `unknown_commit_does_not_retry_or_claim_not_imported`, `double_confirm_calls_service_once`, `native_cancel_is_not_error`, `partial_failure_preserves_disabled_saved_explanation`.

- [ ] **Step 2: Verify RED**

Run: `pnpm test -- tests/frontend/pluginStore.test.ts`

Expected: missing import actions/refs.

- [ ] **Step 3: Implement owned state transitions**

Use a monotonic import owner sequence distinct from mutationOwners and snapshotSequence. Prepare sets choosing before invoking service; if the owner/view has been released by its return, cancel any returned ready token and discard it. Ready stores the exact parsed preview; at arrival immediately mark it stale when the already-adopted catalog generation is strictly greater than its bound generation, covering a reload that completed while the selector was open. Record a browser elapsed-time deadline for UI indication only. Cancel clears ready only after taking ownership and calls service best-effort; committing cannot cancel.

Implement commit with this critical shape:

```ts
async function commitImport(): Promise<void> {
  const preview = importPreview.value
  if (importStatus.value !== 'preview' || !preview || importPreviewStale.value) return
  importStatus.value = 'committing'
  const requestOrder = ++snapshotSequence
  try {
    const result = await commitLocalManifestImport(preview)
    confirmAuthoritativeSnapshot(result.snapshot, requestOrder)
    importResult.value = result
    importOutcomeUnknown.value = false
  } catch (error) {
    const code = pluginErrorCode(error)
    if (code === 'plugin_import_token_invalid' || code === 'plugin_import_busy') {
      importOutcomeUnknown.value = false
      importError.value = pluginErrorMessage(error)
    } else {
      importOutcomeUnknown.value = true
      importError.value = '结果尚未确认，请重新扫描。'
    }
  } finally {
    importPreview.value = null
    importStatus.value = 'result'
  }
}
```

Add a private `confirmAuthoritativeSnapshot(snapshot, requestOrder)` used by load, reload and import: it calls the existing arbitration, establishes `hasConfirmedSnapshot`, clears stale load failure state, and sets load status ready just as a successful get/reload does. This ensures an import result can be the first confirmed snapshot and later same-generation revisions still pass through the normal revision/request-order checks. Retain a commit flight so releaseImportView never cancels it or increments away its snapshot ownership. A second click sees committing and does not call the service again. Task 9 exports a closed `pluginErrorCode(error): PluginErrorCode | null`; only exact `plugin_import_token_invalid` and `plugin_import_busy` rejections are known pre-admission/no-write outcomes, while malformed responses, transport loss and internal errors remain unknown. In every successful adoption of a strictly higher generation, set `importPreviewStale` when it exceeds the stored ready generation; never rewrite that generation. Add refs/actions to the returned store object. Clear result only by explicit user action, not by changing plugin section.

- [ ] **Step 4: Verify GREEN and existing races**

```powershell
pnpm test -- tests/frontend/pluginStore.test.ts
pnpm test -- tests/frontend/pluginService.test.ts
pnpm exec vue-tsc --noEmit
```

Expected: new session races and existing tied snapshot/mutation identity tests all pass.

- [ ] **Step 5: Commit**

```powershell
git add src/stores/plugin.ts tests/frontend/pluginStore.test.ts
git commit -m "feat(plugin): arbitrate manifest import sessions in the store"
```

### Task 11: Ship accessible confirmation UI and precise local-manifest guidance

**Files:**
- Create: `src/components/plugins/PluginImportDialog.vue`
- Modify: `src/components/plugins/PluginMarketplacePage.vue`
- Modify: `src/components/plugins/PluginMarketplacePage.css`
- Create: `tests/frontend/pluginImportDialog.test.ts`
- Modify: `tests/frontend/pluginMarketplacePage.test.ts`
- Modify: `docs/plugin-local-manifests.md`

**Interfaces:**
- Consumes: Task 10 store; PluginSection existing values installed/market/manage.
- Produces: PluginImportDialog props `preview: ReadyLocalManifestImport`, `committing: boolean`, `stale: boolean`; emits `confirm`, `cancel`. Native HTML dialog with `.showModal()` supplies focus containment; initial focus on cancel, title/description IDs, escape prevented while committing. Component restores the captured opener only if still connected/current view.
- Produces: page entry on installed/manage only; result status/alert surfaces, explicit reload after unknown result.

- [ ] **Step 1: Write RED dialog and page tests**

```ts
it('renders author data as text and does not confirm on Escape', async () => {
  const wrapper = mount(PluginImportDialog, {
    attachTo: document.body,
    props: {
      preview: { ...readyPreview, manifest: { ...readyPreview.manifest, name: '<img src=x onerror=alert(1)>' } },
      committing: false, stale: false,
    },
  })
  expect(wrapper.find('img').exists()).toBe(false)
  expect(wrapper.text()).toContain('<img src=x onerror=alert(1)>')
  expect(wrapper.get('dialog').attributes('aria-modal')).toBe('true')
  await wrapper.get('dialog').trigger('cancel')
  expect(wrapper.emitted('cancel')).toHaveLength(1)
  expect(wrapper.emitted('confirm')).toBeUndefined()
  wrapper.unmount()
})
```

Define `readyPreview: ReadyLocalManifestImport` in this test file with all nine manifest fields, token `'a'.repeat(32)`, generation `'1'`, schema 1, status ready and ttl 300. Stub jsdom's missing `HTMLDialogElement.showModal/close` only as DOM state transitions; do not mock Vue event behavior. Add `committing_blocks_escape_and_double_confirm`, `stale_preview_disables_confirm`, `focus_returns_only_to_connected_opener`; page tests assert only installed/manage show “导入本地清单”, choosing cancel no alert, partial disabled error copy, importedNotVisible/unknown offer reload, and market contains no import button.

- [ ] **Step 2: Verify RED**

```powershell
pnpm test -- tests/frontend/pluginImportDialog.test.ts
pnpm test -- tests/frontend/pluginMarketplacePage.test.ts
```

Expected: missing dialog/import controls.

- [ ] **Step 3: Implement native dialog semantics and page wiring**

The markup kernel is:

```vue
<dialog ref="dialog" aria-modal="true" aria-labelledby="plugin-import-title"
  aria-describedby="plugin-import-warning" @cancel="onCancel">
  <h2 id="plugin-import-title">确认导入此清单</h2>
  <dl><dt>名称</dt><dd><bdi>{{ preview.manifest.name }}</bdi></dd></dl>
  <p id="plugin-import-warning">发布者信息由清单作者填写，未经认证。本次只复制元数据，不运行代码或授予权限。导入后默认停用，启用仅记录宿主偏好。</p>
  <p v-if="committing" role="status" aria-live="polite">正在导入清单…</p>
  <button ref="cancelButton" :disabled="committing" @click="$emit('cancel')">取消</button>
  <button :disabled="committing || stale" @click="$emit('confirm')">确认导入</button>
</dialog>
```

Render all remaining fields (ID, version, description, publisher, publisherId) with their own `<dt>/<dd><bdi>` pairs, plain interpolation, no HTML/autolinks. `onCancel(event: Event)` always preventDefault; emit cancel only when not committing. On mount capture document.activeElement, showModal and focus cancel. On unmount close dialog and restore only a connected captured HTMLElement while the current page still owns the session. Use `aria-busy` for committing; disable entry/confirm during busy. No ticking aria-live counter is needed; show a static five-minute validity hint and stale state.

Page button delegates prepareImport; modal delegates commit/cancel; page onBeforeUnmount calls releaseImportView. Result messages use exact spec meanings, including “导入未完成；该清单的停用偏好已保存，旧启用不会恢复”. Unknown outcome offers “重新扫描本地插件”, never “重试导入”. Keep existing section focus behavior and source labels; do not jump to market or enable anything automatically.

- [ ] **Step 4: Update the user guide with exact operational semantics**

Add subsections “通过应用导入单文件清单”, “确认内容与默认停用”, “部分失败与重新扫描”, “暂存区人工维护”, “状态恢复后重新启用”. Include:

```text
选择一份普通本地 JSON 文件，在预览中核对名称、ID、版本与作者信息，再点击“确认导入”。原文件不会被修改；导入后的清单默认停用。相同 ID 已存在时拒绝导入，不支持覆盖更新。
```

Document 300-second one-time preview, immutable captured content, fixed config root/staging location, 16-entry staging cap, all unchanged scan/state limits, and per-spec partial outcomes. State that old stages are not automatically recovered/deleted; instruct exit the application and inspect exact staging entries manually rather than supplying a broad recursive-delete command. Document that valid primary state retains local enabled, secondary recovery disables local only and requires explicit re-enable. Keep hand-created package shape and source labels; do not imply new ownership/uninstall functionality. Document unsupported concurrent EasiFlux plugin writers.

- [ ] **Step 5: Verify GREEN, typecheck, lint and build**

```powershell
pnpm test -- tests/frontend/pluginImportDialog.test.ts
pnpm test -- tests/frontend/pluginMarketplacePage.test.ts
pnpm test -- tests/frontend/pluginCard.test.ts
pnpm exec vue-tsc --noEmit
pnpm lint
pnpm build
```

Expected: pass; no new a11y/TypeScript/lint failures.

- [ ] **Step 6: Commit**

```powershell
git add src/components/plugins/PluginImportDialog.vue src/components/plugins/PluginMarketplacePage.vue src/components/plugins/PluginMarketplacePage.css tests/frontend/pluginImportDialog.test.ts tests/frontend/pluginMarketplacePage.test.ts docs/plugin-local-manifests.md
git commit -m "feat(plugin): add accessible local manifest import confirmation"
```

### Task 12: Prove crash/failure semantics across platforms and extend CI

**Files:**
- Modify: `src-tauri/src/plugin/runtime/import/tests.rs`
- Modify: `src-tauri/src/storage/local_plugin_import/tests.rs`
- Modify: `src-tauri/src/storage/plugin_state/tests.rs`
- Modify: `.github/workflows/ci.yml`
- Create: `tests/frontend/pluginSecurityWorkflow.test.ts`

**Interfaces:**
- Consumes: real temporary-root storage/state/runtime and Task 4 fault checkpoints.
- Produces: restart/failure integration tests using committed files (not fabricated snapshots), and a Windows/Linux/macOS plugin-security CI matrix.

- [ ] **Step 1: Add and run the persisted crash-contract integration regression**

Use real PluginStateStore and import storage under the same canonical temporary plugins root. This test closes the original backup-authorization resurrection path:

```rust
#[tokio::test]
async fn imported_package_stays_disabled_after_primary_corruption_and_backup_recovery() {
    let mut fixture = ImportFixture::with_real_disk().await;
    fixture.seed_enabled_orphan().unwrap();
    fixture.runtime = fixture.restart_runtime();
    let preview = fixture.prepare_valid().await;
    fixture.runtime.commit_import(&preview.token, &preview.catalog_generation).await.unwrap();
    fixture.corrupt_primary().unwrap();
    let restarted = fixture.restart_runtime();
    let snapshot = restarted.get_catalog().await.unwrap();
    assert_eq!(snapshot.plugins.len(), 1);
    assert_eq!(serde_json::to_value(&snapshot).unwrap()["plugins"][0]["status"], "disabled");
}
```

Extend the Task 7 fixture with exact helpers `async fn with_real_disk() -> Self`, `seed_enabled_orphan(&self) -> io::Result<()>` (write exact VALID fingerprint with enabled=true in real fixture state), `corrupt_primary(&self) -> io::Result<()>` (write `b"broken"` to fixture state.json), `restart_runtime(&self) -> Arc<PluginRuntime>` (new sessions/new store/same owned root). The test explicitly replaces runtime after seeding, before prepare and state load. Never manipulate the real user config.

Run: `cargo test --locked --manifest-path src-tauri/Cargo.toml imported_package_stays_disabled_after_primary_corruption_and_backup_recovery --lib`

Expected: PASS if Tasks 1/7 already cover the implementation; this is integration proof, not a fabricated RED claim. If it exposes missing production behavior, first preserve the failure as a regression, make the minimal correction in the exact responsible source files, stage those exact test/source paths, and create a separate `fix(plugin): ...` commit before resuming Task 12. Record that commit and rerun focused GREEN; do not hide production corrections inside Task 12's tests-only commit list.

- [ ] **Step 2: Exercise the complete persisted-state interruption table**

Use fixture reconstruction at these exact points: stage only, stage+disabled, promotion completed, final response lost. Assert target absent/disabled as specified, tokens invalid after reconstruction, leftover stages ignored/count toward 16, and main corruption falls back to disabled local/builtin preserved. Runtime import tests inject every `ImportFsStep` through Task 4's service and inject whole-save state failure through a `PluginStatePersistence` wrapper; assert no promotion on any precommit/state failure, no NotImported after rename, and no cleanup of a promoted stage. The internal `WriteStep` fault matrix stays in `storage/plugin_state/tests.rs`, where the existing private `AtomicFile` hook is legally accessible; extend those tests to combine every write checkpoint with secondary-recovery normalization rather than exposing filesystem internals crate-wide. For actual process termination, add an ignored helper test `plugin::runtime::import::tests::import_crash_child`, launched via `std::env::current_exe()` with `--ignored --exact plugin::runtime::import::tests::import_crash_child` and environment variables `EASIFLUX_IMPORT_TEST_ROOT` / `EASIFLUX_IMPORT_TEST_CHECKPOINT` containing only the owned fixture root/checkpoint. The helper's test-only hook exits with code 73 at `stage-ready`, `disabled-saved`, `promoted` or `published`; parent asserts 73, then builds a fresh runtime. Do not call exit in the main test runner.

Add a tests-only function `run_crash_child(root: &Path, checkpoint: &str) -> std::process::ExitStatus` that constructs this subprocess and waits with a 10-second watchdog; on timeout kills only that child process and fails. Assert at least one real filesystem no-replace and no-follow test runs per platform. Windows junction setup uses existing safe_fs fixture logic; macOS tests use canonical temporary roots and actual exclusive rename.

- [ ] **Step 3: Write and run RED CI coverage contract**

Create a small workflow coverage test, not a claim that remote tests already ran:

```ts
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { expect, it } from 'vitest'

it('declares all three OS runners and all import security suites', () => {
  const workflow = readFileSync(resolve(process.cwd(), '.github/workflows/ci.yml'), 'utf8')
  const match = workflow.match(
    /(?:^|\r?\n)  plugin-security:\r?\n[\s\S]*?(?=\r?\n  [a-zA-Z0-9_-]+:\r?\n|$)/,
  )
  if (!match) throw new Error('plugin-security job required')
  const job = match[0]
  expect(job).toContain('os: [ubuntu-latest, windows-latest, macos-latest]')
  expect(job).toContain('runs-on: ${{ matrix.os }}')
  for (const suite of [
    'plugin::discovery::safe_fs::tests', 'plugin::import',
    'storage::local_plugin_import', 'storage::plugin_state::tests',
    'plugin::runtime::import::tests',
  ]) {
    expect(job).toContain(`cargo test --locked --manifest-path src-tauri/Cargo.toml ${suite} --lib`)
  }
})
```

Run: `pnpm test -- tests/frontend/pluginSecurityWorkflow.test.ts`

Expected: FAIL because the current workflow lacks macOS and the import-security suites. This textual coverage contract supplements the actual workflow review and remote results; it is not a YAML parser or a replacement for platform execution.

- [ ] **Step 4: Extend CI with concrete platform commands**

Retain the existing frontend/full Rust jobs, but make the Rust job's commands match acceptance exactly: `cargo test --locked --all-targets` and `cargo clippy --locked --all-targets` from its existing `src-tauri` working directory. Replace the Windows-only discovery-security job with a matrix named plugin-security:

```yaml
  plugin-security:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev pkg-config
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import --lib
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import --lib
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
      - run: cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
```

CI test output must report nonzero relevant test counts; a cfg-gated empty test run cannot certify the platform. Linux package list follows the existing workflow; inspect newly resolved dialog crate features for any required system package and add it only if its build emits that dependency requirement. Do not broadly install a desktop stack or enable frontend dialog APIs.

- [ ] **Step 5: Verify GREEN locally and inspect CI definition**

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::discovery::safe_fs::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::import --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml plugin::runtime::import::tests --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::local_plugin_import --lib
cargo test --locked --manifest-path src-tauri/Cargo.toml storage::plugin_state::tests --lib
pnpm test -- tests/frontend/pluginSecurityWorkflow.test.ts
git diff --check
```

Expected: all five current-platform security filters report a nonzero selected-test count and pass, and the workflow structure matches the matrix. Other OS results are pending until CI runs; never label them locally verified.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/plugin/runtime/import/tests.rs src-tauri/src/storage/local_plugin_import/tests.rs src-tauri/src/storage/plugin_state/tests.rs .github/workflows/ci.yml tests/frontend/pluginSecurityWorkflow.test.ts
git commit -m "test(plugin): verify import recovery and platform security"
```

## Final verification and handoff

- [ ] Run the entire frontend and backend suites from this worktree:

```powershell
pnpm test
pnpm exec vue-tsc --noEmit
pnpm lint
pnpm build
cargo test --locked --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: exit 0 everywhere; no new warning in changed Rust paths. Record existing unrelated warnings separately, without broad formatting edits.

- [ ] Check changed Rust formatting without recursing into unrelated module debt:

```powershell
git diff --name-only 2e519da9c35ab7ccdec4de2171cc81e429c7c437 -- '*.rs' | ForEach-Object { rustfmt --edition 2021 --config skip_children=true --check $_ }
git diff --check 2e519da9c35ab7ccdec4de2171cc81e429c7c437
git status --short --branch
```

Expected: scoped rustfmt and diff checks succeed; no unrelated edits are staged or committed. If rustfmt lacks skip_children support, invoke each changed leaf directly and record baseline module-tree differences instead of rewriting them.

- [ ] Inspect exact security surface:

```powershell
rg -n 'prepare_local_manifest_import|cancel_local_manifest_import|commit_local_manifest_import' src-tauri/build.rs src-tauri/src/commands/plugin.rs src-tauri/src/lib.rs src-tauri/capabilities/plugin-runtime.json src-tauri/permissions/autogenerated
rg -n 'dialog:|fs:|remote|\*' src-tauri/capabilities
rg -n 'install_plugin|uninstall_plugin|download_plugin|runtime\.lock|eval\(|WebAssembly|Library::new' src-tauri/src/plugin src/services/pluginService.ts src/stores/plugin.ts src/components/plugins
```

Expected: six fixed plugin commands with three new minimal permissions, no new dialog/fs/remote grant, no execution/uninstall/cross-process-lock implementation. Interpret unrelated preexisting matches from diffs, not raw search count.

- [ ] Perform a real native-desktop smoke in an already provisioned disposable desktop OS profile/VM whose configuration root is a test fixture, not the user's working profile. Stock app launch has no arbitrary configuration-root override; do not invent one or point the smoke at real user state. In that isolated profile, use an owned temporary source: cancel selector; preview valid file; change source after preview; confirm; verify captured content/default disabled; explicitly enable and then disable that imported item so the atomic store leaves an enabled prior document in `.bak`; restart; corrupt only fixture primary state; restart again and confirm the secondary candidate loads while local remains disabled; visit core business pages. Record OS, actual fixture config root, commands, outcomes and cleanup of only that fixture. If an isolated desktop environment is unavailable, report native whole-app smoke as unverified; backend injection tests and component tests are not a substitute, and setting a guessed environment variable is not proof of isolation.

- [ ] Require Windows/Linux/macOS security CI results before claiming cross-platform verification. If one platform has not run, report exactly that remaining verification rather than claiming completion across all OSes.

- [ ] Request independent spec-compliance and code-quality/security review through the execution skill. Fix each actionable finding with its own failing regression, minimal fix, focused GREEN and commit; rerun the entire final suite afterwards.

- [ ] Handoff states: implemented tasks/commits, verification commands and results, platform evidence, any remaining blocker, deliberate limitations (no execution/updates/removal/signatures; single-process writes; explicit partial failures). Do not merge, publish or delete the branch without separate authorization.

## Plan self-review coverage map

| Spec requirement | Implementation task |
| --- | --- |
| §1–3 nonexecution, scope, partial failure, secondary recovery | 1, 2, 7, 11 |
| §4 exact capacities, fixed names, staging retention | 2, 4, 5, 11 |
| §5 native picker, no-follow bounded input, content snapshot | 2, 3, 8 |
| §6 exact envelope, TTL, one-time ownership, cancellation | 2, 7, 9, 10 |
| §7 preflight/disabled/promotion/authoritative publication | 4, 5, 6, 7 |
| §8 all failure and crash points | 1, 4, 7, 12 |
| §9 FIFO, cancellation-safe work, blocking IO, unlocked reads | 6, 7, 10 |
| §10 error closure and illegal response handling | 2, 7, 9, 10 |
| §11 UX, focus, stale preview, result copy, view release | 10, 11 |
| §12–13 interfaces and union ACL | 2–10 |
| §14–15 tests, platform evidence, acceptance | every task; 12 and final verification |

Self-review before execution: no schema change, no absent-ID public mutator, no path payload, no token stored on disk, no second catalog state, no re-open of confirmed source, no precommit failure reported after rename, no automatic recovery of staging, no runtime.lock, and no claims that an unrun platform or native smoke passed.
