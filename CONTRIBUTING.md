# Contributing to EasiFlux Desktop (Tauri)

## Setup

```bash
pnpm install
```

Ensure Rust and Tauri prerequisites are installed for your platform.

## Commands

```bash
pnpm tauri dev      # run desktop app
pnpm lint           # ESLint
pnpm test           # Vitest
pnpm build          # frontend production build
cd src-tauri && cargo test && cargo clippy
```

## Code style

- TypeScript: `strict` mode, no `any`
- Rust: `cargo fmt`, clippy clean
- Keep modules small and single-purpose

## Pull requests

1. Fork and create a feature branch
2. Run lint and tests
3. Open PR against `main`

## Related repos

- Legacy client: `EasiFlux-Desktop` (Python/PySide6)
- SDK reference: `EasiFlux-SDK` (Python, not embedded here)
