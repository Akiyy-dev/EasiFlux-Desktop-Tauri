# Chart Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在独立“图表”模块中提供一张全尺寸 KLineCharts Pro K 线图，并由 Rust 每 5 秒按脏状态持久化最多 10,000 根 K 线、用户绘图、指标偏好和每个交易对/周期的视口。

**Architecture:** Rust 作为本地持久化权威：`KlineStore` 管理增量 JSONL 与 10,000 根历史，`ChartStateStore` 管理原子 JSON 快照，`ChartWorkspaceService` 协调加载、revision 和部分成功。Vue 通过专用 load/save Commands 和一个原子市场上下文 Command 协作；版本锁定的 KLineCharts DOM/实例注册表桥接器取得核心 `Chart`，适配器负责绘图注册表、语义 pane、视口与指标快照，应用级唯一的纯 TypeScript autosave 控制器负责活动捕获源、5 秒脏检查、single-flight、旧键重试和立即补存。

**Tech Stack:** Tauri 2, Rust stable, Tokio, serde/serde_json, std::fs, Vue 3, TypeScript strict, Pinia, KLineCharts 9.8.12, @klinecharts/pro 0.1.1, Vitest, Vue Test Utils.

## Global Constraints

- 只修改 `G:\EasiFlux\EasiFlux-Desktop-Tauri`；旧 Desktop 与 Python SDK 仅可只读参考。
- 工作分支保持 `chart/createworkspace`。
- 使用 `klinecharts` `9.8.12` 和 `@klinecharts/pro` `0.1.1`；不引入 ECharts、SQLite、Python sidecar 或新的图表引擎。
- `@klinecharts/pro` 必须从 `^0.1.1` 固定为 `0.1.1`，Vite 必须去重 `klinecharts`，否则核心实例注册表桥接不可靠。
- Rust 应用本地数据目录是图表状态的唯一权威；不得继续以 `localStorage` 或 IndexedDB 保存图表配置。
- 每 5 秒进行脏检查；无变化不得调用保存命令或写盘。交易对、周期、页面切换以及正常关闭前立即补存。
- 每个规范化的“交易对 + 周期”最多保留最新 10,000 根有效 K 线；首次无范围加载返回最新 200 根，KLineCharts datafeed 按 `from` / `to` 读取完整本地范围。
- 绘图与视口按“交易对 + 周期”隔离；指标偏好全局保存。主题、语言和时区仍服从应用级配置。
- 图表工作区固定显示 KLineCharts Pro 完整内置绘图栏；顶部栏和左侧一级导航保留，二级侧栏、深度、下单、订单、持仓和分析面板不出现。
- 图表工作区与交易页共享同一 `market.activeSymbol` 和 `market.klineInterval`，包括 Pro 内置交易对/周期选择器触发的变化。
- KLineCharts 桥接失败时保持行情与 Pro 绘图栏可用，只禁用绘图/视口/指标持久化并限频报告一次兼容错误。
- 新文件职责单一，目标少于 200 行；测试较长时拆到 `tests/` 子模块。
- 每个任务遵循 TDD：先写聚焦失败测试并确认 RED，再实现最小完整行为并确认 GREEN。
- 使用 `apply_patch` 编辑文件，保留并排除当前凭据相关修改与 `.pnpm-store/`。
- 未经用户另行明确授权，不执行 `git add`、`git commit` 或 `git push`。各任务的提交命令仅为延后检查点。

## File Responsibility Map

### Rust

- `src-tauri/src/models/chart_workspace.rs`：跨 Tauri 模型、schema 常量、安全键和默认状态。
- `src-tauri/src/storage/chart_state_store.rs`：视图/偏好 JSON 的主文件、`.tmp`、`.bak` 原子持久化。
- `src-tauri/src/storage/kline_store.rs`：每键内存序列、revision、待写增量、范围读取、JSONL flush 和压缩。
- `src-tauri/src/services/chart_workspace.rs`：加载聚合、同键/全局锁、revision 冲突处理和三部分保存结果。
- `src-tauri/src/commands/chart_workspace.rs`：`load_chart_workspace`、`save_chart_workspace` 薄命令。
- `src-tauri/src/services/market.rs`：REST/WS K 线只合并到 `KlineStore` 缓冲，网络范围刷新返回本地+网络合并结果。
- `src-tauri/src/services/scheduler.rs`：独立于账户生命周期锁的 5 秒 K 线 flush 任务。
- `src-tauri/src/state.rs` / `src-tauri/src/lib.rs`：共享服务装配、命令注册和退出前最终 flush。

### Frontend

- `src/types/chartWorkspace.ts`：与 Rust camelCase 对齐的 TypeScript 类型和 JSON 值类型。
- `src/utils/chartWorkspace.ts`：默认值、规范化、稳定指纹与 K 线合并纯函数。
- `src/services/chartWorkspaceService.ts`：Tauri load/save/ranged fetch 调用封装。
- `src/services/chartWorkspaceFlushRegistry.ts`：持久化处理器注册表、唯一活动处理器指针及关闭时全量补存入口。
- `src/services/klineChartsCoreBridge.ts`：唯一依赖 Pro 私有 DOM 标记和 KLineCharts 注册表行为的模块。
- `src/services/klineChartsWorkspaceAdapter.ts`：核心 API 包装、绘图 ID 注册表、语义 pane、视口和指标快照。
- `src/services/chartWorkspaceAutosave.ts`：应用级唯一的 5 秒定时、活动捕获源、revision、fingerprint、single-flight、部分成功和旧键重试。
- `src/services/legacyChartSettingsMigration.ts`：现有 `easiflux.chart-settings.v1` 的一次性只读迁移；Rust 成功接管后删除旧键。
- `src/composables/useChartWorkspaceAutosaveHost.ts`：在 `AppShell` 创建/提供唯一 autosave，并把它注册为全局补存入口。
- `src/composables/easiKlineDatafeed.ts`：本地范围优先、REST 校准、Pro 选择器与共享 market 上下文同步。
- `src/composables/useChartWorkspaceCloseGuard.ts`：窗口关闭前异步补存，再销毁窗口。
- `src/composables/useKlineChartWorkspace.ts`：Pro/datafeed/适配器/autosave 的单实例生命周期和 generation 编排。
- `src/components/market/KlineChart.vue`：单 Pro 实例编排、恢复、实时更新、ResizeObserver 和激活状态。
- `src/components/chart/ChartWorkspacePage.vue`：只承载一张全尺寸工作区图表。
- `src/components/layout/AppShell.vue`：图表页路由、二级侧栏隐藏、页面切换前补存和单实例保活。

## Deferred Commit Map

以下只记录边界；没有用户授权时不得执行：

1. `feat(chart): define workspace persistence contract`
2. `feat(chart): add atomic workspace state storage`
3. `feat(chart): buffer and compact kline history`
4. `feat(market): support ranged chart history`
5. `feat(chart): expose workspace persistence service`
6. `feat(chart): synchronize chart datafeed context`
7. `feat(chart): add KLineCharts workspace adapter`
8. `feat(chart): add revisioned workspace autosave`
9. `feat(chart): add fullscreen chart workspace`

---

### Task 1: Lock the compatibility contract and shared models

**Files:**

- Create: `src-tauri/src/models/chart_workspace.rs`
- Modify: `src-tauri/src/models/mod.rs`
- Modify: `src-tauri/src/models/market.rs`
- Create: `src/types/chartWorkspace.ts`
- Create: `src/utils/chartWorkspace.ts`
- Create: `tests/frontend/chartWorkspaceTypes.test.ts`
- Create: `tests/frontend/helpers/chartWorkspaceFixtures.ts`
- Modify: `package.json`
- Modify: `pnpm-lock.yaml`
- Modify: `vite.config.ts`

**Interfaces:**

- Consumes: `models::config::KLINE_INTERVALS`, existing `models::market::Kline`.
- Produces: `ChartWorkspaceKey::parse`, all Rust/TypeScript snapshot types, `defaultChartViewState`, `defaultChartPreferences`, `stableChartFingerprint`, and an exact single-copy KLineCharts runtime contract used by every later task.

- [ ] **Step 1: Write failing Rust key, schema and serialization tests**

Add `#[cfg(test)] mod tests` to the new model file with these concrete cases:

```rust
#[test]
fn workspace_key_normalizes_symbol_and_accepts_supported_intervals() {
    let key = ChartWorkspaceKey::parse(" btcusdt ", "15").unwrap();
    assert_eq!(key.symbol, "BTCUSDT");
    assert_eq!(key.interval, "15");
}

#[test]
fn workspace_key_rejects_path_segments_and_unknown_intervals() {
    assert!(ChartWorkspaceKey::parse("../BTCUSDT", "15").is_err());
    assert!(ChartWorkspaceKey::parse("BTC/USDT", "15").is_err());
    assert!(ChartWorkspaceKey::parse("BTCUSDT", "2").is_err());
}

#[test]
fn pane_reference_and_workspace_snapshot_serialize_camel_case() {
    let value = serde_json::to_value(sample_snapshot()).unwrap();
    assert_eq!(value["viewState"]["schemaVersion"], 1);
    assert_eq!(
        value["viewState"]["overlays"][0]["pane"],
        serde_json::json!({ "kind": "indicator", "indicatorName": "MACD" })
    );
}

fn sample_snapshot() -> ChartWorkspaceSnapshot {
    let key = ChartWorkspaceKey::parse("BTCUSDT", "15").unwrap();
    ChartWorkspaceSnapshot {
        key: key.clone(),
        klines: Vec::new(),
        view_state: ChartViewStateV1 {
            schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
            symbol: key.symbol,
            interval: key.interval,
            revision: 1,
            saved_at_ms: 0,
            overlays: vec![ChartOverlaySnapshot {
                id: "segment-1".into(),
                group_id: "group-1".into(),
                pane: ChartPaneRef::Indicator { indicator_name: "MACD".into() },
                name: "segment".into(),
                lock: false,
                visible: true,
                z_level: 0,
                mode: "normal".into(),
                mode_sensitivity: 8.0,
                points: vec![ChartPointSnapshot {
                    timestamp: Some(1_000),
                    data_index: None,
                    value: Some(1.0),
                }],
                extend_data: serde_json::Value::Null,
                styles: serde_json::Value::Null,
            }],
            viewport: ChartViewportSnapshot::default(),
        },
        preferences: ChartPreferencesV1 {
            schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
            revision: 1,
            saved_at_ms: 0,
            main_indicators: vec!["MA".into(), "EMA".into()],
            sub_indicators: vec!["VOL".into(), "MACD".into()],
        },
    }
}
```

- [ ] **Step 2: Run the Rust model test and confirm RED**

Run from `src-tauri`:

```powershell
cargo test models::chart_workspace::tests -- --nocapture
```

Expected: compilation fails because `models::chart_workspace` and its types do not exist.

- [ ] **Step 3: Implement the Rust contract and validation**

Define these exact public shapes with `#[serde(rename_all = "camelCase")]`; the tagged pane enum must serialize exactly as shown in Step 1:

```rust
pub const CHART_WORKSPACE_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_CHART_LOAD_LIMIT: usize = 200;
pub const MAX_CHART_KLINES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceKey {
    pub symbol: String,
    pub interval: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChartPaneRef {
    Candle,
    Indicator { indicator_name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPointSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartOverlaySnapshot {
    pub id: String,
    pub group_id: String,
    pub pane: ChartPaneRef,
    pub name: String,
    pub lock: bool,
    pub visible: bool,
    pub z_level: i32,
    pub mode: String,
    pub mode_sensitivity: f64,
    pub points: Vec<ChartPointSnapshot>,
    pub extend_data: serde_json::Value,
    pub styles: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartViewportSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar_space: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_timestamp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartViewStateV1 {
    pub schema_version: u32,
    pub symbol: String,
    pub interval: String,
    pub revision: u64,
    pub saved_at_ms: i64,
    pub overlays: Vec<ChartOverlaySnapshot>,
    pub viewport: ChartViewportSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPreferencesV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub saved_at_ms: i64,
    pub main_indicators: Vec<String>,
    pub sub_indicators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceSnapshot {
    pub key: ChartWorkspaceKey,
    pub klines: Vec<Kline>,
    pub view_state: ChartViewStateV1,
    pub preferences: ChartPreferencesV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveChartWorkspaceRequest {
    pub key: ChartWorkspaceKey,
    #[serde(default)]
    pub view_state: Option<ChartViewStateV1>,
    #[serde(default)]
    pub preferences: Option<ChartPreferencesV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceSaveResult {
    pub key: ChartWorkspaceKey,
    pub view_revision: u64,
    pub preferences_revision: u64,
    pub saved_at_ms: i64,
    pub kline_saved: bool,
    pub view_state_saved: bool,
    pub preferences_saved: bool,
    pub kline_error: Option<String>,
    pub view_state_error: Option<String>,
    pub preferences_error: Option<String>,
}
```

`ChartWorkspaceKey::parse` trims and uppercases the symbol, requires `1..=64` characters containing only ASCII uppercase letters, digits, `_` or `-`, trims the interval, and requires membership in `KLINE_INTERVALS`. Add `PartialEq, Eq` to `Kline` so duplicate REST/WS bars do not advance a dirty revision. Default view/preferences use schema 1, revision 0, `savedAtMs = 0`, empty overlays, empty viewport, main indicators `MA, EMA`, and sub indicators `VOL, MACD`.

- [ ] **Step 4: Rerun the Rust model test and confirm GREEN**

```powershell
cargo test models::chart_workspace::tests -- --nocapture
```

Expected: all model tests pass.

- [ ] **Step 5: Write failing TypeScript normalization and fingerprint tests**

Create `tests/frontend/chartWorkspaceTypes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import {
  chartPreferencesContentFingerprint,
  chartViewContentFingerprint,
  defaultChartPreferences,
  defaultChartViewState,
  normalizeChartWorkspaceKey,
  stableChartFingerprint,
} from '../../src/utils/chartWorkspace'
import { segment } from './helpers/chartWorkspaceFixtures'

describe('chart workspace contract', () => {
  it('normalizes safe keys and rejects unsupported values', () => {
    expect(normalizeChartWorkspaceKey({ symbol: ' btcusdt ', interval: '15' }))
      .toEqual({ symbol: 'BTCUSDT', interval: '15' })
    expect(() => normalizeChartWorkspaceKey({ symbol: '../BTC', interval: '15' })).toThrow()
    expect(() => normalizeChartWorkspaceKey({ symbol: 'BTCUSDT', interval: '2' })).toThrow()
  })

  it('creates independent version-one defaults', () => {
    const first = defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' })
    const second = defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' })
    first.overlays.push(segment())
    expect(second.overlays).toEqual([])
    expect(defaultChartPreferences().mainIndicators).toEqual(['MA', 'EMA'])
  })

  it('fingerprints normalized objects independently of object key order', () => {
    expect(stableChartFingerprint({ b: 2, a: 1 }))
      .toBe(stableChartFingerprint({ a: 1, b: 2 }))
  })

  it('excludes persistence metadata from content fingerprints', () => {
    const firstView = { ...defaultChartViewState({ symbol: 'BTCUSDT', interval: '15' }), revision: 1, savedAtMs: 10 }
    const secondView = { ...firstView, revision: 8, savedAtMs: 99 }
    const firstPreferences = { ...defaultChartPreferences(), revision: 2, savedAtMs: 20 }
    const secondPreferences = { ...firstPreferences, revision: 9, savedAtMs: 100 }
    expect(chartViewContentFingerprint(firstView)).toBe(chartViewContentFingerprint(secondView))
    expect(chartPreferencesContentFingerprint(firstPreferences))
      .toBe(chartPreferencesContentFingerprint(secondPreferences))
  })
})
```

The shared `segment()` fixture below supplies the complete `ChartOverlaySnapshot` with finite values and JSON-safe fields.

- [ ] **Step 6: Run the TypeScript test and confirm RED**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceTypes.test.ts
```

Expected: import resolution fails because the TypeScript contract and utilities do not exist.

- [ ] **Step 7: Implement the TypeScript mirror and pure utilities**

Create exact camelCase counterparts in `src/types/chartWorkspace.ts`, including:

```ts
export type JsonValue = null | boolean | number | string | JsonValue[] | {
  [key: string]: JsonValue
}

export type ChartPaneRef =
  | { kind: 'candle' }
  | { kind: 'indicator'; indicatorName: string }

export interface SaveChartWorkspaceRequest {
  key: ChartWorkspaceKey
  viewState: ChartViewStateV1 | null
  preferences: ChartPreferencesV1 | null
}
```

Mirror every Rust field exactly: optional snapshot point/viewport fields use `?:`, and result error fields use `string | null` unless their Rust fields also add `skip_serializing_if`. In `src/utils/chartWorkspace.ts`, export:

```ts
export const SUPPORTED_CHART_INTERVALS = new Set(['1', '5', '15', '60', '240', 'D'])

export function normalizeChartWorkspaceKey(key: ChartWorkspaceKey): ChartWorkspaceKey
export function defaultChartViewState(key: ChartWorkspaceKey): ChartViewStateV1
export function defaultChartPreferences(): ChartPreferencesV1
export function toJsonValue(value: unknown): JsonValue
export function stableChartFingerprint(value: unknown): string
export function chartViewContentFingerprint(view: ChartViewStateV1): string
export function chartPreferencesContentFingerprint(preferences: ChartPreferencesV1): string
export function mergeKlinesByOpenTime(...groups: Kline[][]): Kline[]
```

`toJsonValue` converts unsupported values, circular references and non-finite numbers to `null`; it never throws. `stableChartFingerprint` recursively sorts object keys but preserves array order. The two content fingerprint helpers destructure away `revision` and `savedAtMs` before fingerprinting so a successful save cannot make an unchanged snapshot dirty again. `mergeKlinesByOpenTime` filters invalid timestamps, overwrites duplicates with the later group, sorts ascending, and retains the newest 10,000.

Create the shared frontend fixture module used verbatim by later tasks:

```ts
export const key = (symbol = 'BTCUSDT', interval = '1'): ChartWorkspaceKey => ({
  symbol,
  interval,
})

export const bar = (openTime: number, symbol = 'BTCUSDT', interval = '1'): Kline => ({
  symbol,
  interval,
  openTime,
  open: '1',
  high: '2',
  low: '0.5',
  close: '1.5',
  volume: '10',
})

export const segment = (id = 'segment-1'): ChartOverlaySnapshot => ({
  id,
  groupId: 'group-1',
  pane: { kind: 'candle' },
  name: 'segment',
  lock: false,
  visible: true,
  zLevel: 0,
  mode: 'normal',
  modeSensitivity: 8,
  points: [{ timestamp: 1_000, value: 1 }],
  extendData: null,
  styles: null,
})

export function viewStateFor(
  workspaceKey: ChartWorkspaceKey,
  revision = 0,
  overlays: ChartOverlaySnapshot[] = [],
): ChartViewStateV1 {
  return {
    schemaVersion: 1,
    symbol: workspaceKey.symbol,
    interval: workspaceKey.interval,
    revision,
    savedAtMs: 0,
    overlays,
    viewport: {},
  }
}

export const viewState = (revision = 0, overlays: ChartOverlaySnapshot[] = []) =>
  viewStateFor(key(), revision, overlays)

export const samplePreferences = (revision = 0): ChartPreferencesV1 => ({
  schemaVersion: 1,
  revision,
  savedAtMs: 0,
  mainIndicators: ['MA', 'EMA'],
  subIndicators: ['VOL', 'MACD'],
})

export const sampleSnapshot = (
  klines: Kline[] = [],
  workspaceKey = key(),
): ChartWorkspaceSnapshot => ({
  key: workspaceKey,
  klines,
  viewState: viewStateFor(workspaceKey),
  preferences: samplePreferences(),
})

export const sampleSaveRequest = (
  patch: Partial<SaveChartWorkspaceRequest> = {},
): SaveChartWorkspaceRequest => ({
  key: key(),
  viewState: viewState(),
  preferences: samplePreferences(),
  ...patch,
})

export const successfulSave = (
  viewRevision = 0,
  preferencesRevision = 0,
): ChartWorkspaceSaveResult => ({
  key: key(),
  viewRevision,
  preferencesRevision,
  savedAtMs: 1_000,
  klineSaved: true,
  viewStateSaved: true,
  preferencesSaved: true,
  klineError: null,
  viewStateError: null,
  preferencesError: null,
})

export function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  let settled = false
  const promise = new Promise<T>((res, rej) => {
    resolve = (value) => { settled = true; res(value) }
    reject = (reason) => { settled = true; rej(reason) }
  })
  return { promise, resolve, reject, get settled() { return settled } }
}
```

Import all referenced types from `src/types/chartWorkspace` and `Kline` from `src/types/models`; tests import helpers from this module instead of redefining ambiguous fixtures.

- [ ] **Step 8: Pin the bridge dependencies and rerun focused checks**

Change the importer specifier in both `package.json` and `pnpm-lock.yaml`:

```json
"@klinecharts/pro": "0.1.1",
"klinecharts": "9.8.12"
```

Add Vite deduplication without removing the existing alias:

```ts
resolve: {
  alias: {
    '@': path.resolve(__dirname, './src'),
  },
  dedupe: ['klinecharts'],
},
```

Run:

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceTypes.test.ts
.\node_modules\.bin\vue-tsc.CMD --noEmit
```

Expected: the focused test and strict typecheck pass without downloading dependencies.

- [ ] **Step 9: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add package.json pnpm-lock.yaml vite.config.ts src-tauri/src/models/chart_workspace.rs src-tauri/src/models/mod.rs src-tauri/src/models/market.rs src/types/chartWorkspace.ts src/utils/chartWorkspace.ts tests/frontend/chartWorkspaceTypes.test.ts tests/frontend/helpers/chartWorkspaceFixtures.ts
git commit -m "feat(chart): define workspace persistence contract"
```

### Task 2: Add atomic chart view and preference storage

**Files:**

- Create: `src-tauri/src/storage/chart_state_store.rs`
- Create: `src-tauri/src/storage/chart_state_store/tests.rs`
- Modify: `src-tauri/src/storage/mod.rs`

**Interfaces:**

- Consumes: `ChartWorkspaceKey`, `ChartViewStateV1`, `ChartPreferencesV1` from Task 1.
- Produces: `ChartStateStore::{load_view, save_view, load_preferences, save_preferences}` for Task 5.

- [ ] **Step 1: Write failing atomic storage and recovery tests**

Add the following named cases to `src-tauri/src/storage/chart_state_store/tests.rs` using a unique `std::env::temp_dir()` root made from process id plus `AtomicU64`:

```rust
#[test]
fn view_state_round_trips_under_the_normalized_key() {
    let root = test_root("view-round-trip");
    let store = ChartStateStore::with_root(root.clone());
    let state = sample_view("BTCUSDT", "15", 3);
    store.save_view(&state).unwrap();
    assert_eq!(store.load_view(&key("BTCUSDT", "15")).unwrap(), Some(state));
    cleanup(&root);
}

#[test]
fn preferences_are_global_and_independent_of_view_files() {
    let root = test_root("preferences");
    let store = ChartStateStore::with_root(root.clone());
    let preferences = sample_preferences(4);
    store.save_preferences(&preferences).unwrap();
    assert_eq!(store.load_preferences().unwrap(), Some(preferences));
    assert!(!root.join("views").join("preferences.v1.json").exists());
    cleanup(&root);
}

#[test]
fn corrupt_main_recovers_valid_backup() {
    let root = test_root("backup-recovery");
    let store = ChartStateStore::with_root(root.clone());
    store.save_view(&sample_view("BTCUSDT", "15", 1)).unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 2)).unwrap();
    std::fs::write(store.view_path_for_test(&key("BTCUSDT", "15")), "not json").unwrap();
    assert_eq!(store.load_view(&key("BTCUSDT", "15")).unwrap().unwrap().revision, 1);
    cleanup(&root);
}

#[test]
fn all_corrupt_candidates_return_storage_error() {
    let root = test_root("all-corrupt");
    let store = ChartStateStore::with_root(root.clone());
    corrupt_main_temp_and_backup(&store, &key("BTCUSDT", "15"));
    assert!(matches!(store.load_view(&key("BTCUSDT", "15")), Err(AppError::Storage(_))));
    cleanup(&root);
}

#[test]
fn save_after_backup_recovery_preserves_a_last_good_candidate() {
    let root = test_root("save-after-recovery");
    let store = ChartStateStore::with_root(root.clone());
    store.save_view(&sample_view("BTCUSDT", "15", 1)).unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 2)).unwrap();
    std::fs::write(store.view_path_for_test(&key("BTCUSDT", "15")), "not json").unwrap();
    store.save_view(&sample_view("BTCUSDT", "15", 3)).unwrap();
    assert_eq!(store.load_view(&key("BTCUSDT", "15")).unwrap().unwrap().revision, 3);
    assert_eq!(store.load_backup_view_for_test(&key("BTCUSDT", "15")).unwrap().revision, 1);
    cleanup(&root);
}
```

Use these concrete test helpers; every path is below the unique temporary root:

```rust
static TEST_ROOT_ID: AtomicU64 = AtomicU64::new(0);

fn test_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "easiflux-chart-state-{}-{}-{}",
        std::process::id(),
        TEST_ROOT_ID.fetch_add(1, Ordering::Relaxed),
        label,
    ))
}

fn cleanup(root: &Path) {
    let _ = std::fs::remove_dir_all(root);
}

fn key(symbol: &str, interval: &str) -> ChartWorkspaceKey {
    ChartWorkspaceKey::parse(symbol, interval).unwrap()
}

fn sample_view(symbol: &str, interval: &str, revision: u64) -> ChartViewStateV1 {
    let key = key(symbol, interval);
    ChartViewStateV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        symbol: key.symbol,
        interval: key.interval,
        revision,
        saved_at_ms: 0,
        overlays: Vec::new(),
        viewport: ChartViewportSnapshot::default(),
    }
}

fn sample_preferences(revision: u64) -> ChartPreferencesV1 {
    ChartPreferencesV1 {
        schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
        revision,
        saved_at_ms: 0,
        main_indicators: vec!["MA".into(), "EMA".into()],
        sub_indicators: vec!["VOL".into(), "MACD".into()],
    }
}

fn corrupt_main_temp_and_backup(store: &ChartStateStore, key: &ChartWorkspaceKey) {
    for path in [
        store.view_path_for_test(key),
        store.view_temp_path_for_test(key),
        store.view_backup_path_for_test(key),
    ] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "not json").unwrap();
    }
}
```

Add four named cases: `missing_files_return_none` asserts `load_view` and `load_preferences` are `Ok(None)`; `unknown_schema_is_rejected` writes schema `2` and asserts `AppError::Storage`; `embedded_key_mismatch_is_rejected` writes an ETH view under the BTC path and asserts `AppError::Storage`; `failed_main_promotion_keeps_a_readable_candidate` injects a replacement failure after temp sync and asserts a subsequent load returns the last good main or backup.

- [ ] **Step 2: Run the storage tests and confirm RED**

Run from `src-tauri`:

```powershell
cargo test storage::chart_state_store::tests -- --nocapture
```

Expected: compilation fails because `ChartStateStore` does not exist.

- [ ] **Step 3: Implement the store with Windows-safe replacement**

Expose this interface:

```rust
pub struct ChartStateStore {
    root: PathBuf,
}

impl ChartStateStore {
    pub fn new() -> Self;
    pub(crate) fn with_root(root: PathBuf) -> Self;
    pub fn load_view(&self, key: &ChartWorkspaceKey) -> AppResult<Option<ChartViewStateV1>>;
    pub fn save_view(&self, state: &ChartViewStateV1) -> AppResult<()>;
    pub fn load_preferences(&self) -> AppResult<Option<ChartPreferencesV1>>;
    pub fn save_preferences(&self, preferences: &ChartPreferencesV1) -> AppResult<()>;
}
```

Under `#[cfg(test)]`, define `enum StateFileKind { View(ChartWorkspaceKey), Preferences }` and add `with_root_and_write_hook(root, Arc<dyn Fn(StateFileKind) + Send + Sync>)`; invoke it immediately before a view/preferences temp write. Also expose `view_path_for_test`, `view_temp_path_for_test`, `view_backup_path_for_test`, `preferences_path_for_test`, and `load_backup_view_for_test`. Service concurrency tests use barriers on the write hook to prove same-view and global-preference serialization. The hook field/call is compiled out of release builds.

`new()` resolves `dirs::data_local_dir()/APP_NAME/chart_workspace`. Use these exact locations:

```text
chart_workspace/preferences.v1.json
chart_workspace/views/BTCUSDT_15.v1.json
```

Declare `#[cfg(test)] mod tests;` from `chart_state_store.rs` so the planned submodule is compiled. For each save, first validate whether main and backup are usable, then follow this Windows-safe state machine:

```rust
let bytes = serde_json::to_vec_pretty(value)
    .map_err(|error| AppError::Storage(error.to_string()))?;
write_and_sync(&temp_path, &bytes)?;
if main_is_valid {
    remove_old_backup_if_present(&backup_path)?;
    rename_main_to_backup(&main_path, &backup_path)?;
} else {
    remove_invalid_main_if_present(&main_path)?; // keep a valid backup untouched
}
rename_temp_to_main_or_restore_backup(&temp_path, &main_path, &backup_path)?;
```

Map every filesystem/JSON failure to `AppError::Storage`. Load candidates in order `main -> temp -> backup`; return the first JSON value that passes schema and embedded-key validation. If at least one candidate exists but none is valid, return `AppError::Storage`; missing all candidates returns `Ok(None)`. A corrupt main must never replace the only valid backup during the next save. Sync the containing directory after renames on platforms where the existing project persistence helper supports it.

- [ ] **Step 4: Run focused storage tests and formatting**

```powershell
cargo test storage::chart_state_store::tests -- --nocapture
cargo fmt -- --check
```

Expected: all chart state storage tests pass and formatting is clean.

- [ ] **Step 5: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src-tauri/src/storage/chart_state_store.rs src-tauri/src/storage/chart_state_store/tests.rs src-tauri/src/storage/mod.rs
git commit -m "feat(chart): add atomic workspace state storage"
```

### Task 3: Convert KlineStore to a revisioned 10,000-bar append log

**Files:**

- Modify: `src-tauri/src/storage/kline_store.rs`

**Interfaces:**

- Consumes: safe `ChartWorkspaceKey`, `Kline: PartialEq + Eq`, existing `<symbol>_<interval>.jsonl` files.
- Produces: buffered upsert, bounded range reads, per-key/all-key flush results and crash-tolerant compaction used by MarketService and ChartWorkspaceService.

- [ ] **Step 1: Add failing buffer, range and compaction tests**

Keep the existing gap test and add these exact behaviors under `storage::kline_store::tests`:

```rust
#[test]
fn equal_bar_does_not_advance_revision_or_write_before_flush() {
    let dir = test_dir("no-op");
    let store = KlineStore::with_dir(dir.clone());
    let key = key("BTCUSDT", "1");
    let first = store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
    let second = store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
    assert!(first.changed);
    assert!(!second.changed);
    assert_eq!(first.revision, second.revision);
    assert!(!dir.join("BTCUSDT_1.jsonl").exists());
}

#[test]
fn load_range_filters_sorts_and_keeps_the_newest_limit() {
    let store = seeded_store("range", 20);
    let bars = store.load_range(&key("BTCUSDT", "1"), Some(5_000), Some(15_000), 4).unwrap();
    assert_eq!(bars.iter().map(|bar| bar.open_time).collect::<Vec<_>>(), vec![12_000, 13_000, 14_000, 15_000]);
}

#[test]
fn store_keeps_latest_ten_thousand_unique_bars() {
    let store = KlineStore::with_dir(test_dir("limit"));
    let bars = (1..=10_250).map(|time| sample(time, "1")).collect::<Vec<_>>();
    store.upsert_bars(&key("BTCUSDT", "1"), &bars).unwrap();
    let loaded = store.load_range(&key("BTCUSDT", "1"), None, None, 20_000).unwrap();
    assert_eq!(loaded.len(), 10_000);
    assert_eq!(loaded.first().unwrap().open_time, 251);
}

#[test]
fn update_arriving_after_flush_snapshot_remains_dirty() {
    let store = KlineStore::with_dir(test_dir("racing-update"));
    let key = key("BTCUSDT", "1");
    store.upsert_bars(&key, &[sample(1_000, "1")]).unwrap();
    let batch = store.prepare_flush_for_test(&key).unwrap().unwrap();
    store.upsert_bars(&key, &[sample(1_000, "2")]).unwrap();
    store.acknowledge_flush_for_test(&batch);
    assert!(store.is_dirty_for_test(&key));
}

#[test]
fn concurrent_flushes_for_same_key_write_one_batch() {
    let fixture = blocking_flush_fixture("same-key-single-flight");
    fixture.store.upsert_bars(&fixture.key, &[sample(1_000, "1")]).unwrap();
    let first = fixture.spawn_flush();
    fixture.wait_until_first_flush_reaches_disk_hook();
    let second = fixture.spawn_flush();
    assert!(!fixture.second_flush_reached_disk_hook());
    fixture.release_first_flush();
    first.join().unwrap().unwrap();
    second.join().unwrap().unwrap();
    assert_eq!(fixture.physical_valid_lines(), 1);
}
```

`blocking_flush_fixture` uses a `#[cfg(test)] KlineStore::with_dir_and_flush_hook` callback plus barriers and blocks only on `FlushPhase::DiskWriteStarted`; no hook field or branch exists in release builds. Add the named cases `malformed_lines_are_skipped_but_counted`, `flush_appends_only_changed_bars`, `physical_threshold_triggers_compaction`, `compaction_keeps_newest_ten_thousand_unique_bars`, `explicit_and_periodic_compaction_serialize_per_key`, and `flush_dirty_attempts_keys_after_a_failure`. The assertions respectively check valid load contents plus physical count, exact JSONL line count, `outcome.compacted`, first/last retained timestamps, a maximum of one simultaneous disk hook for the key, and a success result for the second key after the first key's storage error. Add `mismatched_bar_key_is_rejected_without_mutation` and `mismatched_persisted_lines_are_skipped`, proving a BTC/1 entry can never ingest or return an embedded ETH/15 bar.

- [ ] **Step 2: Run the focused KlineStore tests and confirm RED**

Run from `src-tauri`:

```powershell
cargo test storage::kline_store::tests -- --nocapture
```

Expected: compilation fails because the buffered interfaces and test seams do not exist.

- [ ] **Step 3: Implement per-key buffered state and revision acknowledgment**

Use these public result shapes and methods:

```rust
pub const MAX_STORED_BARS: usize = 10_000;
const COMPACT_PHYSICAL_RECORDS: usize = 12_000;

#[derive(Debug, Clone)]
pub struct KlineMergeResult {
    pub revision: u64,
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub struct KlineFlushOutcome {
    pub revision: u64,
    pub wrote: bool,
    pub compacted: bool,
}

impl KlineStore {
    pub fn new() -> Self;
    pub(crate) fn with_dir(dir: PathBuf) -> Self;
    pub fn upsert_bars(
        &self,
        key: &ChartWorkspaceKey,
        bars: &[Kline],
    ) -> AppResult<KlineMergeResult>;
    pub fn load_range(
        &self,
        key: &ChartWorkspaceKey,
        from: Option<i64>,
        to: Option<i64>,
        limit: usize,
    ) -> AppResult<Vec<Kline>>;
    pub fn last_open_time(&self, key: &ChartWorkspaceKey) -> AppResult<Option<i64>>;
    pub fn flush_key(&self, key: &ChartWorkspaceKey) -> AppResult<KlineFlushOutcome>;
    pub fn flush_dirty(&self) -> Vec<(ChartWorkspaceKey, AppResult<KlineFlushOutcome>)>;
}
```

Add these exact test-only seams under `#[cfg(test)]`; they expose immutable snapshots only and do not introduce production fault branches:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushPhase {
    DiskWriteStarted,
}

pub(crate) fn prepare_flush_for_test(
    &self,
    key: &ChartWorkspaceKey,
) -> AppResult<Option<FlushBatch>>;
pub(crate) fn acknowledge_flush_for_test(&self, batch: &FlushBatch);
pub(crate) fn is_dirty_for_test(&self, key: &ChartWorkspaceKey) -> bool;
pub(crate) fn with_dir_and_flush_hook(
    dir: PathBuf,
    hook: Arc<dyn Fn(&ChartWorkspaceKey, FlushPhase) + Send + Sync>,
) -> Self;
```

Internally keep a synchronized registry and use only `std::sync` locks because every storage method is synchronous:

```rust
pub struct KlineStore {
    dir: PathBuf,
    entries: RwLock<HashMap<ChartWorkspaceKey, Arc<BufferedKeyState>>>,
}

struct BufferedKeyState {
    series: Mutex<BufferedSeries>,
    flush_lock: Mutex<()>,
}
```

Initialize each entry exactly once under the registry write lock, then lazily load its file exactly once. Each pending entry stores both `Kline` and the revision that created it. `prepare_flush` first acquires `flush_lock`, snapshots pending entries plus a `snapshot_revision`, and drops only the series mutex before disk I/O. The flush mutex remains held until disk I/O and acknowledgment finish, so scheduler, context-switch and explicit-save flushes for the same key cannot append or compact concurrently. After successful append/compaction, acknowledgment removes only entries whose revision is not newer than `snapshot_revision`; a same-timestamp WS update arriving during I/O remains pending. Clamp all public read limits to `1..=10_000`.

Load existing JSONL lazily, count every physical line, skip malformed/blank records and records whose embedded symbol/interval do not equal the requested key, deduplicate by `open_time`, and crop the in-memory `BTreeMap` to 10,000. Before mutating an upsert batch, reject any embedded key mismatch and ignore non-positive timestamps. Map filesystem/serialization failures explicitly to `AppError::Storage` rather than relying on the project's general `io::Error -> Internal` conversion. If main is absent/empty after an interrupted compaction, try a fully parseable `.tmp`, then `.bak`; mark a recovered fallback dirty for a canonical compaction even when no new bar arrives. Likewise, loading more than 12,000 physical records sets `needs_compaction` so the scheduler can clean the file without a new market update. A changed bar replaces the same timestamp and advances revision once; a byte-for-byte equal bar is a no-op. `load_range` treats `from` and `to` as inclusive millisecond bounds and rejects `from > to`. Remove pending entries that are evicted by the 10,000-bar crop, and retain `needs_compaction` when an upsert crossed that boundary so obsolete pending bars are never appended during the next flush.

- [ ] **Step 4: Implement append flush and threshold compaction**

Normal flush uses append mode and syncs before acknowledgment:

```rust
let mut file = OpenOptions::new().create(true).append(true).open(&path)
    .map_err(storage_error)?;
for pending in &batch.entries {
    serde_json::to_writer(&mut file, &pending.kline)
        .map_err(|error| AppError::Storage(error.to_string()))?;
    file.write_all(b"\n").map_err(storage_error)?;
}
file.sync_all().map_err(storage_error)?;
```

Compact when physical records exceed 12,000 or an upsert temporarily exceeds 10,000 unique bars. Write the sorted/cropped full snapshot to `.tmp`, `sync_all`, rotate only a valid main file to `.bak`, and promote `.tmp`; retain/restore the last valid `.bak` on promotion failure. Existing `klines/<symbol>_<interval>.jsonl` paths and contents remain readable without migration. Tests verify the flushed file is immediately readable; the implementation review verifies the explicit `sync_all` call because plain `std::fs::File` offers no reliable sync spy.

- [ ] **Step 5: Run the complete KlineStore test group**

```powershell
cargo test storage::kline_store::tests -- --nocapture
cargo fmt -- --check
```

Expected: all old and new KlineStore tests pass, including the simulated concurrent update.

- [ ] **Step 6: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src-tauri/src/storage/kline_store.rs
git commit -m "feat(chart): buffer and compact kline history"
```

### Task 4: Decouple market updates from disk and support ranged REST history

**Files:**

- Modify: `src-tauri/src/services/market.rs`
- Modify: `src-tauri/src/commands/market.rs`
- Modify: `src-tauri/src/commands/connection.rs`
- Modify: `src-tauri/src/state.rs`

**Interfaces:**

- Consumes: Task 3 `KlineStore::{upsert_bars, load_range}` and existing `PublicApi::klines(client, symbol, interval, limit, start, end)`.
- Produces: display-limited buffering, backward-compatible `fetch_klines` range arguments, and atomic `set_chart_context` used by the frontend in Task 6.

- [ ] **Step 1: Write failing market buffering tests**

Extend `services::market::tests` with a pure storage seam so tests do not construct a Tauri `EventEmitter`:

```rust
#[test]
fn market_update_marks_store_dirty_without_immediate_disk_write() {
    let dir = test_dir("market-buffer");
    let store = KlineStore::with_dir(dir.clone());
    let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
    let update = buffer_display_klines(&store, &key, &[sample_kline(1_000, "2")]).unwrap();
    assert_eq!(update.display.len(), 1);
    assert!(update.changed);
    assert!(!dir.join("BTCUSDT_1.jsonl").exists());
}

#[test]
fn market_display_remains_limited_to_latest_two_hundred() {
    let store = KlineStore::with_dir(test_dir("display-limit"));
    let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
    let updates = (1..=240)
        .map(|time| sample_kline(time, &time.to_string()))
        .collect::<Vec<_>>();
    let update = buffer_display_klines(&store, &key, &updates).unwrap();
    assert_eq!(update.display.len(), 200);
    assert_eq!(update.display.first().unwrap().open_time, 41);
}
```

Keep all existing `merge_kline_updates` and interval tests.

- [ ] **Step 2: Run the market tests and confirm RED**

Run from `src-tauri`:

```powershell
cargo test services::market::tests -- --nocapture
```

Expected: compilation fails because `buffer_display_klines` and the new store interface are not wired.

- [ ] **Step 3: Replace `persist_and_emit` with buffer-and-emit**

Implement:

```rust
const MAX_DISPLAY_KLINES: usize = 200;
const MAX_RANGE_FETCH_KLINES: u32 = 500;

struct BufferedKlineUpdate {
    display: Vec<Kline>,
    changed: bool,
    needs_backfill: bool,
}

fn buffer_display_klines(
    store: &KlineStore,
    key: &ChartWorkspaceKey,
    bars: &[Kline],
) -> AppResult<BufferedKlineUpdate> {
    let merge = store.upsert_bars(key, bars)?;
    let display = store.load_range(key, None, None, MAX_DISPLAY_KLINES)?;
    let needs_backfill = !KlineStore::detect_gaps(&display, interval_to_ms(&key.interval)).is_empty();
    Ok(BufferedKlineUpdate { display, changed: merge.changed, needs_backfill })
}
```

Refactor `restore_klines`, `backfill_gaps`, `merge_and_emit_klines` and the latest-history path to:

1. build a validated `ChartWorkspaceKey`;
2. merge into the Rust buffer;
3. cache and emit only the newest 200 bars when `changed` is true;
4. never flush or rewrite a file inside a REST/WS update callback.

Use `KlineStore::load_range(..., 200)` rather than `CacheStore` as the gap-detection source so the display cache never becomes the persistence authority and a WS tick never clones all 10,000 persisted bars. If an update is unchanged, do not emit another identical snapshot.

- [ ] **Step 4: Extend ranged network refresh without breaking existing callers**

Add this MarketService method:

```rust
pub async fn fetch_kline_range(
    &self,
    key: &ChartWorkspaceKey,
    start: Option<i64>,
    end: Option<i64>,
    limit: Option<u32>,
) -> AppResult<Vec<Kline>> {
    let requested = limit.unwrap_or(DEFAULT_KLINE_LIMIT).clamp(1, MAX_RANGE_FETCH_KLINES);
    let rest = PublicApi::klines(
        &self.api,
        &key.symbol,
        &key.interval,
        requested,
        start,
        end,
    ).await?;
    let update = buffer_display_klines(&self.kline_store, key, &rest)?;
    if update.changed {
        self.cache.set_klines(&key.symbol, &key.interval, update.display.clone());
        self.emitter.emit_klines(&update.display);
    }
    self.kline_store.load_range(key, start, end, requested as usize)
}
```

The snippet defines behavior, but the implementation must not execute the synchronous store calls directly on the async worker. Clone the store/key/REST bars into one `tauri::async_runtime::spawn_blocking` closure that performs `buffer_display_klines` and `load_range`; convert `JoinError` to `AppError::Internal`, then update cache/emitter after awaiting the closure. Apply the same rule to the old-key flush and new-key local preload in `set_chart_context`.

Also expose `MarketService::flush_kline_key(&ChartWorkspaceKey) -> AppResult<KlineFlushOutcome>` over its existing shared store. Give `MarketService` a small `Mutex<HashSet<ChartWorkspaceKey>>` warning latch: emit the first context-flush error for a key, retain the dirty batch, and clear the latch after success. This keeps Task 4 independently compilable before `ChartWorkspaceService` is introduced in Task 5.

Keep the existing two-argument `MarketService::fetch_klines(symbol, interval)` as a wrapper using `None, None, Some(200)` for internal compatibility. Extend only the Tauri command arguments:

```rust
#[tauri::command]
pub async fn fetch_klines(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
    limit: Option<u32>,
    start: Option<i64>,
    end: Option<i64>,
) -> AppResult<Vec<Kline>>
```

The command parses the safe key and calls `fetch_kline_range`. Existing JavaScript calls that omit the three optional arguments keep the former latest-200 behavior.

Replace the two independently locked market context fields with one atomic value:

```rust
chart_context: Arc<RwLock<ChartWorkspaceKey>>
```

`active_symbol`, `kline_interval`, and the existing single-field setters become compatibility accessors/wrappers over this value. Add one atomic market-context command instead of making the frontend coordinate two persisted config mutations:

```rust
#[tauri::command]
pub async fn set_chart_context(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
) -> AppResult<Vec<Kline>>
```

It parses one `ChartWorkspaceKey`; if it equals the current key, it returns the current local latest 200 bars without changing config or subscriptions. Otherwise it:

1. best-effort flushes the old key's K-line buffer and emits one non-blocking diagnostic on failure while leaving the buffer dirty;
2. preloads the new key's latest 200 local bars in `spawn_blocking`; malformed JSONL lines are already skipped by `KlineStore`, while an actual I/O/storage failure rejects before config/runtime/subscriptions change;
3. calls `persist_market_config_update` once to update both `active_symbol` and `kline_interval` under the existing config lock;
4. atomically replaces `MarketService.chart_context` and refreshes WebSocket subscriptions once inside that persistence operation;
5. publishes and returns the preloaded bars, then schedules network backfill without awaiting it.

A config-persistence failure occurs before the frontend observes a new key and leaves both old Rust and frontend context intact. Keep `set_active_symbol` and `set_kline_interval` as compatibility wrappers around the same internal context-update helper, reading the unchanged half from `chart_context`. Register `set_chart_context` in `lib.rs` during Task 5 state/command wiring.

Serialize this whole async mutation with a FIFO `tokio::sync::Mutex<()>` in `MarketService`, so concurrent commands cannot interleave config, runtime context and subscriptions. Preserve the current symbol-change side effects after the atomic commit: when the symbol changed and the account is connected, schedule the existing market snapshot plus order/position refresh block exactly once; an interval-only change schedules only K-line backfill. Add regression tests around the extracted helper so this behavior is not lost when the two legacy commands delegate.

Change `MarketService::new` to accept one validated `initial_chart_context`. In `AppState::new`, parse it from the already-loaded `AppConfig` before placing that config in `RwLock`; fail startup with the existing configuration error if it is invalid rather than silently running Rust and Vue on different keys. Expose `replace_runtime_chart_context(key)` for startup/connection use: it atomically writes the one lock and touches the symbol cache but does not persist config or schedule duplicate refreshes. In `commands/connection.rs`, replace the sequential `set_active_symbol` / `set_kline_interval` calls with one `replace_runtime_chart_context(ChartWorkspaceKey::parse(&symbol, &kline_interval)?)` call before local restoration.

Preload the validated initial key into `KlineStore` during synchronous `AppState::new`, before any WebSocket task can start; a true storage/I/O error aborts startup as `AppError::Storage`, while malformed lines remain recoverable. Every later subscription change must await the new key's blocking local preload before replacing the runtime context/subscription. This invariant keeps the synchronous WebSocket hot path memory-only; arbitrary command/datafeed range calls still use `spawn_blocking`. Convert connection-time `restore_klines` to an async wrapper that performs its store read in `spawn_blocking` and emits after await.

- [ ] **Step 5: Run market and API mapper regression tests**

```powershell
cargo test services::market::tests -- --nocapture
cargo test api::mapper::tests::build_kline_params_normalize_millisecond_times_to_seconds -- --nocapture
```

Expected: market buffering tests and existing range-parameter normalization pass.

- [ ] **Step 6: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src-tauri/src/services/market.rs src-tauri/src/commands/market.rs src-tauri/src/commands/connection.rs src-tauri/src/state.rs
git commit -m "feat(market): support ranged chart history"
```

### Task 5: Add ChartWorkspaceService, Commands, the 5-second Rust flush, and shutdown flush

**Files:**

- Create: `src-tauri/src/services/chart_workspace.rs`
- Create: `src-tauri/src/services/chart_workspace/diagnostics.rs`
- Create: `src-tauri/src/services/chart_workspace/tests/mod.rs`
- Create: `src-tauri/src/services/chart_workspace/tests/support.rs`
- Create: `src-tauri/src/commands/chart_workspace.rs`
- Modify: `src-tauri/src/commands/market.rs`
- Modify: `src-tauri/src/services/mod.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/services/scheduler.rs`
- Modify: `src-tauri/src/services/scheduler/tests/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**

- Consumes: Task 2 `ChartStateStore`, Task 3 `KlineStore`, Task 1 request/result models.
- Produces: `load_chart_workspace`, `save_chart_workspace`, a five-second `KlineFlush` scheduler task, and final Rust shutdown durability.

- [ ] **Step 1: Write failing service load/default/range tests**

Use real temp-backed stores and deterministic sample data in `services/chart_workspace/tests`:

```rust
#[test]
fn load_without_range_returns_latest_two_hundred_local_bars() {
    let fixture = Fixture::new("default-range").with_kline_count(240);
    let snapshot = fixture.service.load(&fixture.key, None, None, None).unwrap();
    assert_eq!(snapshot.klines.len(), 200);
    assert_eq!(snapshot.klines.first().unwrap().open_time, 41);
}

#[test]
fn load_with_range_reads_the_full_local_history() {
    let fixture = Fixture::new("explicit-range").with_kline_count(600);
    let snapshot = fixture.service.load(&fixture.key, Some(101), Some(550), Some(500)).unwrap();
    assert_eq!(snapshot.klines.len(), 450);
    assert_eq!(snapshot.klines.first().unwrap().open_time, 101);
    assert_eq!(snapshot.klines.last().unwrap().open_time, 550);
}

#[test]
fn missing_state_returns_version_one_defaults() {
    let fixture = Fixture::new("defaults");
    let snapshot = fixture.service.load(&fixture.key, None, None, None).unwrap();
    assert_eq!(snapshot.view_state.schema_version, 1);
    assert_eq!(snapshot.view_state.revision, 0);
    assert_eq!(snapshot.preferences.main_indicators, vec!["MA", "EMA"]);
}
```

- [ ] **Step 2: Write failing save concurrency and partial-result tests**

Add these concrete save tests:

```rust
#[test]
fn rust_replaces_client_saved_at_and_rejects_stale_revisions() {
    let fixture = Fixture::new("revision");
    let first = fixture.service.save(request(&fixture.key, 2, 3, 1)).unwrap();
    let stale = fixture.service.save(request(&fixture.key, 1, 2, 999_999)).unwrap();
    assert!(first.saved_at_ms > 0);
    assert!(!stale.view_state_saved);
    assert!(!stale.preferences_saved);
    assert_eq!(stale.view_revision, 2);
    assert_eq!(stale.preferences_revision, 3);
    assert_eq!(fixture.load_view().saved_at_ms, first.saved_at_ms);
}

#[test]
fn one_failed_part_does_not_roll_back_successful_parts() {
    let fixture = Fixture::with_failures("partial", Failures::VIEW);
    let result = fixture.service.save(request(&fixture.key, 4, 5, 0)).unwrap();
    assert!(result.kline_saved);
    assert!(!result.view_state_saved);
    assert!(result.preferences_saved);
    assert!(result.view_state_error.is_some());
    assert!(result.kline_error.is_none());
    assert!(result.preferences_error.is_none());
}
```

Add `same_key_saves_enter_the_view_write_hook_serially`, asserting the second hook cannot enter before the first barrier releases. Add `different_keys_keep_independent_view_files`, saving BTC and ETH then asserting each exact revision/content. Add `preferences_serialize_across_different_keys`, blocking BTC's preference write and asserting ETH's preference hook waits. Add `omitted_parts_are_successful_durable_no_ops`, asserting both saved flags are true, returned revisions match disk, and neither write hook fires. Add three table-driven `all_parts_are_attempted_after_one_failure` rows for K-line/view/preferences failure and assert the other two success flags plus their durable files/outcomes.

Add `save_repairs_all_corrupt_state_candidates`: corrupt main/temp/backup, save a revision-1 view, and assert the view part succeeds and a subsequent load returns revision 1. The service may treat unreadable prior content as “durable revision unknown/zero” for conflict comparison, but it must still call `ChartStateStore::save_view`; a permissions/I/O failure then remains a structured failed part, while parse corruption is repaired by Task 2's safe replacement.

Declare `#[cfg(test)] mod tests;` in `services/chart_workspace.rs`. The serialization tests use Task 2's test-only write hook and barriers: block the first same-view/preferences write, start the second, and assert the hook has not been entered twice until the first barrier is released.

`Failures::VIEW` is a test-only filesystem setup, not a production trait: after creating the store root, create a directory at the exact main view-file path so only `save_view` fails while the K-line directory and global preferences path remain writable. Define equivalent isolated setups for `Failures::KLINE` and `Failures::PREFERENCES`. Remove the directories in the fixture's `Drop`; never add runtime fault-injection branches.

`tests/support.rs` owns one concrete fixture API—no test-local alternatives:

```rust
#[derive(Clone, Copy)]
pub(super) enum Failures { Kline, View, Preferences }

pub(super) struct Fixture {
    pub root: PathBuf,
    pub key: ChartWorkspaceKey,
    pub kline_store: Arc<KlineStore>,
    pub state_store: Arc<ChartStateStore>,
    pub service: ChartWorkspaceService,
}

impl Fixture {
    pub fn new(label: &str) -> Self {
        let root = test_root(label);
        let key = ChartWorkspaceKey::parse("BTCUSDT", "1").unwrap();
        let kline_store = Arc::new(KlineStore::with_dir(root.join("klines")));
        let state_store = Arc::new(ChartStateStore::with_root(root.join("state")));
        let service = ChartWorkspaceService::new(
            Arc::clone(&kline_store),
            Arc::clone(&state_store),
            Arc::new(|_| {}),
        );
        Self { root, key, kline_store, state_store, service }
    }

    pub fn with_kline_count(self, count: i64) -> Self {
        let bars = (1..=count).map(sample_kline).collect::<Vec<_>>();
        self.kline_store.upsert_bars(&self.key, &bars).unwrap();
        self
    }

    pub fn with_failures(label: &str, failure: Failures) -> Self {
        let fixture = Self::new(label);
        fixture.kline_store.upsert_bars(&fixture.key, &[sample_kline(1)]).unwrap();
        let path = match failure {
            Failures::Kline => fixture.root.join("klines").join("BTCUSDT_1.jsonl"),
            Failures::View => fixture.state_store.view_path_for_test(&fixture.key),
            Failures::Preferences => fixture.state_store.preferences_path_for_test(),
        };
        std::fs::create_dir_all(path).unwrap();
        fixture
    }

    pub fn load_view(&self) -> ChartViewStateV1 {
        self.state_store.load_view(&self.key).unwrap().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
```

`sample_kline(open_time)` fills the exact eight existing `Kline` fields with this fixture's BTC/1 key and decimal strings. `request(key, view_revision, preferences_revision, saved_at_ms)` returns both schema-one payloads with empty overlays, default viewport and `MA, EMA` / `VOL, MACD`. Reuse Task 2's atomic temp-root pattern, and expose `preferences_path_for_test` alongside the other `#[cfg(test)]` state-store paths.

- [ ] **Step 3: Run the service tests and confirm RED**

Run from `src-tauri`:

```powershell
cargo test services::chart_workspace::tests -- --nocapture
```

Expected: compilation fails because `ChartWorkspaceService` and its test seams do not exist.

- [ ] **Step 4: Implement load/save coordination and result semantics**

Expose:

```rust
pub struct ChartWorkspaceService {
    kline_store: Arc<KlineStore>,
    state_store: Arc<ChartStateStore>,
    view_locks: Mutex<HashMap<ChartWorkspaceKey, Arc<Mutex<()>>>>,
    preferences_lock: Mutex<()>,
    diagnostics: ChartWorkspaceDiagnostics,
}

impl ChartWorkspaceService {
    pub fn new(
        kline_store: Arc<KlineStore>,
        state_store: Arc<ChartStateStore>,
        reporter: Arc<dyn Fn(String) + Send + Sync>,
    ) -> Self;
    pub fn load(
        &self,
        key: &ChartWorkspaceKey,
        from: Option<i64>,
        to: Option<i64>,
        limit: Option<u32>,
    ) -> AppResult<ChartWorkspaceSnapshot>;
    pub fn save(&self, request: SaveChartWorkspaceRequest) -> AppResult<ChartWorkspaceSaveResult>;
    pub fn flush_kline_key(&self, key: &ChartWorkspaceKey) -> AppResult<KlineFlushOutcome>;
    pub fn flush_dirty_klines(&self) -> Vec<(ChartWorkspaceKey, AppResult<KlineFlushOutcome>)>;
}
```

`ChartWorkspaceDiagnostics` owns `Mutex<HashSet<ChartDiagnosticKey>>`. `report_once(key, message)` invokes the injected reporter only on first insertion; `clear(key)` removes the latch after a successful retry. `AppState` injects an `EventEmitter::emit_error` closure, while tests inject a `Mutex<Vec<String>>` collector. Use keys that include storage part plus workspace key (or `preferences` globally), so a five-second scheduler failure cannot spam but a later success re-enables a future diagnostic.

Now that this shared service exists, change `set_chart_context` in `commands/market.rs` to call `state.chart_workspace.flush_kline_key(&old_key)` for the pre-switch best-effort flush. Remove Task 4's transitional `MarketService` K-line warning latch and raw flush wrapper, so context, explicit-save and scheduler failures all pass through the same keyed diagnostic latch and cannot double-report.

`load` validates the key and range, clamps limit to `1..=10_000`, loads latest 200 when no range is supplied, and supplies version-one defaults for missing state. Corrupt state uses the store's backup recovery; if all state copies are invalid, return defaults while logging one storage diagnostic rather than preventing local K-line display.

`save` must re-run `ChartWorkspaceKey::parse(&request.key.symbol, &request.key.interval)` because serde can construct an unvalidated key, replace the request key with that normalized value, and validate both schema versions plus `viewState.symbol/interval` before touching disk. It always attempts the current key's K-line flush, then optional view state, then optional global preferences. Acquire and release the per-view lock before taking the global preferences lock; never hold both, so concurrent saves for different keys cannot deadlock.

Use these exact result semantics:

- omitted or content-equal view/preferences are durable no-ops: `*_saved = true`, no write, and return the last durable revision. Content equality ignores only `revision` and `savedAtMs`, so retrying a response-lost save cannot conflict solely with Rust's authoritative timestamp;
- a lower revision with different content, or the same revision with different content, is that part's structured revision conflict (`*_saved = false`, error populated), while the other parts are still attempted;
- a write failure returns the last durable revision, never the rejected request revision;
- preferences revision comparison is global across every workspace key;
- clear a diagnostic latch only after that part is durably clean;
- compute one Rust `now_ms` per call. If any part writes bytes (including K-line append), result `savedAtMs = now_ms`; otherwise return the maximum existing durable view/preference `savedAtMs`, or `0` when neither exists.

Rust overwrites client `savedAtMs` only on an accepted newer state revision. Never roll back a successful part because another part failed.

Add `response_lost_retry_is_content_equal_no_op`: persist revision 4, retry revision 4 with the same user content but client `savedAtMs = 0`, and assert success, no write-hook entry, and the original Rust timestamp/revision are returned.

- [ ] **Step 5: Add the thin Commands and state wiring**

Create:

```rust
#[tauri::command]
pub async fn load_chart_workspace(
    state: State<'_, AppState>,
    symbol: String,
    interval: String,
    from: Option<i64>,
    to: Option<i64>,
    limit: Option<u32>,
) -> AppResult<ChartWorkspaceSnapshot> {
    let key = ChartWorkspaceKey::parse(&symbol, &interval)?;
    let service = state.chart_workspace.clone();
    tauri::async_runtime::spawn_blocking(move || service.load(&key, from, to, limit))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}

#[tauri::command]
pub async fn save_chart_workspace(
    state: State<'_, AppState>,
    request: SaveChartWorkspaceRequest,
) -> AppResult<ChartWorkspaceSaveResult> {
    let service = state.chart_workspace.clone();
    tauri::async_runtime::spawn_blocking(move || service.save(request))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}
```

Export/register both commands plus Task 4's `set_chart_context`. In `AppState::new`, create one `Arc<KlineStore>`, give the same Arc to `MarketService` and `ChartWorkspaceService`, create `ChartStateStore`, retain `pub chart_workspace: Arc<ChartWorkspaceService>`, and pass that service to `SchedulerService`.

- [ ] **Step 6: Write failing scheduler timing and bootstrap tests**

Extend scheduler tests:

```rust
#[test]
fn kline_flush_is_five_seconds_and_not_in_connection_bootstrap() {
    assert_eq!(TaskId::KlineFlush.interval(), Some(Duration::from_secs(5)));
    assert!(!bootstrap_tasks(true).contains(&TaskId::KlineFlush));
    assert!(!bootstrap_tasks(false).contains(&TaskId::KlineFlush));
}

#[test]
fn kline_flush_bypasses_account_lifecycle_coordination() {
    assert!(!TaskId::KlineFlush.requires_account_lifecycle());
    assert!(TaskId::MarketFallback.requires_account_lifecycle());
}
```

- [ ] **Step 7: Implement the periodic and final flush**

Add `TaskId::KlineFlush` exhaustively to `interval`, `bootstrap_label` (`"K线持久化"`), `from_name` (`"kline" | "klineFlush"`), the periodic task registry/start list, `SchedulerRefs`, `clone_refs`, and both execute-inner matches. Define and test a pure `first_tick_delay(task)` helper, then start it with a delayed first tick:

```rust
fn first_tick_delay(task: TaskId) -> Duration {
    if task == TaskId::KlineFlush {
        task.interval().expect("periodic task has an interval")
    } else {
        Duration::ZERO
    }
}

let start = if task == TaskId::KlineFlush {
    tokio::time::Instant::now() + first_tick_delay(task)
} else {
    tokio::time::Instant::now()
};
let mut ticker = tokio::time::interval_at(start, interval);
ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
```

Both `SchedulerService::execute` and `SchedulerRefs::execute` must branch through `TaskId::requires_account_lifecycle()`. Implement `execute_kline_flush(service: Arc<ChartWorkspaceService>) -> AppResult<()>`: it uses `tokio::task::spawn_blocking`, calls `flush_dirty_klines()` without the account lifecycle lock, lets the service report each key through its latch, and returns one joined `AppError::Storage` only after all keys were attempted. Convert a `JoinError` to `AppError::Internal`; do not perform synchronous file writes on the scheduler's async worker.

In `RunEvent::Exit`, preserve this order:

```rust
tauri::async_runtime::block_on(async {
    state.scheduler.stop().await;
});
for (key, result) in state.chart_workspace.flush_dirty_klines() {
    if let Err(error) = result {
        tracing::error!(symbol = %key.symbol, interval = %key.interval, %error, "final kline flush failed");
    }
}
```

This final hook covers Rust K-line buffers only; frontend drawing/config closure is handled in Task 9 before window destruction.

- [ ] **Step 8: Run Rust workspace service and scheduler tests**

```powershell
cargo test services::chart_workspace::tests -- --nocapture
cargo test services::scheduler::tests -- --nocapture
cargo test storage::kline_store::tests -- --nocapture
```

Expected: all focused Rust groups pass.

- [ ] **Step 9: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src-tauri/src/services/chart_workspace.rs src-tauri/src/services/chart_workspace/diagnostics.rs src-tauri/src/services/chart_workspace/tests src-tauri/src/commands/chart_workspace.rs src-tauri/src/commands/market.rs src-tauri/src/services/mod.rs src-tauri/src/commands/mod.rs src-tauri/src/services/scheduler.rs src-tauri/src/services/scheduler/tests/mod.rs src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "feat(chart): expose workspace persistence service"
```

### Task 6: Add frontend persistence calls, atomic chart context switching, and ranged datafeed

**Files:**

- Create: `src/services/chartWorkspaceService.ts`
- Create: `src/services/chartWorkspaceFlushRegistry.ts`
- Modify: `src/composables/easiKlineDatafeed.ts`
- Modify: `src/stores/market.ts`
- Create: `tests/frontend/chartWorkspaceService.test.ts`
- Create: `tests/frontend/chartWorkspaceSwitch.test.ts`
- Create: `tests/frontend/easiKlineDatafeed.test.ts`

**Interfaces:**

- Consumes: Task 1 TypeScript types, Task 4 extended `fetch_klines`, Task 5 Commands.
- Produces: typed Tauri calls, an active/all flush registry, `marketStore.setChartContext`, and a datafeed that honors Pro `from` / `to` while synchronizing both symbol and interval.

- [ ] **Step 1: Write failing command wrapper tests**

Mock `tauriInvoke` and assert exact argument names:

```ts
it('loads the exact workspace range', async () => {
  await loadChartWorkspace(
    { symbol: 'BTCUSDT', interval: '15' },
    { from: 1000, to: 2000, limit: 500 },
  )
  expect(tauriInvoke).toHaveBeenCalledWith('load_chart_workspace', {
    symbol: 'BTCUSDT', interval: '15', from: 1000, to: 2000, limit: 500,
  })
})

it('sends nullable dirty parts without uploading klines', async () => {
  const request = sampleSaveRequest({ viewState: null, preferences: samplePreferences(3) })
  await saveChartWorkspace(request)
  expect(tauriInvoke).toHaveBeenCalledWith('save_chart_workspace', { request })
  expect(tauriInvoke).not.toHaveBeenCalledWith(
    'save_chart_workspace',
    expect.objectContaining({ klines: expect.anything() }),
  )
})

it('flushes only the active owner for page/context and every owner for close', async () => {
  const first = vi.fn().mockResolvedValue(undefined)
  const second = vi.fn().mockRejectedValue(new Error('disk full'))
  const a = registerChartWorkspaceFlusher(first)
  const b = registerChartWorkspaceFlusher(second)
  a.setActive(true)
  b.setActive(true)
  a.setActive(false) // stale deactivation must not clear b
  await expect(flushActiveChartWorkspace('page')).rejects.toThrow('disk full')
  await expect(flushAllChartWorkspaces('close')).resolves.toBeUndefined()
  expect(first).toHaveBeenCalledWith('close')
  expect(second).toHaveBeenCalledWith('page')
  expect(second).toHaveBeenCalledWith('close')
  a.unregister()
  b.unregister()
})
```

- [ ] **Step 2: Implement the typed service and active flush registry**

Export:

```ts
export interface ChartHistoryRange {
  from?: number
  to?: number
  limit?: number
}

export function loadChartWorkspace(
  key: ChartWorkspaceKey,
  range?: ChartHistoryRange,
): Promise<ChartWorkspaceSnapshot>

export function saveChartWorkspace(
  request: SaveChartWorkspaceRequest,
): Promise<ChartWorkspaceSaveResult>

export function refreshChartKlines(
  key: ChartWorkspaceKey,
  range: Required<ChartHistoryRange>,
): Promise<Kline[]>
```

`refreshChartKlines` invokes existing `fetch_klines` with `{ symbol, interval, start: from, end: to, limit }`.

The registry can retain multiple durability handlers but has exactly one active handler for page/context flushes; Task 9 normally registers the one app-level autosave, while the multi-handler behavior remains covered for safe close aggregation:

```ts
export type ChartWorkspaceFlushReason = 'timer' | 'context' | 'page' | 'close'
export type ChartWorkspaceFlushHandler = (reason: ChartWorkspaceFlushReason) => Promise<void>

export interface RegisteredChartWorkspaceFlusher {
  setActive(active: boolean): void
  unregister(): void
}

export function registerChartWorkspaceFlusher(
  handler: ChartWorkspaceFlushHandler,
): RegisteredChartWorkspaceFlusher

export async function flushActiveChartWorkspace(
  reason: ChartWorkspaceFlushReason,
): Promise<void>

export async function flushAllChartWorkspaces(
  reason: Extract<ChartWorkspaceFlushReason, 'close'>,
): Promise<void>
```

`setActive(true)` atomically replaces the active pointer; `setActive(false)` clears it only when it still points to that registration. `unregister()` removes that same registration without affecting a newer active chart. `flushAllChartWorkspaces` snapshots the registered set and uses `Promise.allSettled` so one controller failure cannot prevent another controller's close flush.

- [ ] **Step 3: Write failing atomic context-switch tests**

Test one flush per logical context change and no intermediate frontend key:

```ts
it('flushes once before atomically changing symbol and interval', async () => {
  const market = useMarketStore()
  market.activeSymbol = 'BTCUSDT'
  market.klineInterval = '1'
  market.klines = [bar(1_000)]
  vi.mocked(tauriInvoke).mockResolvedValue([bar(2_000)])

  await market.setChartContext({ symbol: 'ETHUSDT', interval: '15' })

  expect(flushActiveChartWorkspace).toHaveBeenCalledTimes(1)
  expect(flushActiveChartWorkspace).toHaveBeenCalledWith('context')
  expect(tauriInvoke).toHaveBeenCalledTimes(1)
  expect(tauriInvoke).toHaveBeenCalledWith('set_chart_context', {
    symbol: 'ETHUSDT', interval: '15',
  })
  expect(market.activeSymbol).toBe('ETHUSDT')
  expect(market.klineInterval).toBe('15')
  expect(market.klines).toEqual([bar(2_000)])
})
```

Also verify same-key calls do not flush/invoke, a failed command leaves symbol, interval and the old K-line array unchanged, and `setActiveSymbol`/`setKlineInterval` delegate to `setChartContext`.

Add a rapid-intent case: start A→B with a deferred Rust result, request A→C before B resolves, resolve B, then resolve C. Assert Rust calls are serialized, B never becomes visible in Pinia, and the final visible key/data are C.

Add `compensates_when_the_latest_intent_returns_to_the_original_key`: start A→B and wait until Rust is invoked, request A before B resolves, resolve B, and assert a second `set_chart_context(A)` command is issued before the promise for A resolves. B must never become visible and final Pinia bars must be the A bars returned by that compensating command.

- [ ] **Step 4: Implement `marketStore.setChartContext`**

Add:

```ts
let latestContextGeneration = 0
let contextTransitionTail: Promise<void> = Promise.resolve()
let backendContext: ChartWorkspaceKey | null = null
let backendKlines: Kline[] = []
let backendSeeded = false

function setChartContext(next: ChartWorkspaceKey): Promise<void> {
  const key = normalizeChartWorkspaceKey(next)
  const generation = ++latestContextGeneration
  const transition = contextTransitionTail.then(async () => {
    if (generation !== latestContextGeneration) return
    backendContext ??= normalizeChartWorkspaceKey({
      symbol: activeSymbol.value,
      interval: klineInterval.value,
    })
    if (!backendSeeded) {
      backendKlines = [...klines.value]
      backendSeeded = true
    }
    if (sameChartKey(key, backendContext)) {
      activeSymbol.value = key.symbol
      klineInterval.value = key.interval
      klines.value = backendKlines
      return
    }

    try {
      await flushActiveChartWorkspace('context')
    } catch (error) {
      reportError(error, '图表上下文切换前补存失败')
    }
    if (generation !== latestContextGeneration) return

    let restored: Kline[]
    try {
      restored = await tauriInvoke<Kline[]>('set_chart_context', {
        symbol: key.symbol,
        interval: key.interval,
      })
    } catch (error) {
      if (generation !== latestContextGeneration) return
      throw error
    }
    backendContext = key
    backendKlines = restored
    if (generation !== latestContextGeneration) return

    activeSymbol.value = key.symbol
    klineInterval.value = key.interval
    klines.value = restored
    ticker.value = null
    depth.value = null
  })
  contextTransitionTail = transition.catch(() => undefined)
  return transition
}
```

Import the existing global error reporter as `reportError` and add pure `sameChartKey`. `setActiveSymbol(symbol)` delegates with the current interval; `setKlineInterval(interval)` delegates with the current symbol. Do not clear any frontend state before the Rust command succeeds. A save failure is best-effort and cannot block the context change; a Rust context/config failure still rejects and preserves the old visible state. Track the last successfully committed Rust key/data separately from visible Pinia state: a superseded in-flight command may mutate Rust even though its UI result is ignored, so a queued “back to A” intent must compare with `backendContext` and issue the compensating command. The generation check implements last-intent-wins while `contextTransitionTail` prevents overlapping Rust context commands.

Add `superseded_command_failure_does_not_reject_the_latest_intent`: reject deferred B after C is queued, then resolve C and assert C succeeds/appears without reporting B as the current transition failure.

- [ ] **Step 5: Write failing datafeed range and Pro-selection tests**

```ts
it('syncs both Pro-selected key fields and reads the requested range', async () => {
  const onContextChange = vi.fn().mockResolvedValue(undefined)
  const onRefresh = vi.fn()
  const datafeed = makeDatafeed({ onContextChange, onRefresh })
  vi.mocked(loadChartWorkspace).mockResolvedValue(sampleSnapshot(
    [bar(1_000, 'ETHUSDT', '15')],
    key('ETHUSDT', '15'),
  ))
  const result = await datafeed.getHistoryKLineData(
    { ticker: 'ETHUSDT' },
    { multiplier: 15, timespan: 'minute', text: '15m' },
    1_000,
    2_000,
  )
  expect(onContextChange).toHaveBeenCalledWith({ symbol: 'ETHUSDT', interval: '15' })
  expect(loadChartWorkspace).toHaveBeenCalledWith(
    { symbol: 'ETHUSDT', interval: '15' },
    { from: 1_000, to: 2_000, limit: 500 },
  )
  expect(result.map((bar) => bar.timestamp)).toEqual([1_000])
})

it('returns primed local history before the REST refresh settles', async () => {
  const rest = deferred<Kline[]>()
  vi.mocked(refreshChartKlines).mockReturnValue(rest.promise)
  const datafeed = makeDatafeed()
  datafeed.primeHistory(key('BTCUSDT', '1'), 7, [bar(1_000)])
  const result = await datafeed.getHistoryKLineData(symbol(), period(), 1_000, 2_000)
  expect(result.map((bar) => bar.timestamp)).toEqual([1_000])
  expect(rest.settled).toBe(false)
})

it('awaits REST as the initial Pro payload when no local bars exist', async () => {
  vi.mocked(loadChartWorkspace).mockResolvedValue(sampleSnapshot([], key()))
  vi.mocked(refreshChartKlines).mockResolvedValue([bar(2_000)])
  const result = await makeDatafeed().getHistoryKLineData(
    symbol(), period(), 1_000, 2_000,
  )
  expect(result.map((item) => item.timestamp)).toEqual([2_000])
})

it('suppresses an intermediate key during programmatic symbol-period sync', async () => {
  const onContextChange = vi.fn().mockResolvedValue(undefined)
  const datafeed = makeDatafeed({ onContextChange })
  datafeed.primeHistory(key('BTCUSDT', '1'), 7, [bar(1_000)])
  datafeed.expectProgrammaticContext(key('ETHUSDT', '15'), 8)
  const result = await datafeed.getHistoryKLineData(
    { ticker: 'ETHUSDT' }, period('1'), 1_000, 2_000,
  )
  expect(onContextChange).not.toHaveBeenCalled()
  expect(result.map((bar) => bar.timestamp)).toEqual([1_000])
})
```

Add `offline_refresh_reports_once_after_local_success`: resolve local data, reject REST twice for the same key/range, assert both history calls resolve locally and the rate-limited reporter runs once. Add `rest_refresh_emits_key_generation_and_range`: resolve REST and assert the full `ChartHistoryRefreshResult`. Add `stale_a_local_result_returns_current_b_prime`: defer A's local load, prime B, resolve A, and assert the caller receives B data and no A refresh callback. Preserve `subscribe_deduplicates_only_identical_realtime_updates`: send one bar, an identical repeat, then a changed close at the same timestamp and assert callbacks contain the first and changed bars. Add `realtime_bar_for_another_subscription_key_is_ignored`, subscribing BTC/1 then pushing ETH/15 and asserting no callback.

- [ ] **Step 6: Refactor EasiKlineDatafeed**

Use this constructor:

```ts
export interface ChartHistoryRefreshResult {
  key: ChartWorkspaceKey
  generation: number
  range: Required<ChartHistoryRange>
  klines: KLineData[]
}

type ChartContextChangeHandler = (key: ChartWorkspaceKey) => Promise<void>
type ChartHistoryRefreshHandler = (result: ChartHistoryRefreshResult) => void

constructor(
  private getSymbols: () => string[],
  private getContext: () => ChartWorkspaceKey,
  private onContextChange: ChartContextChangeHandler,
  private onHistoryRefresh: ChartHistoryRefreshHandler,
  private report: (context: string, error: unknown) => void,
) {}

primeHistory(key: ChartWorkspaceKey, generation: number, klines: Kline[]): void
pushRealtimeBar(key: ChartWorkspaceKey, bar: KLineData | null): void
expectProgrammaticContext(key: ChartWorkspaceKey, generation: number): void
waitForExpectedContext(
  key: ChartWorkspaceKey,
  generation: number,
  timeoutMs: number,
): Promise<boolean>
```

`getHistoryKLineData` converts the Pro period and normalizes `{ symbol: symbol.ticker, interval }`. Give every call a monotonically increasing request generation and follow this exact flow:

1. If `expectProgrammaticContext` is armed and the request is an intermediate key, do not call `onContextChange`; return the currently primed key's range.
2. If the request is the expected final key, clear the suppression and resolve matching `waitForExpectedContext` waiters with `true`. Otherwise, when it differs from `getContext()`, await `onContextChange(requestedKey)`; Task 9's handler does not resolve until the keyed snapshot is loaded and primed. A waiter timeout resolves `false` and removes itself.
3. After every await, compare request generation and key with the latest/current values. A stale request returns current primed data and never returns old-key bars to Pro.
4. Return matching primed local data immediately. If no primed range exists (for example Pro load-more), await `loadChartWorkspace` for that local range. A local read failure is reported but becomes an empty local result; it must not prevent the network attempt.
5. Start `refreshChartKlines` for the same `{ from, to, limit: 500 }`. When local bars are non-empty, do not await it: return local bars and emit a guarded `ChartHistoryRefreshResult` on later success. When local bars are empty, await REST and return those bars directly as Pro's initial payload without also emitting a replacement callback; if REST fails, report it and return `[]` rather than rejecting. Maintain an internal warning `Set` keyed by operation + normalized key + range, call the injected reporter only on first failure, and clear that key after success. Task 7/9 owns later guarded full-history replacement because Pro's subscribe callback accepts only one `KLineData` and cannot backfill arbitrary history.

Store the normalized key supplied to `subscribe(symbol, period)` and clear it on the matching `unsubscribe`. `pushRealtimeBar` forwards only when its key equals that subscription; ignore identical repeats but forward a changed candle at the same timestamp. Do not pass arrays through `subscribe`, and do not manually apply initial data to the core chart.

- [ ] **Step 7: Run all focused frontend service/switch/datafeed tests**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceService.test.ts tests/frontend/chartWorkspaceSwitch.test.ts tests/frontend/easiKlineDatafeed.test.ts
```

Expected: all command, flush ordering, shared context and offline fallback cases pass.

- [ ] **Step 8: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src/services/chartWorkspaceService.ts src/services/chartWorkspaceFlushRegistry.ts src/composables/easiKlineDatafeed.ts src/stores/market.ts tests/frontend/chartWorkspaceService.test.ts tests/frontend/chartWorkspaceSwitch.test.ts tests/frontend/easiKlineDatafeed.test.ts
git commit -m "feat(chart): synchronize chart datafeed context"
```

### Task 7: Add the version-locked KLineCharts core bridge and workspace adapter

**Files:**

- Create: `src/services/klineChartsCoreBridge.ts`
- Create: `src/services/klineChartsWorkspaceAdapter.ts`
- Create: `tests/frontend/klineChartsCoreBridge.test.ts`
- Create: `tests/frontend/klineChartsWorkspaceAdapter.test.ts`

**Interfaces:**

- Consumes: the exact dependency contract from Task 1, KLineCharts Pro's rendered DOM, and Task 1 snapshot utilities.
- Produces: a guarded core `Chart` handle and a reversible adapter that is the only module allowed to patch or call private-version-sensitive chart behavior.

- [ ] **Step 1: Write failing bridge contract tests**

Use injected `version`, `init`, and DOM fixtures so the bridge is testable without constructing Pro:

```ts
it('recovers the already-registered core chart from the Pro widget', () => {
  widget.setAttribute('k-line-chart-id', 'k_line_chart_1')
  const chart = fakeCoreChart({ id: 'k_line_chart_1' })
  const init = vi.fn().mockReturnValue(chart)

  const result = recoverKLineChartsCore(proHost, {
    expectedVersion: '9.8.12',
    version: () => '9.8.12',
    init,
  })

  expect(result).toEqual({ ok: true, chart })
  expect(widget.id).toBe('k_line_chart_1')
  expect(init).toHaveBeenCalledOnce()
  expect(init).toHaveBeenCalledWith(widget)
})

it.each([
  ['wrong runtime', { version: () => '10.0.0' }],
  ['missing registry marker', { marker: null }],
  ['conflicting widget id', { marker: 'k_line_chart_1', widgetId: 'different' }],
  ['mismatched chart id', { returnedId: 'chart_2' }],
  ['missing required method', { removeMethod: 'createOverlay' }],
])('rejects %s without throwing', (_label, overrides) => {
  const result = recoverFixture(overrides)
  expect(result.ok).toBe(false)
})
```

Also assert that a missing `k-line-chart-id` never calls `init`. A non-empty widget DOM `id` is accepted only when it equals the marker; a different ID must fail before `init`, because calling `init(widget)` under that ID would create a second chart. Add one contract test with the real mocked `klinecharts.init` registry behavior: the returned object must be the same registered instance, not a newly created chart.

- [ ] **Step 2: Run the bridge test and confirm RED**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/klineChartsCoreBridge.test.ts
```

Expected: import resolution fails because the bridge does not exist.

- [ ] **Step 3: Implement a narrow, discriminated bridge**

Define only the methods later tasks need:

```ts
import type { Chart } from 'klinecharts'

export interface CoreBridgeDependencies {
  expectedVersion: '9.8.12'
  version: () => string
  init: (container: HTMLElement) => Chart | null
}

export type WorkspaceCoreChart = Pick<Chart,
  | 'id'
  | 'applyNewData'
  | 'createOverlay'
  | 'getOverlayById'
  | 'overrideOverlay'
  | 'removeOverlay'
  | 'createIndicator'
  | 'getIndicatorByPaneId'
  | 'overrideIndicator'
  | 'removeIndicator'
  | 'getBarSpace'
  | 'setBarSpace'
  | 'getVisibleRange'
  | 'getDataList'
  | 'scrollToTimestamp'
  | 'subscribeAction'
  | 'unsubscribeAction'
  | 'resize'
>

export type CoreBridgeResult =
  | { ok: true; chart: WorkspaceCoreChart }
  | { ok: false; code: 'version' | 'widget' | 'marker' | 'instance' | 'shape'; message: string }

export function recoverKLineChartsCore(
  proHost: HTMLElement,
  dependencies?: CoreBridgeDependencies,
): CoreBridgeResult
```

The default dependency object is `{ expectedVersion: '9.8.12', version, init }`, with both functions imported from the same deduplicated `klinecharts` module; tests inject the complete object and never mock private Pro fields.

The implementation must:

1. Check `version() === '9.8.12'` before touching the DOM.
2. Require exactly one `.klinecharts-pro-widget` below `proHost`.
3. Read a non-empty `k-line-chart-id`; do not call `init` when it is absent.
4. Copy that marker to `widget.id` only when the DOM `id` is empty; fail if a non-empty DOM `id` differs from the marker.
5. Call the deduplicated module's `init(widget)` once and require `chart.id === marker`.
6. Check every method in `WorkspaceCoreChart` at runtime and return a failure result instead of throwing.

Do not export raw registry details or use `_chartApi` anywhere.

- [ ] **Step 4: Rerun the bridge test and confirm GREEN**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/klineChartsCoreBridge.test.ts
```

Expected: all version, marker, identity and method-shape cases pass.

- [ ] **Step 5: Write failing adapter snapshot and restoration tests**

Cover the complete behavioral boundary with a fake `WorkspaceCoreChart`:

```ts
import { ActionType, type Indicator, type KLineData, type VisibleRange } from 'klinecharts'

const bars = (count: number): KLineData[] => Array.from({ length: count }, (_, index) => ({
  timestamp: (index + 1) * 1_000,
  open: 1,
  high: 2,
  low: 0.5,
  close: 1.5,
  volume: 10,
}))

const range = (from: number, to: number): VisibleRange => ({
  from,
  to,
  realFrom: from,
  realTo: to,
})

const indicator = (name: string): Indicator => ({ name } as Indicator)

const preferences = (mainIndicators: string[], subIndicators: string[]): ChartPreferencesV1 => ({
  ...samplePreferences(),
  mainIndicators,
  subIndicators,
})

const viewWithIndicatorOverlay = (indicatorName: string): ChartViewStateV1 => ({
  ...viewState(),
  overlays: [{
    ...segment(),
    pane: { kind: 'indicator', indicatorName },
  }],
  viewport: { barSpace: 8, rightTimestamp: 5_000 },
})

it('captures only completed registered overlays with semantic panes', () => {
  const chart = fakeChart({
    overlays: [
      overlay({ id: 'done', currentStep: -1, paneId: 'pane_macd' }),
      overlay({ id: 'drawing', currentStep: 2, paneId: 'candle_pane' }),
    ],
    panes: { pane_macd: indicator('MACD') },
  })
  const adapter = createKLineChartsWorkspaceAdapter(chart)
  adapter.attach()
  chart.createOverlay(overlayCreateResult('done'))

  expect(adapter.captureViewState(key(), 4).overlays).toEqual([
    expect.objectContaining({ id: 'done', pane: { kind: 'indicator', indicatorName: 'MACD' } }),
  ])
})

it('uses the exclusive visible-range upper bound for the right timestamp', () => {
  const adapter = adapterFixture({
    visibleRange: { from: 2, to: 5, realFrom: 2, realTo: 5 },
    data: bars(8),
  })
  expect(adapter.captureViewState(key(), 1).viewport.rightTimestamp)
    .toBe(bars(8)[4].timestamp)
})

it('adopts Pro-created indicators before restoring semantic panes', async () => {
  const fixture = restoreFixture({
    indicatorMaps: new Map([
      ['candle_pane', new Map([['MA', indicator('MA')]])],
      ['pane_macd', new Map([['MACD', indicator('MACD')]])],
    ]),
  })
  const restored = fixture.adapter.prepareViewReplacement(
    preferences(['MA'], ['MACD']),
    viewWithIndicatorOverlay('MACD'),
    1,
  )
  fixture.emitAction(ActionType.OnDataReady)
  await restored
  expect(fixture.calls).toEqual([
    'createOverlay:segment:pane_macd',
    'setBarSpace',
    'scrollToTimestamp',
  ])
})

it('reapplies the live viewport after guarded REST replacement', async () => {
  const fixture = restoreFixture({ visibleRange: range(2, 5), data: bars(8) })
  const refreshed = fixture.adapter.applyRefreshedHistory(bars(12), 3)
  expect(fixture.calls[0]).toBe('applyNewData:12')
  fixture.emitAction(ActionType.OnDataReady)
  await refreshed
  expect(fixture.calls.slice(-2)).toEqual(['setBarSpace', 'scrollToTimestamp'])
})
```

Build `fakeChart`, `adapterFixture`, and `restoreFixture` in the test file as strict fakes implementing every `WorkspaceCoreChart` method with `vi.fn`; expose `calls` and an `emitAction(ActionType)` helper backed by the subscribed callback map. Add these named cases:

- `wrapped_mutators_preserve_this_arguments_and_return_values`: invoke `createOverlay`, `overrideOverlay`, and `removeOverlay` through the patched chart and assert their original receiver, exact arguments, and return values.
- `overlay_callbacks_are_composed`: give an overlay existing `onDrawEnd`, `onPressedMoveEnd`, and `onRemoved` spies, trigger each wrapper, and assert both the original callback and `onViewDirty` run once.
- `indicator_mutation_marks_only_preferences_dirty`: call create/override/remove indicator and assert `onPreferencesDirty` increments while `onViewDirty` does not.
- `indicator_create_is_idempotent_after_cross_surface_restore`: preseed RSI in a sub-pane, call the Pro-style `createIndicator('RSI')`, and assert the existing pane ID is returned, the original creator is not called, and no duplicate/dirty callback is produced.
- `snapshot_json_sanitization_never_leaks_runtime_values`: include a function, circular object, `NaN`, and `Infinity` in `extendData`, `styles`, and points; assert the captured values are JSON-safe `null` and `JSON.stringify` succeeds.
- `unknown_overlay_names_are_skipped_independently`: register one missing overlay template and one valid segment; assert only the segment restores and one diagnostic is reported.
- `missing_indicator_pane_falls_back_once`: restore two overlays targeting the same unavailable indicator pane; assert both use the candle pane and the diagnostic spy is called once.
- `detach_is_exactly_reversible`: capture original mutator identities, attach/detach twice, and assert originals are restored plus each stable action callback is unsubscribed once per attachment.
- `viewport_actions_coalesce_one_dirty_notification`: emit `OnZoom`, `OnScroll`, and `OnVisibleRangeChange` in one microtask and assert one `onViewDirty` call.

- [ ] **Step 6: Implement the reversible adapter**

Export this public surface:

```ts
import { ActionType, type Indicator, type KLineData } from 'klinecharts'

export interface KLineChartsWorkspaceAdapter {
  attach(): void
  detach(): void
  prepareViewReplacement(
    preferences: ChartPreferencesV1,
    viewState: ChartViewStateV1,
    generation: number,
  ): Promise<void>
  applyPrimedHistoryFallback(data: KLineData[], generation: number): void
  applyRefreshedHistory(data: KLineData[], generation: number): Promise<void>
  cancelGeneration(generation: number): void
  capturePreferences(revision: number): ChartPreferencesV1
  captureViewState(key: ChartWorkspaceKey, revision: number): ChartViewStateV1
  resize(): void
}

export interface WorkspaceAdapterOptions {
  onViewDirty: () => void
  onPreferencesDirty: () => void
  reportDiagnostic: (message: string) => void
}
```

Implementation rules:

- Capture original overlay and indicator methods once and restore them in `detach`; attaching twice is a no-op.
- Keep adapter diagnostics in a `Set<string>` keyed by compatibility cause/pane/name; call `reportDiagnostic` only on first occurrence for that key during the adapter lifetime.
- Maintain a `Set<string>` of overlay IDs returned by both string and string-array `createOverlay` results.
- Compose `onDrawEnd`, `onPressedMoveEnd`, and `onRemoved` with normal functions so callback `this`, arguments and boolean return are preserved. Mark dirty or remove the ID from the registry in `finally`; when no callback existed, return `false` so persistence does not consume the chart event. Coalesce patched `removeOverlay` with `onRemoved` so one deletion advances revision once even if both paths fire.
- Patch `overrideOverlay` as well as create/remove; after a successful tracked override, coalesce one view-dirty notification even when Pro applies a group-wide mode/lock/visibility change without an explicit ID.
- Patch `createIndicator` and `removeIndicator`, preserving arguments, return values and `this`, and mark the global name-only preference revision dirty after successful user-driven mutations. Preserve `overrideIndicator` unchanged because schema v1 does not persist indicator parameters, visibility or styles.
- Snapshot only IDs whose current object still exists and has `currentStep === -1`.
- Resolve `candle_pane` to `{ kind: 'candle' }`; resolve other pane IDs through `getIndicatorByPaneId(paneId)` and persist `{ kind: 'indicator', indicatorName }`. Capture main-indicator names from the `candle_pane` map and sub-indicator names from every other pane map, preserving map insertion order.
- Make preference reconciliation idempotent. On first construction Pro has already created the saved `mainIndicators`/`subIndicators`; call `getIndicatorByPaneId()` with no arguments, narrow it to `Map<string, Map<string, Indicator>>`, and record existing pane IDs rather than creating duplicates. On later activation, remove extras and create only missing names, then refresh the indicator-name-to-pane map.
- Wrap later indicator creation idempotently: extract the requested semantic name, return the existing matching candle/sub-pane ID when restoration already created it, and call the original creator only when absent. Treat removal of an absent indicator as a no-op and calculate preference dirtiness from before/after semantic indicator maps. This prevents a keep-mounted Pro surface's internal menu state from creating duplicate panes after another surface changed the global preference.
- Suppress dirty callbacks while restoring preferences, viewport or overlays; re-enable them only after restoration completes.
- Persist overlay `mode`, but no global drawing-toolbar mode.
- Treat `visibleRange.to` as exclusive and guard all absent/out-of-range values.
- Subscribe once with stable callback references and the `ActionType` enum, not string literals. `OnZoom`, `OnScroll`, and `OnVisibleRangeChange` feed one microtask-coalesced viewport-dirty callback; `ActionType.OnDataReady` dispatches only when a pending `{ generation, reason }` token exists.
- `prepareViewReplacement` immediately suppresses tracking, removes every registered old-key overlay, reconciles indicators, and arms a `persisted` token. On the matching data-ready event it enumerates panes, restores overlays, calls `setBarSpace`, then `scrollToTimestamp`, clears the token and resumes tracking. A realtime data-ready event with no token is ignored.
- Never call core `applyNewData` for the first component hydration. Pro owns initial application through the primed datafeed. On a later same-instance context switch, `applyPrimedHistoryFallback` may call it only when Task 6 proves Pro never requested the expected final key; it reuses the already-armed `persisted` token. For a keyed REST refresh, `applyRefreshedHistory` merges the refresh with `getDataList()` so realtime bars that arrived later win, captures the live viewport, arms a `refresh` token, and calls `applyNewData(KLineData[])`; its matching data-ready event restores only that captured viewport while tracking is suppressed.
- `cancelGeneration` resolves any stale pending gate without applying it (never reject into an unobserved lifecycle promise). `detach` restores patched methods and unsubscribes the permanent action callbacks with the exact same function references. Never recreate the Pro instance.

- [ ] **Step 7: Run adapter tests and strict typecheck**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/klineChartsCoreBridge.test.ts tests/frontend/klineChartsWorkspaceAdapter.test.ts
.\node_modules\.bin\vue-tsc.CMD --noEmit
```

Expected: all adapter cases and strict typecheck pass.

- [ ] **Step 8: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src/services/klineChartsCoreBridge.ts src/services/klineChartsWorkspaceAdapter.ts tests/frontend/klineChartsCoreBridge.test.ts tests/frontend/klineChartsWorkspaceAdapter.test.ts
git commit -m "feat(chart): add KLineCharts workspace adapter"
```

### Task 8: Add revisioned, fingerprint-backed workspace autosave

**Files:**

- Create: `src/services/chartWorkspaceAutosave.ts`
- Create: `tests/frontend/chartWorkspaceAutosave.test.ts`

**Interfaces:**

- Consumes: Task 6 load/save calls and Task 7 adapter captures.
- Produces: one framework-independent application-wide controller that owns the active capture-source registry, five-second cadence, dirty revisions, stale-key queue, global preferences and partial-success acknowledgement.

- [ ] **Step 1: Write failing timer, single-flight and revision tests**

Use Vitest fake timers and this concrete factory (imports come from Task 1's shared fixtures/utilities):

```ts
function makeFixture(options: {
  saveResults?: Array<ChartWorkspaceSaveResult | Promise<ChartWorkspaceSaveResult>>
  firstSaveRejects?: boolean
} = {}) {
  const state = {
    activeKey: key(),
    capturedView: viewState(),
    capturedPreferences: samplePreferences(),
  }
  const results = [...(options.saveResults ?? [])]
  let call = 0
  const save = vi.fn(async (request: SaveChartWorkspaceRequest) => {
    call += 1
    if (options.firstSaveRejects && call === 1) throw new Error('disk full')
    return await (results.shift() ?? {
      ...successfulSave(
        request.viewState?.revision ?? 0,
        request.preferences?.revision ?? 0,
      ),
      key: request.key,
    })
  })
  const autosave = new ChartWorkspaceAutosave({
    save,
    report: vi.fn(),
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
  })
  const source = autosave.registerCaptureSource({
    getActiveKey: () => state.activeKey,
    capture: {
      capture: (_key, viewRevision, preferencesRevision) => ({
        viewState: { ...state.capturedView, revision: viewRevision },
        preferences: { ...state.capturedPreferences, revision: preferencesRevision },
        viewFingerprint: chartViewContentFingerprint(state.capturedView),
        preferencesFingerprint: chartPreferencesContentFingerprint(state.capturedPreferences),
      }),
    },
  })
  source.setActive(true)
  autosave.adoptSnapshot(sampleSnapshot())
  return { state, autosave, save, source }
}
```

Tests mutate `fixture.state.activeKey` / `capturedView` / `capturedPreferences` so the dependency closures see the update:

```ts
it('does not save clean state on a five-second tick', async () => {
  const fixture = makeFixture()
  fixture.autosave.start()
  await vi.advanceTimersByTimeAsync(15_000)
  expect(fixture.save).not.toHaveBeenCalled()
})

it('detects a missed callback from the captured fingerprint', async () => {
  const fixture = makeFixture()
  fixture.autosave.start()
  fixture.state.capturedView = viewState(1, [segment('new')])
  await vi.advanceTimersByTimeAsync(5_000)
  expect(fixture.save).toHaveBeenCalledOnce()
  expect(fixture.save.mock.calls[0]![0].viewState?.revision).toBe(1)
})

it('keeps the newer payload when an older revision completes', async () => {
  const first = deferred<ChartWorkspaceSaveResult>()
  const fixture = makeFixture({ saveResults: [first.promise, successfulSave(2, 0)] })
  fixture.source.markViewDirty()
  const flushing = fixture.autosave.flush('timer')
  await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledOnce())
  fixture.state.capturedView = viewState(2, [segment('newer')])
  fixture.source.markViewDirty()
  first.resolve(successfulSave(1, 0))
  await flushing
  await vi.waitFor(() => expect(fixture.save).toHaveBeenCalledTimes(2))
  expect(fixture.save.mock.calls[1]![0].viewState?.revision).toBe(2)
})

it('retains a serialized old-key payload after navigation', async () => {
  const fixture = makeFixture({ firstSaveRejects: true })
  fixture.state.capturedView = viewState(1, [segment('pending')])
  fixture.source.markViewDirty()
  await fixture.autosave.flush('context')
  fixture.state.activeKey = key('ETHUSDT', '15')
  fixture.state.capturedView = viewStateFor(fixture.state.activeKey, 0, [])
  fixture.autosave.start()
  await vi.advanceTimersByTimeAsync(5_000)
  expect(fixture.save.mock.calls[1]![0].key).toEqual(key('BTCUSDT', '1'))
  expect(fixture.save.mock.calls[1]![0].viewState?.overlays).toEqual([segment('pending')])
})
```

Add these named cases with the stated assertions:

- `single_flight_never_overlaps_save_calls`: hold the first deferred save, call `flush` three times, and assert one call plus a maximum in-flight count of one.
- `queued_flush_drains_immediately_after_settlement`: dirty revision 2 while revision 1 is in flight, resolve revision 1, and assert the second request is issued without advancing the timer and carries revision 2.
- `partial_success_retries_only_the_failed_preferences_part`: return `viewStateSaved: true` and `preferencesSaved: false`, then assert the retry has `viewState: null` and the unchanged preference payload/revision.
- `immediate_kline_only_flush_omits_frontend_state`: call `flush('context')` with clean snapshots and assert `{ viewState: null, preferences: null }` plus the active key.
- `late_key_a_success_never_acknowledges_key_b`: switch to B and dirty it while A is deferred, resolve A, and assert B remains queued and its request contains only B's payload.
- `failed_old_key_drains_before_new_active_key`: fail A, activate and dirty B, then assert the next attempt retries A before the first B attempt and both eventually drain without overlap.
- `failed_kline_flush_retries_on_the_next_tick`: return `klineSaved: false`, advance five seconds, assert a second request for the same key, then return `klineSaved: true` and assert a later clean tick makes no call.
- `stop_cancels_future_ticks`: mark the active source dirty, start, stop before the first tick, advance fifteen seconds, and assert no call.
- `stale_source_cannot_dirty_or_deactivate_the_new_source`: register A then B, activate B, call A's dirty methods and `setActive(false)`, advance a tick, and assert capture/save use B only.
- `global_preferences_have_one_queue_across_sources`: fail a preference save from A, activate B and adopt a newer persisted preference revision, then assert A's stale registration cannot rebase/overwrite it and the next captured B preference is the only queued global payload.
- `capture_key_mismatch_is_reported_and_never_queued`: make the active source key B return a view tagged A, advance a tick, and assert no save call plus one compatibility report.

- [ ] **Step 2: Run the autosave test and confirm RED**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceAutosave.test.ts
```

Expected: import resolution fails because the controller does not exist.

- [ ] **Step 3: Implement the pure controller**

Use dependency injection and these types:

```ts
export interface CapturedWorkspaceState {
  viewState: ChartViewStateV1
  preferences: ChartPreferencesV1
  viewFingerprint: string
  preferencesFingerprint: string
}

export interface ChartWorkspaceCapture {
  capture(
    key: ChartWorkspaceKey,
    viewRevision: number,
    preferencesRevision: number,
  ): CapturedWorkspaceState | null
}

export interface ChartWorkspaceCaptureSource {
  getActiveKey: () => ChartWorkspaceKey
  capture: ChartWorkspaceCapture
}

export interface RegisteredChartWorkspaceCaptureSource {
  setActive(active: boolean): void
  setCaptureEnabled(enabled: boolean): void
  markViewDirty(): void
  markPreferencesDirty(): void
  unregister(): void
}

export interface ChartWorkspaceAutosaveDependencies {
  save: (request: SaveChartWorkspaceRequest) => Promise<ChartWorkspaceSaveResult>
  report: (context: string, error: unknown) => void
  setInterval: typeof globalThis.setInterval
  clearInterval: typeof globalThis.clearInterval
}

export class ChartWorkspaceAutosave {
  constructor(dependencies: ChartWorkspaceAutosaveDependencies)
  registerCaptureSource(
    source: ChartWorkspaceCaptureSource,
  ): RegisteredChartWorkspaceCaptureSource
  adoptSnapshot(snapshot: ChartWorkspaceSnapshot): void
  start(): void
  stop(): void
  flush(reason: ChartWorkspaceFlushReason): Promise<void>
}
```

Maintain keyed view/K-line records separately from one global preference record. Records keep the last serialized payload, not only fingerprints, so a hidden or old key can retry without reading the current canvas:

```ts
interface PendingViewRecord {
  key: ChartWorkspaceKey
  payload: ChartViewStateV1 | null
  viewRevision: number
  viewAckRevision: number
  observedFingerprint: string
  ackFingerprint: string
  forceKlineFlush: boolean
}

interface PendingPreferencesRecord {
  payload: ChartPreferencesV1 | null
  revision: number
  ackRevision: number
  observedFingerprint: string
  ackFingerprint: string
}
```

Required algorithm:

1. `registerCaptureSource` retains multiple keep-mounted charts but exactly one active source. `setActive(false)` clears only its own registration, and `unregister()` cannot clear a newer active source. There is one timer, queue, in-flight promise, and global preference record for the whole app.
2. `adoptSnapshot` seeds that key's view acknowledgement and the single global preference acknowledgement. If an incoming durable revision is greater than or equal to a pending record's revision, replace/acknowledge that older-or-equal pending payload with the loaded durable content; if it is lower, preserve the newer local failed payload. This rule lets an activation load discard a hidden source's stale preference attempt without erasing a genuinely newer unsaved edit.
3. When the active source's own capture flag is enabled, each five-second tick captures it with `chartViewContentFingerprint` / `chartPreferencesContentFingerprint`. Reject/report a capture whose schema is not 1 or whose view symbol/interval does not exactly equal its normalized active key. The registration handle's `markViewDirty` and `markPreferencesDirty` are no-ops unless that exact source is active and capture-enabled; otherwise they increment their revision and set a `needsCapture` flag. The next valid capture serializes at that already-incremented revision, records the content fingerprint, and clears the flag without incrementing again. When no flag is set but the captured content fingerprint differs, treat it as a missed third-party callback, increment exactly once, recapture with that revision, and store the new payload. Never fingerprint `revision` or `savedAtMs`.
4. When the active source is capture-disabled or no source is active during a transition, timer ticks still drain previously serialized failed payloads and `forceKlineFlush` records; they simply do not inspect a hidden canvas. Capture flags belong to registrations, so a stale source cannot re-enable a newer active source.
5. `context`, `page`, and `close` first capture the current active source, then set that key's `forceKlineFlush`. A storage/transport failure is reported and retained but the returned promise resolves, so navigation and normal close are not locked.
6. At request creation, copy the exact key, serialized payloads, revisions and fingerprints into an immutable attempt. Attach the one global preference payload to at most one queued key attempt; if no view key is dirty, use the active source's key.
7. Keep one global in-flight promise. Additional calls set a drain flag; when the promise settles, drain every queued key without overlapping calls. Preserve stable FIFO order by the time a key first became pending: an already-failed old key is retried before a newly captured active key, while re-dirtying an existing key updates its immutable next payload without moving it behind or ahead unpredictably.
8. Clear `forceKlineFlush` only when `klineSaved` is true. Clear view/global-preference dirty state only when that part is saved, the returned durable revision covers the attempt, and the current payload revision/fingerprint still equals the attempt.
9. Keep failed parts, their serialized payloads, and old keys queued. If a part is rejected and its returned durable revision is greater than or equal to the attempted revision, classify it as a revision conflict, rebase that still-current payload to `durableRevision + 1`, and retry; a normal write failure returns a lower durable revision and keeps the same revision. Add `rebases_a_stale_view_after_revision_conflict` and `rebases_global_preferences_after_cross_key_conflict` tests that assert the second request carries the incremented revision and unchanged content.
10. A late result is applied to its immutable attempt record and the single global preference record, never to whichever view key or source happens to be active.
11. `stop()` only clears the timer. Callers that need durability must `await flush(reason)` before `stop()`.
12. Maintain an internal `Set<string>` keyed by operation + workspace key + storage part. Call the injected existing error reporter only on first failure for that key; a successful retry removes the latch so a later independent failure can be reported once again.

- [ ] **Step 4: Rerun autosave tests and strict typecheck**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceAutosave.test.ts
.\node_modules\.bin\vue-tsc.CMD --noEmit
```

Expected: timer, fingerprint, partial failure, old-key retry and concurrency tests pass.

- [ ] **Step 5: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src/services/chartWorkspaceAutosave.ts tests/frontend/chartWorkspaceAutosave.test.ts
git commit -m "feat(chart): add revisioned workspace autosave"
```

### Task 9: Integrate the single-instance fullscreen chart workspace and lifecycle flushes

**Files:**

- Create: `src/components/chart/ChartWorkspacePage.vue`
- Create: `src/composables/useChartWorkspaceCloseGuard.ts`
- Create: `src/composables/useChartWorkspaceAutosaveHost.ts`
- Create: `src/composables/useKlineChartWorkspace.ts`
- Create: `src/services/legacyChartSettingsMigration.ts`
- Modify: `src/components/market/KlineChart.vue`
- Modify: `src/components/layout/TradingLayout.vue`
- Modify: `src/components/layout/AppShell.vue`
- Modify: `src/App.vue`
- Delete: `src/services/chartSettingsService.ts`
- Delete: `tests/frontend/chartSettings.test.ts`
- Create: `tests/frontend/klineChartWorkspace.test.ts`
- Create: `tests/frontend/chartWorkspacePage.test.ts`
- Create: `tests/frontend/chartWorkspaceCloseGuard.test.ts`
- Create: `tests/frontend/legacyChartSettingsMigration.test.ts`
- Modify: `tests/frontend/accountNavigation.test.ts`
- Modify: `tests/frontend/appAccountEventHandlers.test.ts`

**Interfaces:**

- Consumes: Tasks 6-8, existing `AppShell` navigation, existing market/config stores, Tauri window lifecycle and KLineCharts Pro.
- Produces: the user-visible PRD-08 page, one persistent Pro instance per chart surface, one application-wide autosave controller, immediate local restoration, shared context synchronization and best-effort page/close flushing.

- [ ] **Step 1: Write failing workspace layout and navigation-order tests**

Mount the existing `AppShell` at its real default page, stub expensive page children, and navigate by emitting the existing `NavigationRail` `select` event:

```ts
async function selectPage(wrapper: VueWrapper, page: NavKey): Promise<void> {
  wrapper.findComponent(NavigationRail).vm.$emit('select', page)
  await flushPromises()
}

it('shows only the full-size chart page and hides the real Sidebar', async () => {
  const wrapper = mountShell()
  await selectPage(wrapper, 'charts')
  const page = wrapper.findComponent(ChartWorkspacePage)
  expect(page.exists()).toBe(true)
  expect(wrapper.findComponent(Sidebar).exists()).toBe(false)
  expect(wrapper.findComponent(TradingLayout).exists()).toBe(false)
  expect(page.findComponent(AppCard).exists()).toBe(false)
})

it('keeps the same workspace instance across account navigation', async () => {
  const wrapper = mountShell()
  await selectPage(wrapper, 'charts')
  const first = wrapper.findComponent(ChartWorkspacePage).vm
  await selectPage(wrapper, 'account')
  await selectPage(wrapper, 'charts')
  expect(wrapper.findComponent(ChartWorkspacePage).vm).toBe(first)
})

it('keeps the same trading chart after it has been visited', async () => {
  const wrapper = mountShell()
  await selectPage(wrapper, 'trading')
  const first = wrapper.findComponent(TradingLayout).vm
  await selectPage(wrapper, 'charts')
  await selectPage(wrapper, 'trading')
  expect(wrapper.findComponent(TradingLayout).vm).toBe(first)
})

it('waits for page flush and applies only the latest navigation intent', async () => {
  const pending = deferred<void>()
  vi.mocked(flushActiveChartWorkspace).mockReturnValueOnce(pending.promise)
  const wrapper = mountShell()
  wrapper.findComponent(NavigationRail).vm.$emit('select', 'charts')
  wrapper.findComponent(NavigationRail).vm.$emit('select', 'account')
  expect(wrapper.findComponent(AccountCenterPage).exists()).toBe(false)
  pending.resolve()
  await flushPromises()
  expect(wrapper.findComponent(AccountCenterPage).exists()).toBe(true)
})
```

Add a rejection case showing a reported flush failure still navigates. Update the existing account navigation test so `TopBar` and `NavigationRail` remain mounted and only the charts route hides `Sidebar`.

- [ ] **Step 2: Write failing `KlineChart` integration tests**

Mock Pro, the datafeed, bridge, workspace adapter, the provided application autosave and persistence services. Add named tests proving:

- workspace passes `drawingBarVisible: true`, hides the legacy external settings toolbar, and trading passes `false`;
- each keep-mounted chart creates exactly one `KLineChartPro` across `active=false/true` and context changes;
- `primeHistory` runs before Pro's initial history promise resolves, then the first data-ready gate restores overlays/viewport;
- a REST refresh that resolves before the initial data-ready event is buffered and applied only after persisted overlays/viewport finish restoring;
- multiple early refresh ranges are timestamp-merged rather than dropping an older range when a newer request finishes first;
- initial `load_chart_workspace` failure reports once, uses version-one defaults, still constructs Pro and permits REST history;
- activation suspends capture, clears/replaces the old keyed overlays, primes the new snapshot, programmatically suppresses intermediate symbol/period requests, and resumes only after the matching restore gate;
- selector, Pinia watcher, and activation requests share one serialized transition queue; a watcher echo for the in-flight key does not duplicate load/restore, and rapid A→B→C applies only C;
- if Pro does not request the expected final key within 250 ms, `applyPrimedHistoryFallback` is invoked for that generation;
- a generation token discards late key-A snapshot and REST-refresh results after B is active;
- REST refresh calls `applyRefreshedHistory` only for the current active key/generation;
- a bridge failure reports one compatibility diagnostic but leaves Pro/datafeed mounted and permits K-line-only flushes;
- workspace and trading sources share the exact same autosave instance; only the active registration can mark/capture, while a hidden source cannot overwrite global preferences;
- hidden mode disables that source's canvas capture while the application timer keeps retrying already-serialized failed payloads;
- `ResizeObserver` calls adapter `resize`, reactivation calls it after `nextTick` to recover from `display: none`, and the observer disconnects only on final component unmount.

In `legacyChartSettingsMigration.test.ts`, add `migrates_legacy_preferences_and_every_safe_viewport_before_first_chart_load`, `does_not_overwrite_a_nonzero_rust_revision`, `keeps_the_legacy_key_after_any_failed_view_or_preference_part`, `removes_the_legacy_key_only_after_all_targets_are_durable`, and `malformed_legacy_json_falls_back_without_blocking_chart_startup`. Assert exact save requests/revisions and exact `localStorage.removeItem` call counts.

- [ ] **Step 3: Implement and verify the one-time legacy migration**

Implement `migrateLegacyChartSettingsOnce(fallbackKey)` as one module-level shared promise so the two keep-mounted charts cannot race. It may read only `easiflux.chart-settings.v1`; normalize the existing version-one indicator arrays and every safe, supported `SYMBOL:INTERVAL` viewport, load each target's Rust snapshot, and save revision 1 only when that target/global durable revision is still 0. Existing nonzero Rust content always wins. The obsolete `layout` toggle is intentionally not migrated because the two modes now have fixed drawing-bar behavior. Treat K-line save status as unrelated; remove the legacy key only after every required view/preferences part reports durable success. On parse, invoke, or partial failure retain the key for a later launch and report once without blocking chart construction.

Run:

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/legacyChartSettingsMigration.test.ts
```

Expected: every migration/retention case passes.

- [ ] **Step 4: Refactor `KlineChart.vue` around a stable workspace instance**

Add explicit props:

```ts
const props = withDefaults(defineProps<{
  mode?: 'trading' | 'workspace'
  active?: boolean
}>(), {
  mode: 'trading',
  active: true,
})
```

Move orchestration into `useKlineChartWorkspace.ts` so `KlineChart.vue` remains the host/presentation component. Both visible modes use the Rust workspace state; workspace mode fixes the complete drawing bar on, while trading mode fixes it off and relies on Pro's built-in indicator controls. The old external compact/standard toggle is removed because Pro 0.1.1 has no safe live drawing-bar setter and rebuilding leaks its discarded Solid root. Indicators and viewport continue to persist through Rust.

Reduce the component template to the root `.chart` and one `.chart-view` host—remove the entire legacy `.chart-toolbar`/Naive UI settings popover in both modes. Add a mode class; `.chart--workspace` must set `min-height: 0` so the existing trading-oriented `clamp(320px, 42vh, 620px)` cannot overflow the dedicated page. The host and `.klinecharts-pro` remain `width/height: 100%` with `min-width/min-height: 0`.

Use this exact lifecycle:

All selector callbacks, Pinia key watchers, and activation syncs first update one normalized `desiredKey`, increment a monotonic generation, and enter one `transitionTail`. At every await boundary, stale generations stop before mutating the adapter/Pro. If a watcher reports the same key already in flight, it joins that transition rather than starting a second load. Only the final generation may arm a restore token or re-enable its capture source.

1. Await the singleton `migrateLegacyChartSettingsOnce(currentKey)` before the first workspace load, then normalize the current shared key, increment `generation`, and load its snapshot before constructing Pro. On migration/load failure, report once, preserve the legacy key, and continue with the normal Rust snapshot or version-one defaults plus empty local bars.
2. Construct one `EasiKlineDatafeed`, prime it with the snapshot, then construct `KLineChartPro` exactly once with saved indicators and `drawingBarVisible: props.mode === 'workspace'`.
3. Immediately recover/attach Task 7 and call `prepareViewReplacement` before the primed async history result can reach Pro. Pro alone performs the first `applyNewData`.
4. Inject the Task 8 application autosave, call `adoptSnapshot`, and register this adapter/key as one capture source. `setActive(props.active)` changes the autosave's active source pointer and capture enablement; adapter dirty callbacks call only their registration handle, so a hidden chart cannot mark the active chart dirty. Do not create a timer or a second autosave inside `KlineChart`.
5. A visible Pro selector calls a coordinator handler that awaits `marketStore.setChartContext(key)`, loads/primes that key's snapshot, and prepares its view before returning to datafeed.
6. A shared-store change while hidden only records the desired key. On activation, suspend capture, load the latest snapshot, remove old registered overlays through `prepareViewReplacement`, arm `expectProgrammaticContext`, and call only the necessary Pro `setSymbol`/`setPeriod` methods. Intermediate requests are suppressed. If `waitForExpectedContext(..., 250)` is false—or no setter was needed—call `applyPrimedHistoryFallback`. Resume capture only after the matching generation's restore promise resolves.
7. `onHistoryRefresh` rejects stale/hidden generations and timestamp-merges all early refresh bars for the active generation (later completion wins only for duplicate timestamps). Never call `applyRefreshedHistory` while that generation's persisted restore promise is unresolved; after the matching initial/context restore completes, re-check active key/generation and apply the merged buffer once. Later matching refreshes call it directly. This prevents a fast REST promise from consuming or reordering the first `OnDataReady` gate without dropping concurrent load-more ranges. `applyRefreshedHistory` preserves realtime bars and viewport.
8. Existing realtime Pinia changes push only the last bar plus its normalized market key through datafeed's single-bar subscription path. The datafeed drops it unless that exact Pro subscription key matches, so a hidden chart on an old key cannot ingest the active market's candle. Keep the project's current dark/`zh-CN` Pro values; do not claim nonexistent theme/locale watchers. Bind timezone only if the existing application config already exposes it.
9. Observe the actual chart container and call adapter `resize()`. When `active` becomes true, await `nextTick()` and resize again after `v-show` has restored dimensions; never call `replaceChildren`, rebuild Pro, or manually mutate its private facade.

On final component unmount, disable/unregister only its capture source, cancel its generation, detach the adapter and disconnect `ResizeObserver`. The app-level host owns the autosave timer/flusher lifetime. Do not remove/recreate the Pro DOM during ordinary route changes.

- [ ] **Step 5: Implement the dedicated page and keep-mounted route**

`ChartWorkspacePage.vue` is intentionally thin:

```vue
<script setup lang="ts">
defineProps<{ active: boolean }>()
</script>

<template>
  <section class="chart-workspace-page" data-testid="chart-workspace-page">
    <KlineChart mode="workspace" :active="active" />
  </section>
</template>
```

It must use flex/grid `min-width: 0`, `min-height: 0`, `width: 100%`, and `height: 100%`; do not wrap the chart in `AppCard` or add a page title.

`useChartWorkspaceAutosaveHost.ts` exposes the ownership boundary explicitly:

```ts
const chartWorkspaceAutosaveKey: InjectionKey<ChartWorkspaceAutosave> =
  Symbol('chart-workspace-autosave')

export function useChartWorkspaceAutosaveHost(): ChartWorkspaceAutosave
export function useChartWorkspaceAutosave(): ChartWorkspaceAutosave
```

The host function constructs, provides, starts/stops, and registers the singleton; the consumer function injects it and throws a descriptive configuration error when no host exists. In tests, mounting `KlineChart` alone supplies a fake controller with Vue `global.provide`.

In `AppShell.vue`:

- call `useChartWorkspaceAutosaveHost()` exactly once; it constructs and provides one `ChartWorkspaceAutosave`, registers its `flush` as the sole app durability handler, and starts one timer on mount. On app-shell unmount it unregisters and stops; the close guard remains the only lifecycle path that awaits the final flush before window destruction;
- retain the existing top bar and primary navigation;
- render `Sidebar` only when the real `activePage !== 'charts'`;
- set `chartsVisited` and `tradingVisited` on first entry;
- mount both Pro-bearing pages once with `v-if="...Visited"`, thereafter use `v-show` and pass `:active="activePage === ..."`; this avoids repeated undisposable Pro roots;
- make `navigateTo` async with a `navigationTail` promise plus monotonic generation, best-effort `await flushActiveChartWorkspace('page')` before changing `activePage`, and last-intent-wins semantics; queued calls continue after a caught predecessor failure;
- ensure trading/account content cannot render underneath the chart page.

Add `active: boolean` to `TradingLayout.vue` and pass it to `<KlineChart mode="trading" :active="active" />`.

- [ ] **Step 6: Write failing close-guard tests**

Mock `getCurrentWindow().onCloseRequested`, `destroy`, and the registry:

```ts
it('prevents close, awaits one flush, then destroys the window', async () => {
  const fixture = closeGuardFixture()
  await fixture.fireClose()
  expect(fixture.event.preventDefault).toHaveBeenCalledOnce()
  expect(flushAllChartWorkspaces).toHaveBeenCalledWith('close')
  expect(fixture.window.destroy).toHaveBeenCalledOnce()
})

it('still closes after a reported flush failure and does not recurse', async () => {
  const fixture = closeGuardFixture({ flushError: new Error('disk full') })
  await fixture.fireClose()
  expect(fixture.report).toHaveBeenCalledOnce()
  expect(fixture.window.destroy).toHaveBeenCalledOnce()
  await fixture.fireClose()
  expect(fixture.secondEvent.preventDefault).toHaveBeenCalledOnce()
  expect(fixture.window.destroy).toHaveBeenCalledOnce()
})

it('reports a destroy failure and allows a later close retry', async () => {
  const fixture = closeGuardFixture({ destroyError: new Error('window busy') })
  await fixture.fireClose()
  expect(fixture.report).toHaveBeenCalledWith(
    expect.any(Error),
    '关闭图表窗口失败',
  )
  fixture.window.destroy.mockResolvedValueOnce(undefined)
  await fixture.fireClose()
  expect(fixture.window.destroy).toHaveBeenCalledTimes(2)
})
```

- [ ] **Step 7: Implement and install the close guard**

Export:

```ts
export function useChartWorkspaceCloseGuard(): void
```

On mount, register `onCloseRequested`. Every close request calls `event.preventDefault()` synchronously. Track `idle | flushing | destroying | destroyed`: non-idle requests return without starting duplicate work; an idle request enters `flushing`, awaits `flushAllChartWorkspaces('close')`, reports any aggregate/unexpected error once, then enters `destroying` and awaits `getCurrentWindow().destroy()`. Success enters `destroyed`; a destroy rejection is reported separately and returns to `idle` so the user can retry. Dispose the listener on app unmount. Install the composable once from `App.vue`; do not install it inside each chart instance.

Mock `useChartWorkspaceCloseGuard` (or `@tauri-apps/api/window`) in `appAccountEventHandlers.test.ts` before mounting `App.vue`, so the existing browser-like unit test never invokes a real Tauri window API.

- [ ] **Step 8: Run focused UI and lifecycle tests**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/klineChartWorkspace.test.ts tests/frontend/chartWorkspacePage.test.ts tests/frontend/chartWorkspaceCloseGuard.test.ts tests/frontend/legacyChartSettingsMigration.test.ts tests/frontend/accountNavigation.test.ts tests/frontend/appAccountEventHandlers.test.ts
```

Expected: the fullscreen boundary, stable instance, local-first restoration, stale-result guard, resize and close order all pass.

- [ ] **Step 9: Remove legacy chart settings writers and run the full frontend suite**

Delete `src/services/chartSettingsService.ts` and its focused test only after `rg "chartSettingsService|easiflux.chart-settings.v1" src tests/frontend` shows the migration module is the sole remaining key reader and no legacy writer remains. Do not remove unrelated instrument caching or other browser storage.

Run:

```powershell
.\node_modules\.bin\vitest.CMD run
.\node_modules\.bin\vue-tsc.CMD --noEmit
```

Expected: all frontend tests and strict typecheck pass.

- [ ] **Step 10: Record the deferred checkpoint**

Do not run without explicit user authorization:

```powershell
git add src/components/chart/ChartWorkspacePage.vue src/composables/useChartWorkspaceCloseGuard.ts src/composables/useChartWorkspaceAutosaveHost.ts src/composables/useKlineChartWorkspace.ts src/services/legacyChartSettingsMigration.ts src/components/market/KlineChart.vue src/components/layout/TradingLayout.vue src/components/layout/AppShell.vue src/App.vue tests/frontend/klineChartWorkspace.test.ts tests/frontend/chartWorkspacePage.test.ts tests/frontend/chartWorkspaceCloseGuard.test.ts tests/frontend/legacyChartSettingsMigration.test.ts tests/frontend/accountNavigation.test.ts tests/frontend/appAccountEventHandlers.test.ts
git add -u src/services/chartSettingsService.ts tests/frontend/chartSettings.test.ts
git commit -m "feat(chart): add fullscreen chart workspace"
```

### Task 10: Run complete verification and manual recovery smoke tests

**Files:**

- Verify only; do not intentionally modify source files.

**Interfaces:**

- Consumes: every task above.
- Produces: reproducible evidence for the acceptance criteria, plus a clean scope report that excludes the user's unrelated credential-editor changes and `.pnpm-store/`.

- [ ] **Step 1: Run all focused PRD-08 frontend tests together**

```powershell
.\node_modules\.bin\vitest.CMD run tests/frontend/chartWorkspaceTypes.test.ts tests/frontend/chartWorkspaceService.test.ts tests/frontend/chartWorkspaceSwitch.test.ts tests/frontend/easiKlineDatafeed.test.ts tests/frontend/klineChartsCoreBridge.test.ts tests/frontend/klineChartsWorkspaceAdapter.test.ts tests/frontend/chartWorkspaceAutosave.test.ts tests/frontend/klineChartWorkspace.test.ts tests/frontend/chartWorkspacePage.test.ts tests/frontend/chartWorkspaceCloseGuard.test.ts tests/frontend/legacyChartSettingsMigration.test.ts tests/frontend/accountNavigation.test.ts tests/frontend/appAccountEventHandlers.test.ts
```

Expected: all PRD-08 contract, datafeed, adapter, autosave and UI tests pass in one process.

- [ ] **Step 2: Run the complete frontend gates**

```powershell
pnpm lint
pnpm test
pnpm build
```

If pnpm aborts before executing because of the known non-interactive modules-directory or ignored-builds prompt, run the already-installed local binaries for diagnosis and report the pnpm environment failure separately; do not silently claim the requested pnpm gate passed.

- [ ] **Step 3: Run Rust formatting and all backend gates**

From `src-tauri`:

```powershell
cargo fmt --all -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: formatting, all Rust tests and Clippy pass.

- [ ] **Step 4: Verify diff hygiene and exact scope**

From the repository root:

```powershell
git diff --check
git status --short --branch
git diff --name-only
git ls-files --others --exclude-standard
```

Inspect every PRD-08 path and explicitly confirm these pre-existing items were not edited, staged or included by this work:

```text
src/components/account/CredentialEditor.vue
src/components/settings/SettingsDialog.vue
tests/frontend/settingsCredentialEditor.test.ts
.pnpm-store/
```

Because the design and plan are untracked until authorized, also inspect their whitespace/content directly; `git diff --check` alone does not cover untracked files.

- [ ] **Step 5: Perform a Tauri manual smoke test**

Run only in an interactive implementation session where launching the app is allowed:

```powershell
pnpm tauri dev
```

Verify this exact sequence:

1. Open **图表**: top bar and primary nav remain; secondary sidebar and every trading panel are absent; one chart fills the remainder.
2. Draw, move, style, lock and delete overlays; change indicators and viewport.
3. Wait more than five seconds, leave and return: one Pro instance is reused and state is intact.
4. Switch symbol and period from Pro; confirm the trading page shows the same shared context.
5. Draw distinct overlays on key A and key B, switch A→B→A, and confirm neither overlays nor viewport leak across keys.
6. Disconnect the network and re-enter: local K-lines/drawings/preferences/viewport appear before any failed REST request.
7. Reconnect: history fills without duplicates and realtime updates continue.
8. Close normally and reopen: the last state is restored.

Use the temp-root Rust tests as the default corruption/recovery evidence. A real restart cannot validate an unbound “disposable copy.” Do not alter the live application data directory for a manual corruption test unless the user separately authorizes the exact file; if authorized, back it up first, use a dedicated non-production symbol/interval view, and restore it immediately afterward.

- [ ] **Step 6: Report results without committing**

Summarize:

- exact commands and pass/fail counts;
- manual cases actually exercised versus not exercised;
- any environment-only blockers;
- PRD-08 changed/untracked paths;
- confirmation that unrelated dirty paths remain untouched;
- current branch and `HEAD`.

Do not run `git add`, `git commit`, `git push`, merge, or create a PR unless the user separately authorizes it.
