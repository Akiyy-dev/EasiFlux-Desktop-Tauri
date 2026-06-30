# EasiFlux Desktop (Tauri)

Modern cryptocurrency contract trading desktop client built with **Tauri 2**, **Rust**, and **Vue 3**.

> **For AI agents / new sessions:** read [AGENTS.md](AGENTS.md) first — it contains project memory, architecture decisions, and development conventions.

This is the next-generation client. The legacy Python/PySide6 client remains in [EasiFlux-Desktop](https://github.com/Akiyy-dev/EasiFlux-Desktop).

## Stack

| Layer | Technology |
|-------|------------|
| Desktop | Tauri 2 |
| Backend | Rust (tokio, reqwest, tokio-tungstenite) |
| Frontend | Vue 3, TypeScript, Vite, Pinia, Naive UI |
| Charts | ECharts |

API communication is implemented in Rust (protocol-aligned with [EasiFlux-SDK](https://github.com/Akiyy-dev/EasiFlux-SDK)); Python SDK is **not** embedded at runtime.

## Requirements

- Node.js 20+
- pnpm 9+
- Rust stable (with MSVC toolchain on Windows)
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

## Development

```bash
pnpm install
pnpm tauri dev
```

Frontend only (browser, IPC calls will fail):

```bash
pnpm dev
```

## Build

```bash
pnpm tauri build
```

Artifacts are written to `src-tauri/target/release/bundle/`.

## Testing

```bash
# Frontend
pnpm test
pnpm lint

# Rust
cd src-tauri && cargo test && cargo clippy -- -D warnings
```

## Project layout

```
src/                 Vue 3 frontend
src-tauri/src/       Rust core (api, ws, services, storage)
tests/               Frontend + Rust tests
```

## Configuration

User config: `%APPDATA%/EasiFlux Desktop/config.toml` (compatible schema with legacy client)

API credentials: OS keyring (`easiflux_desktop_tauri` service)

## License

MIT
