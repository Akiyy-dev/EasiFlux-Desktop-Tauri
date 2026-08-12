# 新闻模块移除实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 从桌面应用中完整移除新闻中心及其后台、存储、凭据、构建与发布接线，同时保持非新闻模块行为不变。

**Architecture:** 以共享导航类型、Tauri `AppState`/handler 注册和发布 workflow 输入为三个删除边界。前端入口使用 RED/GREEN 行为测试；Rust 纯删除与 workflow 配置变更使用变更前基线、编译/既有测试和残留扫描验证。Cargo 锁文件由当前 manifest 重算，不回退历史版本。

**Tech Stack:** Vue 3、TypeScript、Vitest、Pinia、Tauri 2、Rust、Cargo、GitHub Actions

## Global Constraints

- 工作分支固定为 `dev/remove-news-module`，直接在当前检出目录修改，不使用 worktree。
- 完整删除新闻导航、仪表盘入口、前后端实现、专用测试、构建凭据和发布资产流程。
- 不设计或实现任何新闻插件、插件占位、兼容 API 或迁移层。
- 不删除用户设备上的旧新闻 SQLite 数据库、Keyring 条目或历史 Release 资产。
- 保留 `CHANGELOG.md` 的既有发布历史，保留仍被其他模块使用的 `keyring`、`reqwest` 和 Tauri opener。
- 只执行基础检查：残留扫描、前端 lint/test/build、Rust fmt/check/test。

---

### Task 1: 移除前端新闻入口和实现

**Files:**
- Modify: `tests/frontend/accountNavigation.test.ts`
- Modify: `tests/frontend/chartWorkspacePage.test.ts`
- Modify: `src/components/layout/AppShell.vue`
- Modify: `src/components/layout/NavigationRail.vue`
- Modify: `src/components/dashboard/DashboardQuickActions.vue`
- Modify: `src/components/dashboard/DashboardRecentActivity.vue`
- Modify: `src/components/dashboard/types.ts`
- Modify: `src/types/navigation.ts`
- Delete: `src/components/news/NewMessagesBanner.vue`
- Delete: `src/components/news/NewsCenterPage.vue`
- Delete: `src/components/news/newsCenterPage.css`
- Delete: `src/components/news/NewsStatusBar.vue`
- Delete: `src/components/news/newsStatusBar.css`
- Delete: `src/components/news/NewsTimeline.vue`
- Delete: `src/components/news/newsTimeline.css`
- Delete: `src/composables/useNewsPageController.ts`
- Delete: `src/composables/useNewsRuntimeHost.ts`
- Delete: `src/services/newsService.ts`
- Delete: `src/stores/news.ts`
- Delete: `src/stores/newsAwayRecovery.ts`
- Delete: `src/stores/newsLatestCoordinator.ts`
- Delete: `src/stores/newsLoadMore.ts`
- Delete: `src/stores/newsPending.ts`
- Delete: `src/stores/newsState.ts`
- Delete: `src/stores/newsStatusOrder.ts`
- Delete: `src/stores/newsViewMerge.ts`
- Delete: `src/types/news.ts`
- Delete: `src/utils/newsLinks.ts`
- Delete: `tests/frontend/newsCenterPage.test.ts`
- Delete: `tests/frontend/newsLatestCoordinator.test.ts`
- Delete: `tests/frontend/newsLinks.test.ts`
- Delete: `tests/frontend/newsNavigation.test.ts`
- Delete: `tests/frontend/newsRuntimeHost.test.ts`
- Delete: `tests/frontend/newsService.test.ts`
- Delete: `tests/frontend/newsStatusBar.test.ts`
- Delete: `tests/frontend/newsStore.test.ts`
- Delete: `tests/frontend/newsStoreAwayRecovery.test.ts`
- Delete: `tests/frontend/newsStoreCausalRace.test.ts`
- Delete: `tests/frontend/newsStoreConcurrencyReview.test.ts`
- Delete: `tests/frontend/newsStoreConcurrencyRound2.test.ts`
- Delete: `tests/frontend/newsStoreInactivePending.test.ts`
- Delete: `tests/frontend/newsStoreStatusRace.test.ts`
- Delete: `tests/frontend/newsTimeline.test.ts`

**Interfaces:**
- Consumes: 现有 `AppShell`、`NavigationRail` 和 `DashboardQuickActions` 挂载测试。
- Produces: 不含 `news` 的 `NavKey` 与 `DashboardNavTarget`；主导航和仪表盘不再暴露新闻入口。

- [ ] **Step 1: 写入会失败的前端移除测试**

在 `tests/frontend/accountNavigation.test.ts` 增加：

```ts
it('does not expose news navigation or a dashboard news action', () => {
  const wrapper = mountShell()

  expect(wrapper.find('button[aria-label^="新闻"]').exists()).toBe(false)
  expect(wrapper.getComponent(DashboardQuickActions).text()).not.toContain('新闻中心')
})
```

- [ ] **Step 2: 运行测试并确认按预期失败**

Run: `pnpm exec vitest run tests/frontend/accountNavigation.test.ts -t "does not expose news"`

Expected: FAIL，因为当前导航栏和仪表盘仍包含新闻入口。

- [ ] **Step 3: 删除前端新闻实现并清理共享接线**

删除上列新闻专属源码和测试。共享文件按以下最小行为调整：

- `NavKey`、`DashboardNavTarget` 和 `DashboardActivityType` 删除 `news` 成员。
- `AppShell.vue` 删除新闻 imports、store/runtime 初始化、`newsVisited`、标题映射、页面块和 unread prop；Sidebar 只在 charts 页面隐藏。
- `NavigationRail.vue` 删除 Newspaper 图标、新闻 item、unread prop/computed/ARIA/CSS。
- 仪表盘删除新闻快捷操作与 news placeholder activity。
- `accountNavigation.test.ts` 删除 news runtime mock，把 Sidebar 测试改为只验证 charts，并通过“账户”ARIA 标签返回普通页面。
- `chartWorkspacePage.test.ts` 删除 news runtime mock。

- [ ] **Step 4: 运行前端定向测试并确认通过**

Run: `pnpm exec vitest run tests/frontend/accountNavigation.test.ts tests/frontend/chartWorkspacePage.test.ts`

Expected: PASS，且新增缺失入口断言为绿色。

- [ ] **Step 5: 提交前端删除**

```powershell
git add -A -- src tests/frontend
git commit -m "refactor: remove news frontend"
```

### Task 2: 移除 Rust/Tauri 新闻运行时和依赖

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/api/mod.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/events/emitter.rs`
- Modify: `src-tauri/src/events/mod.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/storage/mod.rs`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`
- Delete: `src-tauri/build_support/news_build_config.rs`
- Delete: `src-tauri/tests/news_build_config.rs`
- Delete: `src-tauri/src/api/news_client.rs`
- Delete: `src-tauri/src/api/news_client/error.rs`
- Delete: `src-tauri/src/api/news_client/validation.rs`
- Delete: `src-tauri/src/api/news_client_tests.rs`
- Delete: `src-tauri/src/bin/easiflux-news-provision.rs`
- Delete: `src-tauri/src/commands/news.rs`
- Delete: `src-tauri/src/commands/news_tests.rs`
- Delete: `src-tauri/src/events/news.rs`
- Delete: `src-tauri/src/events/news_tests.rs`
- Delete: `src-tauri/src/models/news.rs`
- Delete: `src-tauri/src/models/news_tests.rs`
- Delete: `src-tauri/src/news_lifecycle_tests.rs`
- Delete: `src-tauri/src/news_provision.rs`
- Delete: `src-tauri/src/news_provision_tests.rs`
- Delete: `src-tauri/src/news_runtime.rs`
- Delete: `src-tauri/src/news_runtime_tests.rs`
- Delete: `src-tauri/src/services/news.rs`
- Delete: `src-tauri/src/services/news/backoff.rs`
- Delete: `src-tauri/src/services/news/blocking.rs`
- Delete: `src-tauri/src/services/news/control.rs`
- Delete: `src-tauri/src/services/news/error.rs`
- Delete: `src-tauri/src/services/news/facade.rs`
- Delete: `src-tauri/src/services/news/poller.rs`
- Delete: `src-tauri/src/services/news/poller/cycle.rs`
- Delete: `src-tauri/src/services/news/poller/session.rs`
- Delete: `src-tauri/src/services/news/ports.rs`
- Delete: `src-tauri/src/services/news/shutdown.rs`
- Delete: `src-tauri/src/services/news/status.rs`
- Delete: `src-tauri/src/services/news/tests/backoff.rs`
- Delete: `src-tauri/src/services/news/tests/facade.rs`
- Delete: `src-tauri/src/services/news/tests/mod.rs`
- Delete: `src-tauri/src/services/news/tests/poller.rs`
- Delete: `src-tauri/src/services/news/tests/ports.rs`
- Delete: `src-tauri/src/services/news/tests/status.rs`
- Delete: `src-tauri/src/services/news/tests/support.rs`
- Delete: `src-tauri/src/storage/news_database.rs`
- Delete: `src-tauri/src/storage/news_database/error.rs`
- Delete: `src-tauri/src/storage/news_database/pagination.rs`
- Delete: `src-tauri/src/storage/news_database/queries.rs`
- Delete: `src-tauri/src/storage/news_database/schema.rs`
- Delete: `src-tauri/src/storage/news_database/schema_validation.rs`
- Delete: `src-tauri/src/storage/news_database/transactions.rs`
- Delete: `src-tauri/src/storage/news_database_tests/mod.rs`
- Delete: `src-tauri/src/storage/news_database_tests/queries.rs`
- Delete: `src-tauri/src/storage/news_database_tests/schema.rs`
- Delete: `src-tauri/src/storage/news_database_tests/transactions.rs`
- Delete: `src-tauri/src/storage/news_token.rs`
- Delete: `src-tauri/src/storage/news_token_tests.rs`

**Interfaces:**
- Consumes: Tauri application setup, invoke handler list, `AppState`, event emitter, Cargo manifest.
- Produces: 单一桌面 binary；无新闻服务字段、命令、事件、后台任务或专用构建配置。

- [ ] **Step 1: 运行变更前 Rust 基线**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: 两条命令均 PASS；若基线失败，先记录并停止归因，不把既有失败误判为删除引入。

- [ ] **Step 2: 删除 Rust 新闻模块并清理共享注册点**

- `lib.rs` 删除新闻模块、公开 token 类型、启动/退出处理、5 个 handler 和 lifecycle 测试注册；保留 scheduler 及其他退出逻辑。
- `state.rs` 删除 `NewsService` import、字段、构造和初始化。
- 各 `mod.rs` 删除新闻模块导出；`events/emitter.rs` 删除 `NewsEventSink` 实现。
- `build.rs` 精简为：

```rust
fn main() {
    tauri_build::build()
}
```

- `Cargo.toml` 删除 `default-run` 和显式 bin 块、build-dependency `url`、`reqwest` 的 `stream` feature、`zeroize`、`rpassword`、`async-trait`、`httpdate`、`rusqlite`、dev `tempfile` 与 tokio `test-util`。保留现有 `keyring` 平台 features。
- 运行 Cargo 命令让 `Cargo.lock` 基于当前 manifest 更新，禁止用 pre-news 锁文件覆盖。

- [ ] **Step 3: 运行基础 Rust 检查**

Run: `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: PASS，并只更新新闻专属依赖对应的锁文件节点。

Run: `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: PASS。

- [ ] **Step 4: 提交 Rust/Tauri 删除**

```powershell
git add -A -- src-tauri
git commit -m "refactor: remove news backend"
```

### Task 3: 移除新闻发布流程和过期文档

**Files:**
- Delete: `tests/frontend/newsReleaseWorkflow.test.ts`
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/release-please.yml`
- Modify: `.github/workflows/tauri-build-reusable.yml`
- Modify: `docs/UI_REFACTOR.md`
- Delete: `docs/news-deployment.md`
- Delete: `docs/superpowers/specs/2026-07-30-news-center-design.md`
- Delete: `docs/superpowers/plans/2026-07-30-news-center.md`

**Interfaces:**
- Consumes: release/release-please callers and reusable Tauri build workflow.
- Produces: 只接收 `tag_name` 的 reusable workflow；调用方恢复 `secrets: inherit`；不再构建或上传 provision 资产。

- [ ] **Step 1: 记录发布 workflow 基线**

Run: `pnpm exec vitest run tests/frontend/newsReleaseWorkflow.test.ts`

Expected: PASS，证明删除前的新闻发布契约测试处于绿色；该测试会随新闻发布流程一起删除。

- [ ] **Step 2: 清理发布 workflow 与文档**

- 两个 caller 删除新闻 input/secret 映射并恢复 `secrets: inherit`。
- reusable workflow 删除新闻 input/secrets、provision 构建/校验和/上传、桌面构建中的新闻环境变量。
- 保留固定 SHA 的 Tauri action 和 `libdbus-1-dev`。
- 删除现役新闻部署文档、旧新闻设计/计划和旧正向发布测试。
- `docs/UI_REFACTOR.md` 的快捷入口改为“交易 / 账户 / 插件”，动态项删除 RSS 新闻占位描述。
- 不改写 `CHANGELOG.md` 的 0.4.0 历史。

- [ ] **Step 3: 检查 workflow 残留和 diff**

Run: `rg -n -i "EASIFLUX_NEWS|easiflux-news|news_api_|news_source_epoch|news-provision" .github/workflows`

Expected: 无匹配。

Run: `git diff --check -- .github docs tests/frontend`

Expected: PASS。

- [ ] **Step 4: 提交发布和文档清理**

```powershell
git add -A -- .github docs tests/frontend
git commit -m "chore: remove news release pipeline"
```

### Task 4: 执行基础全量验证

**Files:**
- Verify only: all changed files

**Interfaces:**
- Consumes: Tasks 1-3 的完整结果。
- Produces: 可交付的新闻模块移除分支和验证证据。

- [ ] **Step 1: 扫描运行时代码和 workflow 残留**

Run:

```powershell
rg -n -i "EASIFLUX_NEWS|easiflux-news|get_news_|list_news_|mark_news_|retry_news_|recheck_news_|news://" src src-tauri .github/workflows
rg -n -i "\bnews\b|新闻" src src-tauri .github/workflows
```

Expected: 两条命令均无匹配。测试与历史文档中的契约/发布记录不在扫描范围内。

- [ ] **Step 2: 运行前端基础检查**

```powershell
pnpm lint
pnpm test
pnpm build
```

Expected: 全部 PASS。

- [ ] **Step 3: 运行 Rust 基础检查**

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo check --manifest-path src-tauri/Cargo.toml --all-targets
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

Expected: 全部 PASS。

- [ ] **Step 4: 检查最终 diff**

```powershell
git diff --check
git status --short --branch
git log --oneline -4
```

Expected: 当前分支为 `dev/remove-news-module`，无非新闻范围的未提交修改，最近提交对应设计、前端、后端和发布清理。
