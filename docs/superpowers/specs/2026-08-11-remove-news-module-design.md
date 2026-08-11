# 新闻模块移除设计

## 目标

从桌面应用中完整移除现有新闻中心，停止其界面、后台同步、存储、凭据、命令和发布流程。后续插件替代方案不属于本次范围。

工作分支为 `dev/remove-news-module`，直接在当前检出目录修改，不创建 Git worktree。

## 范围

### 删除

- 删除主导航和仪表盘中的新闻入口、新闻页面、未读角标和新闻运行时宿主。
- 删除前端新闻组件、composable、service、Pinia store、类型、工具函数及专用测试。
- 删除 Rust 新闻 API 客户端、模型、命令、事件、服务、轮询器、SQLite 存储、Token 管理、运行时、生命周期和专用测试。
- 删除新闻凭据配置校验、provision 二进制、专用 Cargo 依赖和发布资产构建/上传流程。
- 删除现役新闻部署文档以及对应的旧新闻设计与实施计划，避免留下可被误认为仍受支持的说明。

### 保留

- 保留交易、账户、行情、图表、插件和通用导航行为。
- 保留仍被其他模块使用的 `keyring`、`reqwest`、Tauri opener 插件和通用事件/命令基础设施。
- 保留 `CHANGELOG.md` 中已经发布过新闻功能的历史记录。
- 保留现有用户设备上的新闻 SQLite 数据库、Keyring 条目和旧 Release 资产；本次不执行破坏性数据清理或迁移。
- 不增加插件占位、插件 API、兼容层或未来新闻插件实现。

## 实现边界

前端以共享 `NavKey` 为编译期边界：移除 `news` 联合类型成员后，同步清理 `AppShell`、`NavigationRail`、仪表盘和共享导航测试中的引用。新闻专属源码与测试直接删除，不保留禁用代码。

Rust 侧移除 `AppState` 中的 `NewsService`、应用启动/退出生命周期、Tauri handlers、事件 sink 和各模块注册。`build.rs` 仍保留 `tauri_build::build()`。`Cargo.toml` 仅定点删除新闻二进制、专用依赖和 feature；`Cargo.lock` 由 Cargo 重新生成，避免回退后续依赖安全更新。

发布工作流删除新闻 secrets、source epoch、provision 工具构建、校验和生成与上传步骤，同时保留非新闻构建逻辑和已固定到提交 SHA 的 Tauri action。

## 数据与错误处理

新闻代码移除后，应用不再读取、写入或清理旧新闻数据。删除过程中不引入运行时错误分支；所有残留引用应在 TypeScript 或 Rust 编译期被发现。构建工作流不再要求任何 `EASIFLUX_NEWS_*` 配置。

## 验证

本次只做基础检查：

- 用 `rg` 检查新闻运行时标识、命令、事件和构建变量是否残留；允许历史 `CHANGELOG.md` 中的发布记录继续出现。
- 运行 `pnpm lint`、`pnpm test` 和 `pnpm build`。
- 运行 Rust 格式检查、`cargo check --all-targets` 和 `cargo test --all-targets`。
- 确认 Git diff 只包含新闻模块删除、共享接线清理、锁文件更新及本设计/实施计划。

## 完成标准

- 应用界面没有新闻入口、占位页或未读角标。
- 应用启动后不再创建新闻服务、数据库或网络轮询任务。
- Tauri 不再公开新闻命令或新闻事件。
- Cargo 与发布工作流不再构建或配置新闻 provision 工具。
- 基础前端和 Rust 检查通过，且非新闻模块行为保持不变。
