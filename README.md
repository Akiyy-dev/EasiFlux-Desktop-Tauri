# EasiFlux Desktop (Tauri)

基于 **Tauri 2**、**Rust** 与 **Vue 3** 构建的现代加密货币合约交易桌面客户端。

这是下一代客户端。旧版 Python/PySide6 实现仍保留在 [EasiFlux-Desktop](https://github.com/Akiyy-dev/EasiFlux-Desktop)。

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面壳 | Tauri 2 |
| 后端 | Rust（tokio, reqwest, tokio-tungstenite） |
| 前端 | Vue 3, TypeScript, Vite, Pinia, Naive UI |
| 图表 | ECharts |

API 通信在 Rust 中独立实现（协议与 [EasiFlux-SDK](https://github.com/Akiyy-dev/EasiFlux-SDK) 对齐）；运行时**不嵌入** Python SDK。

## 环境要求

- Node.js 20+
- pnpm 9+
- Rust stable（Windows 需 MSVC 工具链）
- [Tauri 前置依赖](https://v2.tauri.app/start/prerequisites/)

## 开发

```bash
pnpm install
pnpm tauri dev
```

仅前端（浏览器模式，IPC 调用会失败）：

```bash
pnpm dev
```

## 构建

```bash
pnpm tauri build
```

产物输出至 `src-tauri/target/release/bundle/`。

## 测试

```bash
# 前端
pnpm test
pnpm lint

# Rust
cd src-tauri && cargo test && cargo clippy
```

## 项目结构

```
src/                 Vue 3 前端
src-tauri/src/       Rust 核心（api, ws, services, storage）
tests/               前端与 Rust 测试
```

## 配置

- 用户配置：`%APPDATA%/EasiFlux Desktop/config.toml`（与旧版客户端 schema 兼容）
- API 凭据：系统 Keyring（service 名 `easiflux_desktop_tauri`）

## 版本与发布

本项目使用 [release-please](https://github.com/googleapis/release-please) 管理版本与变更日志。提交请遵循 [Conventional Commits](https://www.conventionalcommits.org/) 规范，详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 贡献

欢迎提交 Issue 与 Pull Request。请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 了解提交规范与开发流程。

## 许可证

MIT
