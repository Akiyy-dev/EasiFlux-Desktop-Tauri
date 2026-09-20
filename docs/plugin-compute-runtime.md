# 本地数值计算插件（Manifest v4）

Manifest v4 在既有受管本地清单流程中增加一个受限动作：
`sandbox.computeSeries`。它只运行无导入的 WebAssembly 数值模块；这不是
通用插件 SDK、WASI、JavaScript/npm 执行环境，也不代表发布者已经签名或可信。

可直接审阅和导入的示例见
[Series SMA](../examples/plugins/series-sma/README.md)。其中平均值循环位于 WAT
来宾代码，不是宿主内置的 SMA 公式。

## 清单契约

```json
{
  "kind": "command",
  "contributionId": "series.sma",
  "title": "Compute simple moving average",
  "actionId": "sandbox.computeSeries",
  "params": {
    "runtime": "wasm-v1",
    "abi": "series-f64-v1",
    "moduleBase64": "<canonical padded standard Base64>",
    "parameter": { "label": "Period", "default": 3, "min": 1, "max": 4096 }
  }
}
```

- 只有 `runtime: "wasm-v1"` 与 `abi: "series-f64-v1"` 可用。
- `moduleBase64` 必须是规范的带填充标准 Base64；解码后非空、不超过 8192
  字节，并以 WebAssembly v1 文件头开始。整个清单仍不得超过 16 KiB。
- 参数元数据必须是 -1000000 到 1000000 内的安全整数，并满足
  `min <= default <= max`；用户实际输入可以是范围内有限小数，由算法自行决定
  是否接受。
- `requestedCapabilities` 必须保持 `[]`。未知字段、重复字段、动作与参数混用
  都会拒绝。

## 固定来宾 ABI

模块不得有任何 import 或 start 函数，且必须导出：

```text
memory
alloc(bytes: i32) -> i32
run(ptr: i32, count: i32, parameter: f64) -> f64
```

宿主接受 1–4096 个有限 `f64`，以 little-endian 连续写入来宾内存；写入前会
检查分配指针及长度。结果只接受一个有限 `f64`。每次 Run 都创建全新的实例和
store，因此来宾不能依赖上一次运行的内存或全局状态。

宿主表单的序列文本最多 128 KiB，以逗号或空白分隔；每项必须是十进制或科学
计数法的有限数值。空输入、十六进制、`NaN`、`Infinity`、额外字符和超过 4096
项都会在进入后端前拒绝。参数框只接受一个有限数值，并必须落在清单声明的闭区间；
它不因元数据是整数就自动取整。

## 执行与生命周期

导入、预览、启用、搜索、渲染和导航都不会运行来宾。只有插件当前可用、内容
身份与启用决定一致、没有待协调生命周期状态，并且用户在计算表单中明确点击
**Run** 后，后端才按当前目录解析模块。模块字节、路径和导出名不能由 WebView
执行请求替换。

同一进程一次只允许一个计算 worker。关闭表单、离开上下文、停用、移除、重扫
或内容身份变化会请求取消或使结果失效；worker 真正退出前仍占用槽位。取消是
协作式的，不保证瞬间停止。前端也会丢弃过期或来自旧请求的返回值，不会自动重试。

输入和结果只保存在内存中，不写入清单、插件状态或日志。应用不会把账户、凭据、
行情订阅、订单、文件路径或其他业务数据自动提供给来宾。

## 资源边界

| 项目 | 边界 |
| --- | --- |
| 输入 | 1–4096 个有限 `f64`；原始文本最多 128 KiB |
| 模块 | 解码后 1–8192 字节 |
| 线性内存 | 最多 1 MiB，一个 memory |
| 执行 fuel | 总计 2,000,000；每 10,000 fuel 检查一次 |
| 协作式期限 | 2 秒，在 fuel 切片之间检查 |
| 并发 | 全进程一个 worker |

结构预检还限制：最多 128 个类型、128 个函数、64 个全局、32 个导出；单函数最多
16 个参数和 1 个结果；最多 256 个 local 声明/locals；最多 32 个 data segment。
只允许一个 32-bit、非 shared 的 memory；初始页数和已声明的最大值都不得超过
16 页。未声明最大值仍由宿主 store 限制在 1 MiB，但作者示例应明确声明最大值。
table、element、import、start、tag、component 和未知 section 均拒绝。

当前引擎只开放 ABI 所需的 MVP 数值操作和浮点数；不支持 mutable globals、sign
extension、saturating float-to-int、multivalue、multimemory、bulk memory、reference
types、tail calls、extended const、custom page sizes、wide arithmetic、SIMD 或 memory64。
调用栈初始 1024 字节、最多 65536 字节，递归上限 64。上游严格校验还要求：当
函数体总字节达到 1000 时，平均函数体至少 40 字节；大量极小函数可能因此拒绝，
即使尚未触及函数数量上限。

编译受模块字节数和结构限制，但不是可中断的硬实时操作。解释器 store 限额也不是
整个进程的内存保证；这是进程内解释器隔离，不是独立 OS 沙箱进程。

## 固定失败代码

界面按固定代码显示安全消息，不显示来宾 trap、模块字节、路径或用户输入。当前
计算域代码如下：

下表适用于已经通过 Tauri 参数解码的业务请求。错误 JSON 类型、未知字段或重复字段
可在进入命令前由原生参数层拒绝，不保证使用 `plugin_compute_*` 代码。

| 代码 | 含义 |
| --- | --- |
| `plugin_compute_invalid_request` | 请求 ID、插件/贡献 ID 或 generation/revision 计数器无效 |
| `plugin_compute_invalid_input` | 序列为空、过长或含非有限值 |
| `plugin_compute_invalid_parameter` | 参数非有限或超出清单范围 |
| `plugin_compute_unavailable` | 目录、生命周期或当前插件状态不可执行 |
| `plugin_compute_disabled` | 当前内容未启用 |
| `plugin_compute_not_found` | 插件或贡献在当前目录中不存在 |
| `plugin_compute_not_supported` | 当前目标不是受支持的 v4 计算贡献 |
| `plugin_compute_stale` | generation、revision 或内容身份已经变化 |
| `plugin_compute_busy` | 已有 worker 尚未退出 |
| `plugin_compute_cancelled` | 匹配请求已协作式取消 |
| `plugin_compute_invalid_module` | import、start、结构或资源声明不受支持 |
| `plugin_compute_invalid_abi` | 固定导出缺失或类型不匹配 |
| `plugin_compute_memory` | 分配地址或输入写入越界 |
| `plugin_compute_budget` | fuel 耗尽 |
| `plugin_compute_deadline` | 协作式两秒期限耗尽 |
| `plugin_compute_trap` | 来宾执行 trap |
| `plugin_compute_invalid_output` | 来宾返回 NaN 或无穷值 |
| `plugin_compute_internal` | 宿主无法安全完成且无更具体公开原因 |

## 不支持的功能

v4 不支持 WebAssembly imports、WASI、网络、文件或操作系统 API、Tauri IPC、
账户/凭据、实时行情订阅、交易、后台任务、任意插件 UI、JS/npm 包、原生动态库、
自选导出名、持久来宾状态或多运行时。签名、市场发布、自动更新和任意第三方插件
兼容性仍是后续工作。
