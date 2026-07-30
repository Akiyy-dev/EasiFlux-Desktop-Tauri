# PRD-09 新闻中心（News Center）设计

日期：2026-07-30

状态：设计章节已由用户逐项确认，等待整篇规格复核

工作分支：`news/createnewscenter`

基线：`main` / `origin/main` / 本分支均为 `953f633bd058ff87b71cc593843567b44c0598f0`

## 1. 需求权威与已确认决策

本规格以用户在本轮明确提供并逐项确认的要求为权威。实现不得用产品偏好替换这些决策。

- 新闻数据只来自固定内置的 [Akiyy-dev/TG-forwarder](https://github.com/Akiyy-dev/TG-forwarder) 实例，不提供数据源切换、添加或编辑界面。
- 数据源基础地址在构建/发布时固化；每个客户端所需 Bearer Token 由部署或安装流程写入系统 Keyring，不提供 Token 配置 UI。
- 新闻同步独立于 EasiCoin 账户、登录和交易连接；只要应用进程运行，就持续同步，包括新闻页未打开或窗口最小化时。
- 首次成功配置后从 `cursor=0` 开始拉取 TG-forwarder 当前 API 可见的全部历史；完成后按游标增量同步。
- 所有已同步历史永久保存在本地，不按数量或日期自动清理。
- 追平后约每 3 秒轮询一次；`has_more=true` 时立即拉取下一页；瞬时错误采用 3、6、12、24、48、60 秒退避，上限 60 秒。
- 只使用 TG-forwarder 当前公开 API，不要求修改 TG-forwarder，也不增加本地来源映射。
- 页面只显示消息时间和正文，不显示来源用户名、来源 ID、媒体类型或其他来源元数据。
- 正文为空的消息仍保留一个时间线条目，正文显示“该消息暂无可展示的文本内容”。
- 页面参考金十数据网页版的时间线阅读方式，但采用单列聚焦布局，不增加右侧栏、日期侧栏或辅助信息栏。
- 首屏显示最新 50 条；使用日期分隔；底部按钮每次手动加载更早 50 条，不使用无限滚动。
- 仅识别正文中的 `http://` 和 `https://` 链接；点击后调用系统默认浏览器，不在应用内打开 WebView。
- 用户位于列表顶部时，新消息实时插入；用户正在阅读较早内容时，保持当前位置并显示“有 N 条新消息”，点击后回到顶部。
- 主导航“新闻”入口显示数字未读角标，超过 99 显示 `99+`；用户查看到最新位置后清除已看到的未读。
- Keyring 中没有 Token 时只禁用新闻模块；其他应用功能继续工作。新闻页显示“新闻服务未配置”，轮询暂停，并提供“重新检查”。
- PRD-09 不发送操作系统通知；系统通知留给 PRD-13 通知中心。
- 持久化方案采用 Rust 独立 `NewsService` 和 SQLite，不使用 JSONL、`localStorage` 或 IndexedDB 作为权威数据源。

## 2. 上游 API 边界

实现以 TG-forwarder `v0.5.0`、提交 `28425452a1948e4cb2118973c4842fea25567daf` 的[公开 API 文档](https://github.com/Akiyy-dev/TG-forwarder/blob/28425452a1948e4cb2118973c4842fea25567daf/docs/api.md)为协议基线：

- 请求：`GET /api/public/v1/messages?cursor=<非负整数>&limit=<1..100>`。
- 鉴权：`Authorization: Bearer <token>`。
- 响应：`{ ok, data: { items, next_cursor, has_more }, meta }`。
- 单条消息可包含 `delivery_id`、`created_at`、`source_backend`、`source_chat_id`、`source_message_id`、`source_chat_username`、`grouped_id`、`text`、`media_type`、`media_items`、`processed_at`。
- `delivery_id` 是递增但不保证连续的投递游标；未读数不得通过两个 ID 相减计算。
- API 只有轮询，没有 SSE 或 WebSocket；读取不会删除消息，也没有 ACK、删除或已读接口。
- API 只返回已处理并发布、且在公开端点绑定后可见的消息；它不能补回绑定前不存在于该端点的历史。
- 媒体字段只有元数据而非媒体二进制；本 PRD 不存储、不下载也不展示媒体字段。
- 当前协议没有分类、重要程度、来源展示名、原始 Telegram 发布时间或稳定原文链接；本 PRD 不推导这些数据。

Rust API 模型只接收本模块需要的 `delivery_id`、`created_at` 和 `text`，其余字段由 Serde 忽略，不进入领域模型、SQLite、Tauri 事件或前端。

协议校验至少包括：HTTP 状态、`ok=true`、必需的 `data` 字段、合法时间，以及 `next_cursor >= 请求 cursor`。每个 item 的 `delivery_id` 必须是正 `i64`、严格大于请求 cursor，同一页内不得重复，items 数不得超过本次请求 limit；非空页的 `next_cursor` 必须等于该页最大 `delivery_id`；空页必须返回原 cursor 且 `has_more=false`。当 `has_more=true` 但页面为空或游标不前进时，视为游标协议错误并停止紧密循环。实现不假定 ID 连续，也不依赖上游数组顺序；入库前按 `delivery_id` 规范化。

`created_at` 必须是带 `Z` 或明确 offset 的 RFC 3339 字符串，解析后统一为 UTC 并检查毫秒转换溢出。`text` 必须存在且类型为字符串；只有合法的空字符串或纯空白字符串可规范化为空正文，缺失、`null` 或其他 JSON 类型都属于协议错误。

客户端设置独立防御上限：单个响应体最多 8 MiB，单条 `text` 的 UTF-8 长度最多 256 KiB。即使服务未发送 `Content-Length`，也必须在流式读取 JSON 前累计检查；超限进入 `contractError`，不截断正文、不写库、不推进游标。

## 3. 目标、依赖与非目标

### 3.1 目标

- 在桌面应用中提供稳定、低干扰、适合持续阅读的实时新闻流。
- 在不暴露 Token 的前提下，由 Rust 完成鉴权、轮询、重试、去重和本地持久化。
- 首次完整同步上游可见历史，之后即使离线、Token 缺失或接口暂时失败也能阅读本地缓存。
- 对批次写入和游标推进提供原子性，确保重启后不丢消息、不重复展示。
- 明确定义首次同步、未读、前台插入、滚动位置保持和手动分页语义。
- 新闻故障只影响新闻模块，不影响行情、交易、账户或其他页面。

### 3.2 与后续 PRD 的边界

- PRD-13 可在未来订阅经过本服务提交的新闻事件并产生系统通知；PRD-09 本身不请求通知权限、不弹系统通知。
- PRD-14 全局搜索未来可以调用只读新闻查询能力；PRD-09 不实现全文搜索或搜索索引。
- PRD-15 快捷键和 PRD-16 工作区布局以后可以导航或放置新闻模块；本 PRD 不定义相关快捷键和布局保存。
- 页面使用现有主题变量和可本地化文案边界，但不在本 PRD 内实现 PRD-17 国际化框架或 PRD-18 主题系统。

### 3.3 非目标

- 修改或扩展 TG-forwarder。
- 多数据源、用户自定义地址、Token 编辑界面或数据源管理。
- 新闻分类、重要性、筛选、搜索、收藏、语音播报或自动翻译。
- 来源名称/ID、媒体类型、图片、视频、文件或媒体预览。
- 操作系统通知和通知中心历史。
- 云同步、跨设备已读状态或服务端 ACK。
- 自动清理本地历史、无限滚动或自动加载更早消息。

## 4. 总体架构与生命周期

```text
固定 TG-forwarder
  -> Rust NewsClient
  -> NewsPoller
  -> NewsRepository (SQLite)
  -> NewsService
  -> Tauri Commands / Events
  -> Pinia news store
  -> NewsCenterPage / 主导航未读角标
```

### 4.1 Rust 组件

- `api/news_client.rs`：独立 `reqwest::Client`，拼装固定公开端点、Bearer 请求、超时和响应校验。连接超时为 5 秒、单次请求总超时为 15 秒；禁用自动重定向，任何 3xx 都按部署/协议错误处理，避免 Bearer Token 被带往其他地址。不得复用持有 EasiCoin 凭据与签名状态的交易 `ApiClient`。
- `models/news.rs`：上游响应模型、领域消息、状态、分页和 Tauri DTO。跨 Tauri 边界的投递 ID 使用十进制字符串，避免 JavaScript `number` 的 53 位精度限制。
- `storage/news_database.rs`：SQLite 建库、迁移、事务、分页、游标、首次同步标记和已读水位。
- `services/news.rs`：面向 Commands 的读取/操作接口以及同步状态快照。
- `services/news/poller.rs`：应用级单实例轮询状态机、退避、取消和事件发送。
- `commands/news.rs`：参数校验后的薄命令层，不包含轮询或 SQL 业务逻辑。

SQLite 使用 `rusqlite` 的 bundled SQLite 构建，以免依赖用户机器上的 SQLite 动态库。数据库工作在 `spawn_blocking` 中执行，并由仓储层串行化同一连接上的操作；异步运行时线程不得直接执行阻塞 SQL。

### 4.2 前端组件

- `stores/news.ts`：状态、最新 50 条、已加载旧页、待查看新消息数、未读角标和命令调用协调。该 store 由 `AppShell` 初始化，而不是等新闻页首次打开，保证主导航角标始终更新。
- `components/news/NewsCenterPage.vue`：新闻页面容器、加载态/故障态、滚动容器和首屏协调。
- `components/news/NewsTimeline.vue`：日期分隔、时间列、正文和手动加载更多。
- `components/news/NewsStatusBar.vue`：紧凑显示“正在同步 / 实时 / 重试中 / 新闻服务未配置”等状态及允许的恢复操作。
- `components/news/NewMessagesBanner.vue`：阅读较早内容时显示待查看新消息数。
- `utils/newsLinks.ts`：把纯文本安全切分为普通文字与 `http(s)` 链接，不生成或注入 HTML。

### 4.3 应用生命周期

1. Tauri `setup` 阶段尝试打开并迁移新闻数据库，创建唯一 `NewsService`；失败只写入新闻的 `storageError` 状态，不能让 Tauri setup 失败或阻止主应用启动。
2. 服务从数据库恢复游标、首次同步完成标记和已读水位。
3. 服务校验构建期固定地址并读取 Keyring Token；条件满足时立即启动唯一 poller，不等待 Vue 挂载新闻页。
4. `AppShell` 初始化 news store 时先注册两个事件监听，再读取包含未读数和最新 ID 的状态快照；事件与快照按最新 ID 对账，以覆盖 Rust 早于 Vue 启动或初始化期间提交消息的竞态。`AppShell` 把角标值传给始终挂载的 `NavigationRail`，新闻消息页只在进入新闻页面时读取。
5. 窗口最小化、切换页面或新闻组件卸载不停止 poller。
6. `NewsService` 保存 poller 的 `JoinHandle`。现有 `RunEvent::Exit` 调用有界的 `news.stop_and_join()`：取消等待/网络请求，不再开启新请求，并等待正在执行的 SQLite 事务提交或回滚；最长等待 20 秒。超时退出仍只能丢弃未提交页，SQLite 原子事务和旧游标必须保持一致。

## 5. 固定地址与 Token 部署契约

### 5.1 固定基础地址

- 发布流程通过构建环境变量 `EASIFLUX_NEWS_API_BASE_URL` 注入唯一 TG-forwarder 基础地址，并通过非秘密的 `EASIFLUX_NEWS_SOURCE_EPOCH` 标识该上游 delivery 命名空间版本；Rust 用 `option_env!` 把两者固化到产物中。
- 正式 release 打包把任一变量缺失、epoch 为空或地址不合法视为打包错误；普通 debug/test 构建可以省略，并在运行时进入无恢复按钮的 `deploymentMisconfigured`，以便无真实服务和 Token 时继续本地开发及测试。
- 地址不是运行时配置，不写入 `config.toml`、SQLite、Pinia 或浏览器存储，也不提供修改命令。
- 启动时使用 `url::Url` 解析和规范化基础地址，再安全拼接 `api/public/v1/messages`，不使用字符串直接拼接用户输入。
- release 构建只接受 `https`。debug 构建仅为本地测试额外允许 host 为 `localhost`、`127.0.0.1` 或 `[::1]` 的 `http` 地址。
- 地址或 epoch 缺失、校验失败时，新闻显示“新闻数据源未内置，请使用正确的安装包”，不启动 poller，也不显示“重新检查”；其他模块不受影响。

### 5.2 Keyring Token

- 新闻使用独立 Keyring service `easiflux_desktop_tauri_news`，entry/user name 固定为 `news_api_token`。不得沿用交易凭据的 `easiflux_desktop_tauri` service，因为交易账户 ID 没有保留字限制，同名账户会产生键碰撞。
- 部署或安装流程把原始 Bearer Token 写入该项；应用只读取，不提供写入、删除或回显 Token 的前端命令。
- 首次安装和后续 Token 轮换必须绑定同一个 TG-forwarder 公开 API endpoint 及同一组来源；换到另一 endpoint 或重建上游 delivery 数据库会改变 `delivery_id` 游标空间，属于需要显式数据迁移的新数据源变更，不得伪装成普通 Token 轮换。TG-forwarder 的公开响应不返回 endpoint 身份，因此部署方必须在管理端核验绑定；客户端无法仅凭 Token 自证这一点。
- Token 不写入日志、错误详情、SQLite、配置文件、Tauri 事件、Pinia 或 DOM。
- “重新检查”只重新读取固定 Keyring 项并恢复 poller，不打开编辑器，也不接受前端传入 Token。
- Keyring 后端错误与 Token 缺失都只停用新闻，但状态文案区分“未配置”和“凭据存储暂不可用”；底层错误详情保持在脱敏诊断中。

### 5.3 安装与轮换

PRD-09 的交付范围包含一个最小 Rust provisioning utility，与桌面应用使用相同版本的 `keyring` crate 和上述固定 service/entry：

- utility 由安装器或受控部署流程在目标桌面用户的登录会话中运行，只负责写入新闻 Token；它不是应用运行时 sidecar，也不由 Vue 调用。
- Token 只通过标准输入或平台等价的匿名安全管道传入，禁止放入命令行参数、环境变量、响应文件、临时文件或安装日志。utility 不回显输入，内存缓冲在完成后清零。
- 首次安装和轮换使用同一写入动作；再次执行覆盖固定 entry。utility 不会在写入前主动删除旧值；写入失败返回非零状态且不启动或破坏桌面应用，但若平台 Keyring 后端不保证原子覆盖，部署流程必须准备原 Token 重新写入，不能声称失败后旧值必然仍可用。
- Windows、macOS 和 Linux 构建均调用 `keyring::Entry::new("easiflux_desktop_tauri_news", "news_api_token")`，由 `keyring` 后端写入当前用户的系统凭据存储；平台凭据存储不可用时部署明确失败，应用保持“凭据存储暂不可用”。
- 部署完成只通过 utility 退出状态和应用的脱敏 `get_news_status` 验证，不读取、打印或比较明文 Token。
- 轮换前由部署方保证新 Token 仍绑定同一公开 endpoint；写入后用户或部署自动化触发“重新检查”。卸载默认不删除系统凭据，避免无意销毁外部管理的秘密；明确的安全回收流程可调用 utility 的删除模式，并需单独确认目标 service/entry。

安装器/部署维护者负责从其秘密管理系统把每客户端 Token 送入该 utility；应用仓库负责 utility、跨平台构建接入、无泄漏测试和部署文档。没有完成这一步的产物可以运行其他模块，但新闻按设计保持未配置。

## 6. SQLite 数据模型与一致性

数据库位于 Tauri 应用本地数据目录下的 `news/news.sqlite3`。首次打开创建父目录和数据库；使用 `PRAGMA user_version` 管理前向迁移。建议启用 WAL、`foreign_keys=ON`、合理的 `busy_timeout` 和 `synchronous=NORMAL`。

### 6.1 Schema v1

```sql
CREATE TABLE news_messages (
  delivery_id   INTEGER PRIMARY KEY CHECK (delivery_id > 0),
  created_at_ms INTEGER NOT NULL,
  text          TEXT NOT NULL,
  received_at_ms INTEGER NOT NULL
);

CREATE INDEX idx_news_messages_created_at
  ON news_messages(created_at_ms DESC, delivery_id DESC);

CREATE TABLE news_sync_state (
  singleton_id          INTEGER PRIMARY KEY CHECK (singleton_id = 1),
  cursor                INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
  last_seen_delivery_id INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_delivery_id >= 0),
  initial_sync_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_sync_complete IN (0, 1)),
  source_fingerprint    TEXT NOT NULL DEFAULT ''
);

INSERT INTO news_sync_state(singleton_id) VALUES (1);
```

- `delivery_id` 是唯一键；重复拉取用 `INSERT ... ON CONFLICT DO NOTHING` 去重。
- `created_at_ms` 来自 API 的 `created_at`，用于显示和日期分隔；它表示公开投递记录时间，不冒充 Telegram 原始发布时间。
- `received_at_ms` 只用于诊断本地同步，不展示给用户。
- `text` 保存上游合法字符串正文；空字符串和纯空白规范化为空字符串，展示时由前端使用固定占位文案。`null`、缺失或非字符串在入库前已作为协议错误拒绝。
- 来源、聊天 ID、用户名、媒体和 Token 不进入数据库。
- `source_fingerprint` 是 `SHA-256(规范化公开 endpoint URL + NUL + SOURCE_EPOCH)`，不包含 Token，也不用于 UI。第一次成功配置时写入；后续构建若修改地址或 epoch，服务进入部署/协议错误而不是复用旧游标或自动清库。
- 部署方在更换 endpoint 或重建上游数据库时必须递增 `EASIFLUX_NEWS_SOURCE_EPOCH`。PRD-09 不自动重置或合并不同命名空间；迁移必须另行批准，并先把旧数据库完整归档，再创建新数据库从 cursor 0 同步。
- 更早分页以 `(delivery_id < before_delivery_id) ORDER BY delivery_id DESC LIMIT (请求数量 + 1)` 查询；只返回前 `limit` 条，第 `limit + 1` 条是否存在决定 `hasMore`。展示主顺序与上游游标一致，日期分隔由 `created_at_ms` 计算。

### 6.2 批次事务

每个成功 API 页面在一个 SQLite 事务中完成：

1. 校验并规范化全部消息。
2. 按 `delivery_id` 升序尝试插入，记录实际新增条数。
3. 把 `cursor` 更新为响应的 `next_cursor`，即使该页全部是已存在记录也要推进合法游标。
4. 如果这是首次同步中第一个 `has_more=false` 的页面，同时设置 `initial_sync_complete=1`，并把 `last_seen_delivery_id` 初始化为 `COALESCE(MAX(delivery_id), 0)`；首次响应就是空页时也能原子完成基线。
5. 提交事务后才能更新内存状态和发出 Tauri 事件。

任何解析、校验或 SQL 失败都回滚整页，游标不得推进。应用崩溃后最多重拉最后一个未提交页面，唯一键保证不会重复展示。

### 6.3 首次历史与未读定义

- 首次同步未完成时，页面显示累计已同步数量，但不逐页展示从最旧开始的中间结果；追平后一次性读取本地最新 50 条。
- 首次追平的同一事务把 `last_seen_delivery_id` 初始化为当时数据库最大 `delivery_id`。首次导入的历史是可浏览基线，不制造成百上千条未读角标。
- `initial_sync_complete=false` 时，对外状态和 committed 事件的 `unreadCount` 固定为 0；历史批次不会让主导航角标短暂跳高。
- 以后未读数通过 `COUNT(*) WHERE delivery_id > last_seen_delivery_id` 计算，不能使用 ID 差值。
- 跨边界返回的角标计数在 Rust 中封顶为 100：`0..99` 原样显示，`100` 表示 UI 显示 `99+`。
- `mark_news_seen(through_delivery_id)` 只把水位单调推进到 `min(through_delivery_id, 当前数据库最大 ID)`；前端传入它已经渲染并位于顶部看到的最新 ID。这样并发到达但尚未渲染的消息不会被误标已读。
- 数据库已有 `initial_sync_complete=1` 时，应用重启立即显示缓存，不重置已读水位；后台继续从保存的游标同步。

## 7. 轮询状态机

### 7.1 状态

```text
deploymentMisconfigured 固定地址缺失或非法；release 打包应在更早阶段阻止该状态
notConfigured     Token 缺失，poller 暂停
credentialStoreUnavailable
initialSync       首次全量同步中，带数据库内累计已同步总数
live              已追平，等待下一次约 3 秒轮询
retrying          瞬时失败退避中，带下一次重试时间和脱敏原因
credentialInvalid 收到 401，停止自动请求，等待重新检查
contractError     422、其他永久 4xx 或响应/游标契约错误，停止紧密重试
storageError      SQLite 打开、迁移或事务失败，停止同步但不删除文件
stopped           应用正在退出
```

状态错误只提供用户可行动的分类和简短文案，不包含请求头、Token、原始响应体、数据库绝对路径或 Keyring 后端详情。

### 7.2 正常循环

1. 从 SQLite 读取已提交 `cursor`；全新数据库从 0 开始。
2. 以 `limit=100` 请求公开消息端点。
3. 原子提交消息和 `next_cursor`。
4. `has_more=true` 时立即让出一次运行时调度后请求下一页，不等待 3 秒。
5. `has_more=false` 时进入 `live`，等待约 3 秒再请求当前已提交游标。
6. 任一成功响应把瞬时错误退避重置为 3 秒。

同一应用进程只允许一个 poller。手动恢复命令若发现任务已运行，只返回当前状态，不创建第二条轮询链。

对 429 或 503 的合法 `Retry-After`，等待时间取“当前指数退避”和该值中的较大者，再按已确认规则封顶 60 秒；无效或缺失的 header 使用指数退避。固定数据源的生产反向代理必须在部署验收中确认允许约 3 秒的稳态轮询，或能稳定返回 429/503 供上述退避处理。

### 7.3 失败分类与恢复

| 条件 | 行为 | 自动恢复 |
|---|---|---|
| 固定地址缺失或非法 | 保留缓存，部署配置错误，停止请求 | 否；重新打包/部署，无运行时按钮 |
| 无 Token | 保留缓存，状态为未配置，停止请求 | 否；部署后“重新检查” |
| Keyring 读取失败 | 保留缓存，凭据存储不可用 | 否；“重新检查” |
| 401 | 凭据无效，停止请求 | 否；重新部署 Token 后“重新检查” |
| 网络错误、超时、408、429、5xx | 保留缓存，按 3/6/12/24/48/60 秒重试 | 是 |
| 422、其他永久 4xx | 视为部署或协议错误，停止高频请求 | 否；“重试同步” |
| JSON/字段/时间/游标不合法 | 回滚本页，状态为协议错误 | 否；“重试同步” |
| SQLite 打开/迁移/事务失败 | 不推进游标，不自动删除或重建数据库 | 否；“重试同步”会重新打开并迁移数据库后再恢复 poller |
| Tauri 事件发送失败 | 已提交数据不回滚 | 是；页面下次从 SQLite 重载 |

“重新检查”只重新读取固定 Keyring Token，并在条件满足时以已提交游标恢复；编译进产物的地址和 epoch 不可能由该按钮改变。“重试同步”用于存储或协议环境已被外部修复后的单次恢复；它仍保持 single-flight。两者都不清库、不重置游标。

故障状态下只有 `initial_sync_complete=true` 的完整缓存可以作为新闻列表展示。首次同步中途失败时保留已提交批次和累计数量用于续传，但继续显示同步中断状态，不把从历史最旧端开始的部分数据库伪装成完整最新列表。

## 8. Tauri Commands、Events 与 DTO

所有 Rust DTO 使用 `serde(rename_all = "camelCase")`，TypeScript 定义与其严格对齐。

### 8.1 Commands

```text
get_news_status() -> NewsStatusSnapshot

list_news_messages(before_delivery_id?: string, limit?: number)
  -> NewsPage

mark_news_seen(through_delivery_id: string)
  -> NewsUnreadSnapshot

recheck_news_credentials()
  -> NewsStatusSnapshot

retry_news_sync()
  -> NewsStatusSnapshot
```

- `list_news_messages` 的 `limit` 默认并最大为 50；只接受 1..50。
- 不传 `before_delivery_id` 返回最新一页；传入时返回严格更早的一页。
- 十进制 ID 必须在 Rust 中严格解析为正 `i64`，拒绝负数、零、空白、前后垃圾字符和溢出。
- 读取命令只查询 SQLite，不直接触发上游网络请求。
- 恢复命令只向应用级服务发送控制信号，不在命令任务内运行无限轮询。

建议 DTO：

```text
NewsMessageDto {
  deliveryId: string
  createdAt: string       // RFC 3339 UTC；前端按本地时区格式化
  text: string
}

NewsPage {
  items: NewsMessageDto[] // deliveryId 降序
  hasMore: boolean
  latestDeliveryId?: string
  unreadCount: number     // 0..100，100 表示 99+
}

NewsStatusSnapshot {
  kind: NewsStatusKind
  initialSyncComplete: boolean
  syncedCount: number      // 首次同步未完成时数据库内已同步的消息总数
  latestDeliveryId?: string
  unreadCount: number      // 0..100，100 表示 99+
  retryAt?: string
  message?: string        // 仅脱敏、可展示文案
}
```

### 8.2 Events

```text
news://messages-committed
  { insertedCount, newestDeliveryId?, unreadCount, initialSyncComplete }

news://status-changed
  NewsStatusSnapshot
```

- `messages-committed` 仅在 SQLite 提交后发送；`insertedCount` 是唯一键去重后的实际新增数。
- 首次同步中仍可发送进度事件，但页面在 `initialSyncComplete=false` 时不展示中间新闻页。
- 事件不携带 Token、来源字段、媒体字段或原始错误响应。
- 事件不是权威存储。监听失败、页面未挂载或应用前端短暂卡顿后，Pinia 必须通过 Commands 从 SQLite 重建状态。

## 9. 页面与交互设计

### 9.1 页面结构

`AppShell` 在 `activePage === 'news'` 时挂载独立 `NewsCenterPage`：

- 保留现有 `TopBar` 和左侧一级主导航。
- 新闻页不显示交易二级 `Sidebar`，不给筛选、账户或行情面板预留空栏。
- 主内容是居中的固定阅读宽度单列；在宽屏上保留舒适留白，在窄屏上收缩到可用宽度。
- 页面本身不滚动；新闻列表使用独立纵向滚动容器，便于稳定保存视口和显示悬浮新消息横幅。
- 顶部只有紧凑状态条，不增加页面级搜索、来源选择、类别标签或媒体开关。

### 9.2 视觉语言与响应式尺寸

本页服务于持续盯盘的交易用户，唯一任务是快速扫描最新消息，并在阅读旧消息后无损回到实时位置。视觉设计以“秒级市场脉冲轨”为识别点，而不是把每条新闻做成通用卡片：

- 颜色只使用现有 `--ef-color-background`、`surface`、`border`、`text`、`text-secondary`、`primary`、`warning` 和 `danger` 语义 Token，不新增硬编码品牌色；正常新闻不按内容猜测涨跌或重要性。
- 正文使用现有 `--ef-font-sans`，时间使用 `--ef-font-mono` 并启用等宽数字，日期和状态使用现有 label 字重。正文建议 `--ef-text-base`、`line-height: 1.65`，不引入外部字体。
- 阅读列 `max-width: 880px`，桌面水平内边距使用 `--ef-space-5`；时间列宽约 72 px，时间与正文间距使用 `--ef-space-4`。条目采用水平细分隔和 14–16 px 的垂直节奏，不用一条一张带阴影卡片。
- 时间列形成一条克制的“脉冲轨”：最新端点使用 `primary`，历史节点和连线使用 `border/text-secondary`。它编码消息先后关系，不承担装饰或虚构重要性。
- 新消息在顶部插入时只执行一次不超过 200 ms 的背景淡出，不使用位移；`prefers-reduced-motion: reduce` 时完全取消动画，避免盯盘干扰和视口抖动。
- 可交互链接、加载按钮、恢复按钮和新消息横幅都有可见键盘焦点环。横幅使用 `aria-live="polite"` 报告累计新消息数，但首次历史同步不逐条播报。
- 可用宽度小于 640 px 时，时间移到正文上方，列内边距降为 `--ef-space-3`；正文、日期、状态和全部恢复操作仍保留，不通过横向滚动隐藏信息。

### 9.3 时间线条目

- 列表按 `delivery_id` 从新到旧展示。
- 每个条目左侧显示本地时区的 `HH:mm:ss`，右侧显示正文；视觉上使用细时间轴和克制的分隔，参考金十数据的信息密度而非复制其完整双栏页面。
- 日期变化处插入本地时区的日期分隔，例如“2026年7月30日”。日期分隔是前端派生视图，不写回数据库。
- 正文为空或只有空白时显示“该消息暂无可展示的文本内容”。不显示“媒体消息”、媒体类型、来源用户名或 ID。
- 正文作为纯文本节点渲染，保留合理换行；禁止 `v-html`。
- URL 解析器只生成 `http`/`https` 链接片段，并剔除句末常见中文/英文标点。点击时再次用 `URL` 校验 scheme，再通过现有 Tauri opener 插件交给系统默认浏览器。

### 9.4 初次同步与缓存优先

- `initial_sync_complete=false` 时，主区域显示“正在同步历史新闻”及数据库内累计已同步总数；中断并重启后该数字从持久化行数恢复，不显示可能从最旧端开始的半成品列表。
- 首次追平后加载最新 50 条并进入实时状态。
- 已有完整缓存的后续启动先立即展示最新 50 条，再让后台 poller 校准；网络、Token 或上游状态不会阻塞缓存阅读。
- 若从未成功同步且 Token 缺失，显示“新闻服务未配置”和“重新检查”；只有 `initial_sync_complete=true` 的完整缓存才继续显示在故障状态条下方。

### 9.5 实时新消息与滚动位置

前端把“最新位置”定义为滚动容器距顶部不超过一个小阈值（建议 24 px）：

- 位于最新位置：收到提交事件后读取最新页，与已加载列表按 `deliveryId` 去重合并并插入顶部；下一帧仍保持顶部。渲染完成后以当前可见最新 ID 调用 `mark_news_seen`。
- 正在阅读较早内容：不修改当前列表和 `scrollTop`，只累计实际新增数并显示悬浮“有 N 条新消息”。因此视口不会因 DOM 高度变化跳动。
- 点击横幅：重新读取最新 50 条、清除当前旧页视图、滚动到顶部；渲染后标记已看到的最新 ID。
- news store 合并同一时段的 committed 事件，同一时刻只执行一次最新页刷新；刷新期间再到达事件时设置一次后续刷新标记。所有结果按 `deliveryId` 去重，并用 generation 防止较早请求覆盖较新列表；每个 command await 和 `nextTick` 后都重新确认页面仍在最新位置、当前 generation 仍有效，再决定插入、滚动或标记已读。
- 页面重新获得可见性、重新挂载或事件监听恢复时，重新读取状态与最新页，以 SQLite 为准纠正横幅和未读数。

### 9.6 手动加载更早消息

- 首屏和点击新消息横幅后均以 50 条为一页。
- 底部“加载更多”把当前最旧 `deliveryId` 作为 `before_delivery_id`，请求严格更早 50 条并追加。
- 请求期间按钮显示加载态且不可重复触发；失败时保留现有列表并允许重试。
- `hasMore=false` 时替换为“已显示全部新闻”，不继续发命令。
- 加载更早消息不会改变未读水位，也不会触发上游请求。

### 9.7 主导航未读角标

- 主导航角标来自 Rust 持久化水位，而不是组件内临时计数；应用重启后保持一致。
- `0` 不显示，`1..99` 显示数字，封顶值 `100` 显示 `99+`。
- 仅仅打开新闻页面但仍停留在较早位置不会清空角标。
- 页面已在顶部并显示新消息，或用户点击横幅回到顶部后，才推进到实际看到的最新 ID。

## 10. 安全、隐私与隔离

- 任何包含 Token 的类型不得派生会输出原值的 `Debug`；日志只能记录请求分类、状态码、退避阶段和脱敏错误类别。
- `reqwest` 错误、响应体、Keyring 错误和 URL 不直接透传到 Vue；统一映射为新闻状态类别。
- 禁止在前端发起 TG-forwarder 请求，避免 CORS 差异和凭据暴露。
- SQLite 查询全部使用参数绑定；前端 ID 在 Rust 严格解析，不参与 SQL 字符串拼接。
- 正文永远按纯文本处理；链接片段只允许 `http`/`https`，其余 scheme（包括 `javascript:`、`data:`、`file:`）保持普通文本且不可点击。
- 外链始终交给系统默认浏览器；新闻正文不能创建应用内 WebView、执行脚本或加载远程媒体。
- 新闻服务拥有独立客户端、状态机、数据库和取消句柄，不读取或修改当前交易账户、EasiCoin API Key、行情订阅或连接状态。
- SQLite 损坏或迁移失败时不得自动删除、覆盖或静默重建原文件；仅禁用新闻同步并保留诊断和人工恢复空间。

## 11. 测试设计

### 11.1 Rust 单元与集成测试

`NewsClient`：

- 发送正确的 Bearer 头、`cursor` 和 `limit=100`，且错误和日志中不出现 Token。
- 解析合法空页、多条页、空字符串/纯空白正文和未知额外字段。
- 接受带 `Z` 或合法 offset 的 RFC 3339 时间并统一到 UTC；拒绝无时区、非法时间和毫秒转换溢出。
- 拒绝 `ok=false`、2xx 非 JSON、缺失/`null`/非字符串正文、倒退游标以及 `has_more=true` 但游标不前进。
- 覆盖空页游标、`next_cursor` 与最大 item ID 不一致、页内重复 ID、item ID 不大于请求 cursor、items 超过请求 limit。
- JSON ID 的 0、负数、小数、字符串和 `i64` 溢出均失败；大于 JavaScript 安全整数但仍在 `i64` 范围内的 ID 成功并以字符串跨 Tauri 边界。
- 拒绝超过 8 MiB 的响应和超过 256 KiB 的正文；正文不被静默截断。
- 3xx 不跟随，测试 Bearer 不会到达第二 origin。
- 401、408、422、429、5xx 和网络超时映射到正确错误类别。

`NewsRepository`：

- 全新数据库创建 v1 schema，游标从 0 开始。
- 单事务插入消息并推进游标；注入失败后消息与游标一起回滚。
- 重复 `delivery_id` 不重复展示，但合法 `next_cursor` 仍可推进。
- 非连续 ID 的未读数通过行数计算，不通过 ID 差值。
- 最新 50、`before_delivery_id` 更早分页、日期跨界数据和永久历史查询正确；49/50/51 条边界返回准确 `hasMore`。
- 首次追平把历史建立为已读基线；首次空历史使用水位 0；后续消息增加未读，`mark_news_seen` 单调推进且不越过当前最大 ID。
- 重启恢复游标、首次同步标记和已读水位。
- 固定 endpoint 指纹一致时正常恢复；指纹变化时拒绝混用旧游标和历史。
- 迁移失败或损坏库不触发自动删除。

`NewsPoller`：

- 首次从 0 多页连续追平；`has_more=true` 不等待 3 秒，追平后等待约 3 秒。
- 崩溃/取消后从最后已提交游标恢复，不丢失也不重复展示。
- 瞬时错误按 3、6、12、24、48、60 秒退避，成功后重置。
- 429/503 的合法、无效和超大 `Retry-After` 按规则合并并封顶 60 秒。
- 401 暂停直至重新检查；422/契约错误暂停直至手动重试。
- 缺少 Token 时不发网络请求；重新检查后只启动一个 poller。
- SQLite 提交失败不发 committed 事件、不推进游标。
- Tauri 事件发送失败不回滚已提交数据。
- `RunEvent::Exit` 调用 `stop_and_join`；收到退出信号后不再开启新请求，当前事务完整结束，超时路径仍保持最后已提交游标一致。
- 新闻同步在没有交易凭据、交易断线、页面未挂载和窗口最小化时仍按自身状态运行。

`Provisioning utility`：

- 只写入 `easiflux_desktop_tauri_news/news_api_token`，与名为 `news_api_token` 的交易账户凭据互不覆盖。
- 只从 stdin/安全管道读取，空 Token 失败；stdout、stderr、日志、进程参数和测试快照中都没有明文 Token。
- 首次写入、轮换、平台失败和明确删除模式返回正确状态；使用可替换 Keyring adapter，不触碰开发者真实凭据。

### 11.2 前端测试

- 点击主导航“新闻”渲染独立页面，保留 TopBar 和一级导航，不渲染交易 Sidebar。
- 时间线按最新优先排序，按本地日期插入分隔，时间显示到秒。
- 空正文显示固定占位，不出现来源用户名/ID和媒体类型。
- 仅 `http`/`https` 被识别；点击调用系统浏览器，危险 scheme 不可点击，正文不使用 `v-html`。
- 首次全量同步显示持久化累计进度，追平前不展示中间旧页；只有已完成首次同步的缓存才先行展示。
- 位于顶部时合并新消息并标记已看到；位于较早位置时列表和 `scrollTop` 不变、横幅计数增加。
- deferred command 交错完成、刷新期间离开顶部、连续 committed 事件和旧 generation 返回时，列表、滚动位置和已读水位都不被旧结果覆盖。
- 点击横幅回到最新 50 条并在渲染后清除对应未读。
- 每次“加载更多”只追加更早 50 条，防止重复请求；末页显示完成文案；失败可重试。
- 主导航角标覆盖 0、1、99、100（`99+`）以及重启恢复；只打开页面但未到顶部不清除。
- 新闻页从未打开时，AppShell 仍初始化监听、恢复未读并把角标传给 NavigationRail；初始化竞态通过状态快照对账。
- 缺 Token、Keyring 不可用、离线/退避、401、协议错误和存储错误都保留可用缓存并显示正确恢复按钮。

### 11.3 回归与验证命令

- `pnpm lint`
- `pnpm test`
- `pnpm build`
- `cargo fmt --check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
- `git diff --check`

测试应使用本地 mock HTTP 服务、临时 SQLite 数据库和可替换的 Token repository；不得依赖真实 TG-forwarder、真实 Token 或用户 Keyring。

## 12. 验收标准

- `news/createnewscenter` 开始实施时基于已重新抓取确认的最新 `origin/main`。
- release 打包必须固化唯一 TG-forwarder 地址和非空 source epoch；页面没有数据源或 Token 编辑入口。
- installer/deployment 通过无回显 provisioning utility 把 Token 写入独立的 `easiflux_desktop_tauri_news/news_api_token`，不会碰撞或覆盖任何交易账户凭据。
- 正确部署 Token 后，全新客户端从 `cursor=0` 拉取全部 API 可见历史，追平后显示最新 50 条。
- 首次导入历史不形成巨大未读角标；完成后到达的新消息产生持久化未读数。
- 重启后从最后已提交游标继续；重复响应、重试和中途退出不会造成丢失或重复展示。
- 追平时约每 3 秒轮询；有更多页时连续拉取；瞬时失败退避且已完成首次同步的缓存始终可读。
- endpoint URL/source epoch 指纹变化时拒绝沿用旧游标；不同 delivery 命名空间不会被静默混入同一数据库。
- 新闻页只显示到秒的时间和正文；不显示来源用户名/ID、媒体类型或媒体预览。
- 空正文消息保留并显示固定占位。
- 用户在顶部时新消息实时出现；阅读较早内容时视口不跳动，并可通过“有 N 条新消息”返回顶部。
- 首屏和每次手动加载均为 50 条；没有无限滚动，全部加载后有明确结束状态。
- 主导航未读角标持久、封顶显示 `99+`，仅在用户实际看到最新位置后推进已读水位。
- 缺 Token、401、网络故障、协议异常或 SQLite 故障只影响新闻；行情、交易、账户和其他页面继续工作。
- 正常退出会有界停止并等待新闻任务；未提交批次不会留下已推进游标。
- Token 不出现在 Vue、DOM、日志、错误、数据库、配置文件或 Tauri 事件中。
- 仅 `http`/`https` 链接可点击，并由系统默认浏览器打开。
- 前端 lint/test/build、Rust fmt/test/clippy 和 `git diff --check` 全部通过。

## 13. 文档与提交边界

本文件只固化已确认设计，不代表已经授权实现、提交或推送。用户复核本规格后，下一步才使用 writing-plans 产出逐步实施计划。未经用户明确要求，不创建 Git commit。
