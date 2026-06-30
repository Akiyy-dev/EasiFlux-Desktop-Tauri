# 贡献指南 — EasiFlux Desktop (Tauri)

## 环境准备

```bash
pnpm install
```

请确保已安装 Rust 及对应平台的 [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)。

## 常用命令

```bash
pnpm tauri dev      # 启动桌面应用
pnpm lint           # ESLint
pnpm test           # Vitest
pnpm build          # 前端生产构建
cd src-tauri && cargo test && cargo clippy
```

## 代码风格

- TypeScript：`strict` 模式，禁止 `any`
- Rust：`cargo fmt`，保持 clippy 清洁
- 模块保持单一职责、体积精简

## 提交规范

本项目使用 [Conventional Commits](https://www.conventionalcommits.org/)，由 [release-please](https://github.com/googleapis/release-please) 自动生成 `CHANGELOG.md` 与版本号。

格式：`<type>(<scope>): <description>`

| 类型 | 用途 | 版本影响 |
|------|------|----------|
| `feat` | 新功能 | minor |
| `fix` | Bug 修复 | patch |
| `docs` | 文档 | 无 |
| `chore` | 构建/工具 | 无 |
| `refactor` | 重构 | 无 |
| `test` | 测试 | 无 |
| `ci` | CI 配置 | 无 |
| `perf` | 性能优化 | patch |
| `BREAKING CHANGE:` 或 `feat!:` | 破坏性变更 | major |

示例：

```
feat(trading): add market order support
fix(ws): reconnect on ping timeout
chore(ci): install tauri linux deps
docs(readme): translate to Chinese
```

## 版本与发布

- 版本号由 release-please 统一管理（`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`）
- 向 `main` 合并符合规范的 commit 后，release-please 会自动创建 Release PR
- 合并 Release PR 后自动打 tag、生成 GitHub Release，并触发多平台打包

## Pull Request

1. Fork 并创建功能分支
2. 运行 lint 与测试
3. 提交信息遵循上述 Conventional Commits 规范
4. 向 `main` 发起 PR

## 相关仓库

- 旧版客户端：[EasiFlux-Desktop](https://github.com/Akiyy-dev/EasiFlux-Desktop)（Python/PySide6）
- 协议参考：[EasiFlux-SDK](https://github.com/Akiyy-dev/EasiFlux-SDK)（Python，运行时未嵌入）
