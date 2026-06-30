# AGENTS.md — EasiFlux Desktop (Tauri) 项目记忆

> **新会话请先读此文件。** 本文档记录仓库背景、架构决策、目录约定与开发注意事项，避免重复探索。

## 项目定位

- **路径**：`G:\EasiFlux\EasiFlux-Desktop-Tauri`
- **版本**：`0.2.0`（`package.json` / `src-tauri/Cargo.toml`）
- **性质**：EasiCoin 合约交易桌面客户端的 **下一代实现**（Tauri 2 + Rust + Vue 3）
- **状态**：四阶段迁移计划（Phase 1–4）已落地初版；可开发、可测试，生产打包需在本地验证 WebView2/磁盘空间

## 三仓库关系（勿混淆）

| 仓库 | 路径 | 角色 | 是否修改 |
|------|------|------|----------|
| **本仓库（主开发）** | `G:\EasiFlux\EasiFlux-Desktop-Tauri` | Tauri + Rust + Vue 客户端 | **是** |
| 旧客户端（只读参考） | `G:\EasiFlux\EasiFlux-Desktop` | Python/PySide6 实现 | **否**（README 已链到新仓库） |
| Python SDK（独立） | `G:\EasiFlux\EasiFlux-SDK` | 协议参考；供脚本/其他项目 | **否** |

### SDK 策略（已确认，不可违背）

- **不嵌入** Python SDK，不用 sidecar，不打包 Python 运行时
- **不重构** `EasiFlux-SDK` 源码
- Rust 在 `src-tauri/src/` **独立实现** REST / WebSocket / HMAC 签名，以 SDK 与旧 Desktop 的 `ModelMapper` 为协议对照
- 未来若需 Rust SDK，可从 `api/` + `ws/` + `auth/` 抽离为独立 crate，**不在本仓库当前范围**

## 技术栈

| 层 | 技术 |
|----|------|
| 桌面壳 | Tauri 2 |
| 后端 | Rust（tokio, reqwest, tokio-tungstenite, hmac/sha2, keyring, toml） |
| 前端 | Vue 3, TypeScript (strict), Vite, Pinia, Naive UI, ECharts (vue-echarts) |
| 通信 | Tauri Commands（请求/响应）+ Tauri Events（推送） |

## 架构与数据流

```
Vue (Pinia Stores)
  → invoke() Tauri Commands
  → Rust commands/（薄层）
  → services/（业务编排）
  → api/ + ws/ + storage/
  → EasiCoin REST/WS

Rust services → emit() → 前端 listen() → Pinia 更新 UI
```

### 前端布局（交易终端）

- 左：市场列表 `MarketList`
- 中：Ticker + K 线 (ECharts) + 深度 `OrderBook`
- 右：下单 `OrderPanel` + 账户摘要
- 底：订单 / 持仓 / 日志 / 分析（Tab）

### Pinia Stores

`app`, `config`, `connection`, `market`, `order`, `position`, `account`, `log`

### Tauri Events（前端 `useTauriEvent` 订阅）

- `app:ready` — 启动时版本号
- `connection:status` — disconnected / connecting / connected / error
- `market:ticker`, `market:depth`, `market:kline`
- `order:updated`, `position:updated`, `balance:updated`
- `error:occurred`, `log:entry`

### 主要 Tauri Commands

`ping`, `get_version`, `get_config`, `save_config`, `save_credentials`, `has_credentials`,
`connect`, `disconnect`, `test_connection`, `get_connection_status`,
`set_active_symbol`, `set_kline_interval`, `refresh_market`, `fetch_ticker`, `fetch_depth`, `fetch_klines`,
`place_order`, `cancel_order`, `refresh_orders`,
`refresh_account`, `refresh_balances`, `refresh_positions`,
`get_trade_stats`, `export_trade_log`, `save_window_size`

## Rust 模块结构（`src-tauri/src/`）

```
lib.rs, main.rs, state.rs, error.rs
models/          config, market, trading, account
auth/            signer (HMAC-SHA256 hex), time_sync
api/             client, endpoints, public, private, mapper, response
ws/              client (tokio-tungstenite, 重连循环)
storage/         config (TOML), credentials (keyring), cache, trade_log (CSV)
services/        connection, market, trading, account, risk, analytics
commands/        app, config, connection, market, trading, account
events/          emitter
plugin/          trait 空壳（预留扩展，未实现插件系统）
```

## 配置与凭据

- **用户配置 TOML**：`%APPDATA%/EasiFlux Desktop/config.toml`（snake_case，与旧 Python 客户端 schema 兼容）
- **凭据 Keyring**：service `easiflux_desktop_tauri`（与旧版 `easiflux_desktop` 不同，不自动共享）
- **默认 API**：`https://api.easicoin.io`

## 开发命令

```bash
cd G:/EasiFlux/EasiFlux-Desktop-Tauri
pnpm install
pnpm tauri dev          # 桌面开发
pnpm build              # 仅前端
pnpm lint               # ESLint
pnpm test               # Vitest
cd src-tauri && cargo test && cargo clippy
pnpm tauri build        # 发布构建
```

### 环境要求

- Node.js 20+, pnpm 9+
- Rust stable + Windows MSVC + [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/)
- **构建注意**：Cargo `target` 目录在 `G:/EasiFlux/EasiFlux-Desktop-Tauri/target`（`src-tauri/.cargo/config.toml`）。C: 盘空间不足时设置 `TMP`/`TEMP` 到 G: 盘，例如 `G:/EasiFlux/EasiFlux-Desktop-Tauri/tmp`

## 测试现状（截至初版完成）

| 检查 | 结果 |
|------|------|
| `pnpm build` | 通过 |
| `pnpm lint` | 0 error（有 vue 风格 warning） |
| `pnpm test` | 1 个前端测试通过 |
| `cargo test` | 6 个 Rust 单元测试通过（signer, response, risk 等） |
| `cargo clippy` | 通过，约 26 warnings（非 `-D warnings`） |

## 已实现功能

- [x] 四区交易 UI + 暗色主题
- [x] API 配置 / 测试连接 / 连接断开
- [x] REST 行情（ticker, kline, depth）+ WS 实时推送
- [x] 限价/市价下单、撤单、挂单/持仓/余额查询
- [x] 下单前风控（数量、日次数、限价偏离）
- [x] 日志面板、交易统计、CSV 订单日志
- [x] CI（`.github/workflows/ci.yml`）+ Release（`release.yml`）
- [x] 插件 trait 预留（`plugin/mod.rs`，无实际插件）

## 未实现 / 延后

- 多账户并行连接（与旧版一致：单连接切换）
- 策略引擎（旧版 GridStrategy 仅为占位）
- 面板拖拽/折叠（splitpanes）
- 旧版 keyring 凭据一键迁移
- Rust 独立 SDK crate 抽离
- clippy 零 warning

## 修改原则（给 Agent）

1. **只改本仓库**；不要动 `EasiFlux-Desktop` 与 `EasiFlux-SDK`（除非用户明确要求）
2. **不要嵌入 Python**；API 逻辑写在 Rust `api/` / `ws/`
3. **业务逻辑不放 Vue**；UI 只调 Commands + 听 Events，状态在 Pinia
4. **单文件职责单一**，避免大文件（目标 <200 行/模块）
5. **TypeScript strict**，避免 `any`
6. **不要擅自 git commit**，除非用户要求

## 参考实现（只读）

旧 Python 客户端中对应概念的可对照文件：

| 新概念 (Rust) | 旧参考 (Python) |
|---------------|-----------------|
| `api/mapper.rs` | `adapters/model_mapper.py` |
| `services/risk.rs` | `services/risk_manager.py` |
| `services/market.rs` | `services/market_manager.py` |
| `ws/client.rs` | `adapters/ws_adapter.py` |
| `storage/config.rs` | `storage/config_store.py` |
| Pinia stores | `core/state_store.py` |

协议对照：`G:\EasiFlux\EasiFlux-SDK\src\easiflux_sdk\`（`config.py`, `core/auth.py`, `websocket/`）

## 关键文件速查

```
src/App.vue                          根组件、事件订阅、自动连接
src/components/layout/AppShell.vue   四区布局
src/composables/useTauriCommand.ts   invoke 封装
src/composables/useTauriEvent.ts     listen 封装
src/types/models.ts                  与 Rust 对齐的 TS 类型
src-tauri/src/lib.rs                 Tauri 入口、命令注册
src-tauri/src/state.rs               AppState DI
src-tauri/tauri.conf.json            窗口 1400x900、identifier io.easiflux.desktop
README.md / CONTRIBUTING.md          用户与贡献者文档
```

## 迁移历史摘要

2026-06 从 `EasiFlux-Desktop`（PySide6）迁移至本仓库，按四阶段计划完成：

1. **Phase 1**：Tauri + Vue 脚手架、布局、ping/pong IPC
2. **Phase 2**：Rust auth/api/ws/storage、行情与连接
3. **Phase 3**：交易、账户、风控
4. **Phase 4**：暗色主题、日志/分析、CI/Release、旧仓库 README 更新

---

*最后更新：2026-06-30 — 与初版 Tauri 迁移完成状态同步*
