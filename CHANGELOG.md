# Changelog

## [0.3.0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/compare/easiflux-desktop-tauri-v0.2.0...easiflux-desktop-tauri-v0.3.0) (2026-07-05)


### Features

* **api:** align REST and WebSocket with EasiFlux-SDK v0.3 ([25fbe6d](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/25fbe6d5f90eb3c8e4b46f8f470a298e052bc045))


### Bug Fixes

* **auth:** align SDK signing and fix market panel display ([8880a16](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/8880a16ed83882343582a69cca6b946e14bdde29))
* **ci:** satisfy void return type in refreshPostConnectData task ([928a20b](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/928a20bdefe5d2e7f8750ef02d32c8f1ccbab64d))
* **connection:** align save-and-connect with test credential path ([0d5f569](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0d5f56960a7dd0fcc13df9fb635eedd7c3527b88))
* **connection:** decouple API connect from WebSocket startup failures ([d2ab829](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d2ab829bdb449ce1088653892bad2f99b7c0e167))
* **market:** credentials, 24h pct semantics, and kline persistence ([b0c1ebe](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/b0c1ebe4108de800e3427245d5434688c656f7e0))
* **market:** harden polling and incremental K-line refresh ([8c7efd9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/8c7efd9cf564de0dfe8155b4547087d0d520abf7))
* **market:** merge WS ticker deltas and restore private panel data ([1a56c92](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/1a56c9246dbc5dc714d653ed4bc90c9de23cf814))
* **market:** restore post-login data and migrate K-line chart ([662d94a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/662d94a65e81d7f5a13aacc04e5376c8adb2f5d7))
* **market:** restore post-login ticker, account, and kline data ([0dcc8c5](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0dcc8c51f042162a31437e58fb426792ad522135))
* **trading:** default coin=USDT for private queries and harden klines ([802abc1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/802abc187aad6af45683af93a839a5d37fb9aa3d))
* **trading:** restore futures panels, private data, and analytics ([a6acbe9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/a6acbe98b2368289fd4430d7c97953ebad1d9a43))
* **trading:** restore private panels, analytics, and kline reset ([999cd19](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/999cd198dc02085d17abb5473ecd7b20a3edddd5))
* **trading:** unify private panel refresh and restore table display ([3bdc30a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/3bdc30acc2ced0e096328e77d0af1226964e4c4c))
* **ws:** align WebSocket flow with SDK and split connection status ([82033b0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/82033b037166f55c692582d459847af5d50faef1))
* **ws:** SDK v0.3 alignment and connection reliability ([1b16646](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/1b16646af316387e1887b452ed9b9845b91f560e))
