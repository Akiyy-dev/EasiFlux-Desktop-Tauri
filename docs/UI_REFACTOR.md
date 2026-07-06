# UI 重构开发规范

> PRD-00 落地文档。本次仅重构前端 UI 与交互，不修改业务逻辑、接口调用、数据结构及状态管理。

## 技术栈（React 迁移方向）

| 层级 | 选型 | 状态 |
|------|------|------|
| 框架 | React 19 + TypeScript | 脚手架已就绪，未挂载 |
| 样式 | Tailwind CSS v4 | 已接入 `global.css` |
| 组件 | shadcn/ui (new-york) | 基础配置 + Button |
| 图标 | Lucide React（React）/ lucide-vue-next（Vue） | 已统一 |
| 动画 | Motion（React）/ CSS motion tokens（Vue） | 已统一 |
| 表格 | TanStack Table | 依赖已安装 |
| 表单 | React Hook Form | 依赖已安装 |
| 状态 | Zustand | 依赖已安装，暂不替换 Pinia |

当前运行入口仍为 **Vue 3 + Pinia + Naive UI**（`src/main.ts` → `src/App.vue`）。React 代码位于 `src/react/`，按 PRD 分步迁移后切换入口。

## 目录约定

```
src/assets/styles/
  tokens.css            # 颜色 / 间距 / 圆角 / 阴影 / 字体 tokens
  components.css        # Card / Button / Table / Motion / a11y
  global.css            # Tailwind + imports
src/components/ui/        # Vue Design System（PRD-05 / PRD-07）
  AppButton.vue           # Primary / Secondary / Ghost / Danger
  AppCard.vue             # 统一 Card 容器
  AppDialog.vue           # Modal 封装
  AppIcon.vue             # Lucide 图标封装
  AppTabs.vue             # 统一 Tab 栏
  AppText.vue             # 排版组件
  AppTooltip.vue          # Tooltip 封装
  MonoValue.vue           # 等宽数字展示
src/composables/
  useUiMotion.ts          # 动画 class 预设
src/constants/
  designColors.ts         # Naive UI 色板
  naiveTheme.ts           # Naive UI 全组件主题
src/react/
  index.ts              # React 模块统一导出
  lib/utils.ts          # cn() 等工具
  components/ui/        # shadcn 基础组件
  layout/               # App Shell 布局原语
  hooks/                # 共享 React hooks
src/assets/styles/
  global.css            # Design Tokens + Tailwind + 遗留 Vue 样式
components.json         # shadcn CLI 配置
```

新增 shadcn 组件：

```bash
pnpm dlx shadcn@latest add <component>
```

## Design System 规则

1. **禁止页面自行定义视觉样式** — 颜色、圆角、阴影、间距必须从 tokens 或 shadcn 组件获取。
2. **统一深色主题** — 交易终端默认深色；tokens 定义在 `global.css` 的 `:root`。
3. **保留遗留变量** — `--bg-primary` 等 Vue 用变量在迁移完成前不得删除。
4. **交易语义色** — 涨 `--up` / 跌 `--down`，对应 Tailwind `text-up` / `text-down`。
5. **Card 化布局** — Vue 层使用 `AppCard`；React 层使用 `bg-card border border-border rounded-lg`。
6. **图标** — 禁止混用风格；Vue 用 `lucide-vue-next` + `AppIcon`，React 用 `lucide-react`。
7. **数字** — 行情/价格/数量使用 `MonoValue` 或 `.ef-mono`（等宽 + tabular-nums）。
8. **避免** — 毛玻璃、复杂渐变、花哨动画、过度阴影。

### 间距与圆角

| Token | 值 |
|-------|-----|
| `--ef-space-1` ~ `--ef-space-8` | 4px ~ 32px |
| `--ef-radius-sm` ~ `--ef-radius-xl` | 4px ~ 12px |
| `--radius` | shadcn 默认 6px |

### 动画

| Token | 值 |
|-------|-----|
| `--ef-duration-fast` | 120ms |
| `--ef-duration-normal` | 200ms |
| `--ef-ease-out` | cubic-bezier(0.16, 1, 0.3, 1) |

Vue 层通过 `useUiMotion.ts` 导出 class 预设（`ef-motion-hover`、`ef-motion-tab`、`ef-motion-sidebar` 等），与 Motion 的「快、自然」原则一致。Dialog / Toast 由 Naive UI `themeOverrides` 对齐圆角与字体。

### 响应式

- 交易区三栏在 `1100px` / `820px` 断点自动重排，优先保证 K 线与下单区空间。
- 根容器 `overflow: hidden`，禁止页面级水平滚动条。
- 行情栏指标区允许内部横向滚动（隐藏滚动条），避免文字遮挡。

## PRD-05 落地范围

| 项 | 实现 |
|----|------|
| 字体 | `--font-sans` / `--font-mono` + `MonoValue` |
| 图标 | `NavigationRail`、`TopBar`、`Sidebar` 迁移至 Lucide |
| Card | `AppCard` 统一图表/深度/交易/订单/占位页 |
| 动画 | CSS motion tokens + Tab 淡入 |
| 响应式 | `global.css` 断点 + `AppShell` 溢出控制 |
| Naive 对齐 | `naiveTheme.ts` 覆盖 Dialog / Message |

## PRD-06 落地范围

| 项 | 实现 |
|----|------|
| 默认页 | 启动后默认进入 `home` |
| 布局 | `DashboardPage` 卡片化分区 |
| 资产 | 读取 account / position store |
| 行情 | `fetch_ticker` 拉取 BTC/ETH/SOL |
| 快捷入口 | 跳转交易 / 账户 / 新闻 / 插件 |
| 动态 | 静态占位，预留 RSS |
| 状态栏 | API / WS / 版本实时展示 |

## PRD-07 落地范围

| 类别 | 实现 |
|------|------|
| Theme | `tokens.css` 语义色 + Dark 默认 + Light 预留 |
| 字体 | `AppText` + `MonoValue` + typography tokens |
| 按钮 | `AppButton`（primary/secondary/ghost/danger） |
| 表单 | Naive UI 全组件 `naiveTheme.ts` 对齐 |
| 表格 | `.ef-table-*` 统一样式 + `TanstackDataTable` |
| 弹层 | `AppDialog` / `AppTooltip` + Naive Drawer/Popover 主题 |
| 动画 | `ef-motion-*` 含 page 切换 |
| a11y | `ef-focus-ring` 焦点环 + 按钮 `aria-*` |

### 颜色 Token 速查

| Token | 用途 |
|-------|------|
| `--background` / `--surface` / `--card` | 背景层级 |
| `--border` | 边框 |
| `--primary` | 主操作 |
| `--success` / `--danger` / `--warning` | 状态语义 |
| `--text` / `--text-secondary` | 正文 / 次要文字 |

## 目标布局（App Shell）

```
TopBar
  ↓
Navigation Rail（一级导航）
  ↓
Sidebar（二级导航）
  ↓
Main Content（主内容区）
```

参考实现：`src/react/layout/AppShellScaffold.tsx`（未挂载，仅供后续 PRD 引用）。

## 迁移原则

1. 每个 PRD 只迁移其范围内的 UI，不改业务逻辑。
2. 新页面使用 `src/react/components/ui/` 组件，不重复实现。
3. Pinia stores 与 Tauri commands 保持不变，React 层通过 composable/adapter 复用。
4. 发现结构与 PRD 冲突时，优先遵循 PRD，渐进迁移，不删除现有功能。

## 参考产品（仅布局与交互）

TradingView Desktop、OKX Desktop、Binance Desktop、VS Code、Cursor、Linear — 不模仿配色与品牌元素。
