# UI 重构开发规范

> PRD-00 落地文档。本次仅重构前端 UI 与交互，不修改业务逻辑、接口调用、数据结构及状态管理。

## 技术栈（React 迁移方向）

| 层级 | 选型 | 状态 |
|------|------|------|
| 框架 | React 19 + TypeScript | 脚手架已就绪，未挂载 |
| 样式 | Tailwind CSS v4 | 已接入 `global.css` |
| 组件 | shadcn/ui (new-york) | 基础配置 + Button |
| 图标 | Lucide React | 依赖已安装 |
| 动画 | Motion | 依赖已安装 |
| 表格 | TanStack Table | 依赖已安装 |
| 表单 | React Hook Form | 依赖已安装 |
| 状态 | Zustand | 依赖已安装，暂不替换 Pinia |

当前运行入口仍为 **Vue 3 + Pinia + Naive UI**（`src/main.ts` → `src/App.vue`）。React 代码位于 `src/react/`，按 PRD 分步迁移后切换入口。

## 目录约定

```
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
5. **Card 化布局** — 使用 `bg-card border border-border rounded-lg`。
6. **避免** — 毛玻璃、复杂渐变、花哨动画、过度阴影。

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
