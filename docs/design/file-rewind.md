# 文件检查点与回退

Scope: Session（WorkspaceTools 的 Session 部件）。
Rules: [设计规则](../architecture/rust-agent/principles.md)、[ADR 002](../adr/002-plugin-scope.md)、
[插件契约](plugin-contract.md)、[Dim 如何使用 rutis](rutis-usage.md)。
Related: [CLI 功能清单](feature-list.md) 的 File rollback，
[WorkspaceTools](../architecture/rust-agent/plugins/WorkspaceTools.md)，
[Session files](appserver/session-files.md)，[ShellInput](plugins/ShellInput.md)（同形的“用户发起、写回分支输入”）。

## 1. 目标与边界

Agent 用 `write` 或 `edit` 改动工作区文件时，WorkspaceTools 在发布替换之前先保存被替换的原内容
（前像）。用户按轮次（一个 `turn_id`）查看这一轮 `write`/`edit` 改了哪些文件，并把它们一键还原到
这一轮开始之前的内容。还原只在文件仍是这一轮留下的内容时进行，不覆盖之后由用户、命令或后续轮次
做的修改；还原写成分支上的一条输入，作为持久记录，并让模型在下一次读取上下文时得知。

不在范围内：

- **命令造成的改动。** `exec` 等命令改了什么无从得知，不保存、不还原。列表给出这一轮运行过的
  命令，客户端据此明示“命令造成的改动不会撤销”（§5.3）。其他工具同理：只有 `write`、`edit`
  的改动可还原。
- **对话回退**（把对话截回某条消息）与“文件和对话一起回退”。对话的切点、压缩与 fork 是另外的设计。
- **跨轮次一次回退**、**撤销本次还原**（redo）、**强制覆盖**。连续撤销多轮时从最近一轮往前逐轮撤销。
- **其他分支与成员的改动。** 分支（线）与协作成员在各自的 Timeline 上按轮列出；一轮的列表只含
  该分支自己的 `write`/`edit`。
- **目录。** 还原不删除 `write` 为新文件创建的父目录。
- **TUI 入口。** 本文只交付方法与 Web 工作台入口；TUI 复用同一组方法，属于后续工作。

## 2. 状态归谁所有

| 状态 | 所有者 | 存放 | 生命周期 |
| --- | --- | --- | --- |
| 前像字节 | SessionStore | `sessionFiles.put` 写成只读的 Session 文件 | 与 Session 文件相同：fork 共享，Session 删除后由清扫移除 |
| 一次改动的记录：哪一轮、哪个文件、前像引用、改动后的版本 | 分支 Timeline | `step/tool-result` 上 `write`/`edit` 原样记录的结果：`fileWritten` 效果声明，`structuredContent` 的 `before` 与 `revision` | 与历史相同 |
| 一次还原的记录 | 分支 Timeline | 还原写入的 Append 输入（`dim-file-rewind` 标记，§5.4） | 与历史相同 |
| 文件当前内容 | 工作区（外部状态） | 磁盘 | 每次列出、还原时现读 |
| 路径锁 | WorkspaceTools 插件已有的写入协调器 | 内存 | 进程内，跨 Session 与各代共享 |

WorkspaceTools 不新增内存状态、缓存或索引。前像由写入方在自己的调用里保存，因为只有持有路径锁、
即将发布替换的 `write`/`edit` 知道被替换的确切字节；还原方法也放在 WorkspaceTools，因为它必须进入
同一个路径锁域，才能与正在执行的 `write`/`edit` 串行。

## 3. 能否从 Timeline 重建

能。一轮的改动列表是以下输入的纯函数，没有别的来源：

1. 该分支上 `turn_id` 等于这一轮的 `step/tool-call`（调用 id 到工具名）与 `step/tool-result`
   （结果、效果声明、`before`、`revision`），以及这一轮的 `turn/start` 与 `turn/end`（是否存在、是否结束）；
2. 工作区里这些文件的当前内容；
3. 前像 Session 文件的内容。

读取经分支门面的事件查询（[ADR 019](../adr/019-event-query.md)），重载、重启、fork 后得到同样的
结果；fork 的历史引用的前像由 fork 共享的所有权行覆盖（[Session files §4](appserver/session-files.md#4-sharing-on-fork)）。
还原记录同样在 Timeline 上；列表不读还原记录，每个文件的状态由当前内容直接判定（§5.1），
所以还原过一次、或用户手工改回原样，列表都如实显示。

一个边界：进程在 `write` 发布替换之后、`step/tool-result` 记录之前崩溃，重载按
[ToolExecutor](../architecture/rust-agent/plugins/ToolExecution.md) 把这批调用结算为错误，
该次改动没有记录，也就不在列表里。这与效果声明的契约一致：声明只随工具返回的结果记录。

## 4. 改动记录（`write`/`edit` 结果）

### 4.1 字段

`write` 与 `edit` 成功的结果已在 `structuredContent.revision` 给出改动后字节的 SHA-256（十六进制）。
本设计增加 `before`，说明被替换的内容：

| `before` | 含义 |
| --- | --- |
| `{"kind": "absent"}` | 调用前文件不存在，调用创建了它 |
| `{"kind": "kept", "path": "<Session 文件绝对路径>", "revision": "<被替换字节的 SHA-256>"}` | 被替换的字节保存在 `path` |
| 缺省 | 没有保存：宿主没有 Session 存储，文件超过 Session 文件的 16 MiB 上限，或结果早于本功能 |

父目录同步失败、结果警告“可能已部分写入”时（`mayHaveModified`），替换已经发布，效果照常声明，
结果同样带 `before` 与 `revision`。

`before` 只由 WorkspaceTools 自己的列表与还原方法读取，是私有内容（[设计规则 §3](../architecture/rust-agent/principles.md)）。
其他工具即使在 `structuredContent` 里放同名字段也不被采信：列表只读工具名为 `write`、`edit` 的调用。

**格式决定：持久化格式不变。** `structuredContent` 是开放的 JSON 对象，旧版本读新结果、新版本读旧结果
都不受影响，旧结果没有 `before`，按“没有保存”处理。`ToolEffect` 的封闭词汇、native store 版本与协议
版本都不改变（对照 [session boundary A.6](contracts/session-boundary.md#a6-tool-effects) 为 `effects` 字段升到 v21 的决定）。

### 4.2 捕获流程

```
write/edit            WorkspaceTools                       SessionStore        工作区
  | ----------------> lock(path)  (与 write/edit 同一 FIFO 域)
  |                   读取绑定
  |                   [阻塞线程] stat、打开原文件、核对版本 = 读取绑定 ------------------> 读
  |                       被替换字节 = 刚读到、哈希等于读取绑定的字节；算出新字节
  |                   [异步] put(sessionId, 名称, 被替换字节) -->| 写入、同步、提交
  |                   [阻塞线程] write_target：发布前再核对版本，暂存、保留元数据、rename --> 写
  | <---------------- 结果：revision、before、fileWritten
```

- 新建文件不需要保存：`before` 为 `absent`，`write_target` 用 `hard_link` 发布，文件已出现则失败。
- 已存在的文件：被替换的字节是锁内读到、哈希与读取绑定一致的那份；`write_target` 在发布前再次核对，
  不一致就失败，不发布。所以记录的前像就是被替换的确切字节。
- 前像存为名为 `before` 的 Session 文件。`too_large` 不阻止写入，`before` 缺省；宿主没有 Session 存储时
  同样缺省。`put` 的其他失败（`storage`、`node_unavailable`、`not_found` 等）使调用失败，代码
  `checkpoint_failed`，文件不变：有 Session 存储时，每一次记录在案的改动都能还原。
- `put` 之后、发布之前被取消或失败：文件不变，没有效果声明；已保存的前像无人引用，随 Session 删除清理。

## 5. 方法

两个方法与 `session.files.search` 同在 WorkspaceTools 注册的 `session.files` 命名空间
（[ADR 002 D2](../adr/002-plugin-scope.md) 的 `sessionMethods`）。路径都是 `fileWritten` 声明的
绝对解析路径。

### 5.1 `session.files.changes`

只读，`mutating: false`，`Interruption::Report`。

| 请求 | 说明 |
| --- | --- |
| `{sessionId, branchId, turnId}` | 均不为空 |

```text
{ "ended": true,
  "files": [
    { "path": "/work/src/lib.rs", "change": "modified", "calls": 2, "status": "restorable" },
    { "path": "/work/src/new.rs", "change": "created",  "calls": 1, "status": "changed" } ],
  "commands": [ { "command": "cargo fmt", "exitCode": 0 } ] }
```

- `files` 按路径排序，每个文件合并这一轮对它的全部 `write`/`edit`：`change` 取这一轮第一次调用的 `before`
  （`absent` 为 `created`，否则 `modified`），`calls` 为调用次数。
- `status` 由当前内容判定。设这一轮开始前的状态 B（第一次调用的 `before`：不存在，或前像的版本）、
  这一轮结束后的版本 A（最后一次调用的 `revision`）：

  | `status` | 条件 |
  | --- | --- |
  | `restorable` | 当前版本等于 A，且 B 是“不存在”或前像可读、版本相符 |
  | `restored` | 当前状态等于 B（已还原，或被改回原样） |
  | `changed` | 其余情况：这一轮之后被修改、删除或替换成非普通文件 |
  | `unrecorded` | 第一次调用没有 `before`，或前像缺失、内容与记录不符 |

- `commands` 按调用顺序列出这一轮所有 `commandRun` 声明（[A.6](contracts/session-boundary.md#a6-tool-effects)），
  带观察到的 `exitCode` 或 `signal`。
- `ended` 为这一轮是否已有 `turn/end`。分支上没有这一轮（没有它的 `turn/start`）时返回 `invalid_params`。

### 5.2 `session.files.rewind`

`mutating: true`，`Interruption::Drain`：卸载等待它以自己的结果结束。

| 请求 | 说明 |
| --- | --- |
| `{sessionId, branchId, turnId, paths}` | `paths` 不为空、不重复，每个都是这一轮 `write`/`edit` 改过的文件 |

流程：

1. 分支上没有这一轮：`invalid_params`；这一轮没有 `turn/end`：`turn_running`。路径为空、重复或不属于这一轮：`invalid_params`。
2. 按路径排序依次取得路径锁（与 `write`/`edit` 同一个 FIFO 域；排序避免两次还原互相等待）。
3. 锁内重新判定每个路径，任何一个不是 `restorable`：`rewind_conflict`，什么都不改。
   前像在这一步读出并核对版本。
4. 逐个还原：`modified` 用前像经 `write_target` 替换（发布前再核对当前版本等于 A，保留文件元数据，
   与 `write` 的保证相同）；`created` 删除文件并同步父目录。
5. 有文件被还原时，向该分支写一条 Append 输入（§5.4）。

```text
{ "restored": ["/work/src/lib.rs"], "removed": ["/work/src/new.rs"],
  "failed?": { "path": "/work/src/b.rs", "message": "..." }, "recorded": true }
```

- 第 4 步某个文件失败（权限、磁盘、发布前被并发替换）时停在该文件：之前的文件保持还原并写入记录，
  `failed` 给出失败的文件与原因，其余文件不动、仍可再次还原。不做补偿回滚：补偿本身可能失败，
  而“停在原处并如实记录”已满足不覆盖、可重试的要求。
- 调用被取消（卸载）时同样停在文件之间，并记录已还原的部分。
- `recorded` 为记录是否写入；分支已关闭等原因写入失败时为 `false`，文件仍已还原。

### 5.3 exec 不在覆盖范围内时怎么告知用户

- 列表的 `commands` 来自这一轮已记录的 `commandRun` 声明，客户端在列表下方显示“这一轮还运行了 N 条
  命令，命令造成的改动不会撤销”，并列出命令文本。
- `exec` 在这一轮两次 `edit` 之间改了同一个文件（例如格式化），还原到这一轮开始前的内容也撤销了这部分：
  “撤销本轮改动”的含义是回到这一轮开始前。之后才发生的修改由 `changed` 保护，不受影响。
- 命令新建或删除的文件不在列表里；文件被命令删除后再由 `write` 新建，记录为 `created`，还原会删除它。

### 5.4 还原记录与模型上下文

还原后模型的旧认知失效，它之后的决定依赖这件事（[设计规则 §4 规则 9](../architecture/rust-agent/principles.md)），
而进入上下文的唯一路径是被采用的输入（[AGENTS.md](../../AGENTS.md#agent-v4-development-rules) 开发规则 10）。还原因此像 ShellInput 一样写一条 Append 输入：
分支空闲时等下一条驱动输入一起被采用，不单独开启一轮；分支正在运行时在下一个工具结果处进入当前轮
（[InputQueue](../architecture/rust-agent/plugins/InputQueue.md)）。

```
<dim-file-rewind>
restored /work/src/lib.rs
removed /work/src/new.rs
</dim-file-rewind>
```

每行一个文件，`restored` 表示恢复为这一轮之前的内容，`removed` 表示删除了这一轮新建的文件；路径按
运行时标签的规则转义。只列实际完成的文件。指南条目由 WorkspaceTools 的 Session 部件在 `apply` 中
以 Session 范围注册（一次还原可能写到 Session 的任意分支），文本与类别见
[WorkspaceTools 的模型文本](../architecture/rust-agent/plugins/WorkspaceTools.md#model-facing-texts)。

还原不修改分支的读取绑定（FileReads）：绑定记录的是还原前的版本，之后对这些文件的 `write`/`edit`
会以 `file_modified_since_read` 要求先重新读取，这是现有的保护。

## 6. 删除、新建与并发

| 场景 | 结果 |
| --- | --- |
| 这一轮新建的文件 | 删除；父目录保留 |
| 这一轮之后文件被删除 | `changed`，不还原 |
| 删除文件 | `write`/`edit` 不删除文件；删除只可能来自命令，不覆盖 |
| 同一文件这一轮改了多次 | 回到第一次调用之前，核对最后一次调用之后的版本 |
| 后续轮次又改了同一文件 | `changed`；先撤销后面那一轮，这个文件就重新是 `restorable` |
| 正在运行的轮次 | 可列出，不可还原（`turn_running`） |
| 还原时另一轮正在运行、正要写同一文件 | 路径锁使两者串行；锁内核对版本，后写的一方以版本不符失败，不会互相覆盖 |
| 两个 Session 共用工作区 | 写入协调器跨 Session 共享，同样串行 |
| 列表与还原之间用户又改了文件 | 第 3 步判定为非 `restorable`，`rewind_conflict`，什么都不改；客户端刷新列表 |
| 发布前一刻的外部替换 | `write_target` 的核对不是可移植的 CAS，与 `write` 的限制相同 |
| 前像文件缺失或被改动 | `unrecorded`，不还原 |

## 7. 依赖

| 服务 | 用途 |
| --- | --- |
| `S:TreeCommands`（已有） | 按 `branchId` 取得分支门面：读取这一轮的事实，写还原记录 |
| `P:SessionFilesHost`（已有，进程内读取，不注入） | 保存前像；宿主没有时不保存 |
| `S:GuideEntries`（新增） | 注册 `dim-file-rewind` 的指南条目；同一插件的 BackgroundExec 已依赖它，装配 WorkspaceTools 的宿主都具备 |

## 8. 影响面与 Requirements raised to mechanism owners

| 改动 | 所有者 | 理由 |
| --- | --- | --- |
| `write`/`edit` 结果的 `before`，失败代码 `checkpoint_failed` | WorkspaceTools | 私有内容；代码随 `structuredContent` 到达客户端，句子是协议文本 |
| `session.files.changes`、`session.files.rewind` | WorkspaceTools | 公开方法：Web 工作台消费；没有现有方法能列出或还原一轮的改动 |
| `dim-file-rewind` 标记与指南条目 | WorkspaceTools | 公开的模型文本：还原改变了模型读过的文件，只有输入能把这一点带进上下文 |
| Web 工作台：轮次入口、改动对话框、还原通知 | Web 工作台 | 客户端功能（[Web workspace](web-workspace.md)） |

Timeline、Dispatcher、事件格式、`ToolEffect`、Session 与分支门面、SessionStore 契约、存储格式都不变。

Requirements raised to mechanism owners：无（§4 规则 8）。前像用 `sessionFiles.put` 的现有契约，
读取用现有事件查询，记录用现有输入入口。

## 9. Rule check

WorkspaceTools 的 Session 部件不是 Capability：它不按序号折叠事实，也不写自己领域的事实；还原记录
是经分支门面写入的普通输入（[设计规则 §1](../architecture/rust-agent/principles.md)）。

| 规则 | 本设计的适用方式 |
| --- | --- |
| §4 规则 1 | 不改别的模块拥有的状态。工作区文件的写入本就归 WorkspaceTools；对分支只写输入，由 InputQueue 与 Loop 按自己的规则处理 |
| §4 规则 2 | 记录一经写入不再改写；撤回它是 InputQueue 的 `input/withdraw` |
| §4 规则 3 | 不注册 `before` 回调；前像保存、文件 I/O 都在调用自己的任务里，不进入提交锁 |
| §4 规则 4 | 核心机制不带回退含义：Rpc 按命名空间分发，标记是普通 raw 块，Timeline 与 Dispatcher 不变 |
| §4 规则 5 | 还原记录是单个事件，不依赖同批次的其他事实；读取时消费已提交的事实 |
| §4 规则 6 | 还原经 InputQueue 采用、Loop 消费后才进入上下文；列表与前像从不进入上下文 |
| §4 规则 7 | 只用公开的分支门面（事件查询、输入）、`sessionFiles` 与 `guideEntries` 服务，不读别的插件的内部状态 |
| §4 规则 8 | 基础设施不变，无需求提给运行时或 rutis（§8） |
| §4 规则 9 | 新增的 Timeline 内容只有还原记录，模型之后的决定依赖它；改动列表、文件状态是方法回复，不进 Timeline，也不需要广播 |
| 开发规则 10 | 还原记录以带最终内容的 Append 输入进入上下文，不另开上下文路径 |
| 开发规则 11 | 还原是用户经 Session 方法发起的动作，不是 Agent 决定的工具调用，不经 Approval 与 Permission；方法只接受这一轮 `write`/`edit` 改过的路径 |
| ADR 002 D1 | 前像随 Session（SessionStore 的 Session 文件），记录随分支 Timeline，方法在 Session 部件，各归其所有者 |
| ADR 002 D2 | 扩展已有的 WorkspaceTools 与 `session.files` 命名空间，不新增插件、接线模块或注册端口 |
| ADR 002 D3 | `sessionFiles` 按调用查找、可缺失，缺失只影响能否保存前像；GuideEntries 是同一插件已依赖的核心服务 |
| ADR 002 D4 | 没有配置；不触发重载 |
| ADR 002 D5 | 方法的请求、回复与错误都是可序列化数据（§5），Rust 写法在附录 A |

### 消融检查

- **不新增插件或部件。** 前像必须在路径锁内、发布之前取得，还原必须进入同一个锁域，两者都只能在
  WorkspaceTools。
- **不新增 Timeline 事件或效果种类。** 改动记录复用 `step/tool-result`，前像引用放在私有的
  `structuredContent`；曾考虑扩展 `fileWritten`，它是封闭词汇且会迫使存储升级版本，去掉后满足同样的需求。
- **不保存改动后的内容。** 只需核对版本，`revision` 已有。
- **不建索引或缓存。** 每次列出都从 Timeline 与磁盘现算；一轮的事实有界，代价可接受。
- **不做补偿回滚、强制覆盖、redo。** 不是需求所必需；停在原处并如实记录已满足“不覆盖、可重试”。
- **还原记录是必需的。** 去掉它，模型不知道文件已回到旧内容，用户也看不到还原发生过；它同时是持久记录。
- **请求带 `paths` 是必需的。** 用户确认的是自己看到的那组文件；服务端按这组文件全部核对，不一致就不动，
  避免“看到 3 个、实际还原 2 个”。

## 10. 验收

1. 隔离 home、本地模拟模型：一轮里 `write` 新建文件、`edit` 修改已有文件两次、`exec` 运行一条命令；
   `session.files.changes` 列出两个文件（`created`、`modified`，`calls` 为 1 和 2）与那条命令。
2. 还原后，被修改的文件与这一轮之前逐字节一致（含 CRLF、BOM 等原样字节），新建的文件被删除；
   列表变为 `restored`；分支上出现 `dim-file-rewind` 输入，下一次请求的上下文含该标记与指南条目。
3. 这一轮之后用户手工修改其中一个文件：该文件为 `changed`；请求还原它得到 `rewind_conflict`，所有文件不变。
4. 运行中的轮次请求还原得到 `turn_running`；没有 Session 存储的宿主里，修改已有文件的调用为
   `unrecorded`，新建的文件仍可删除。
5. 重启 App Server 后，同一轮的列表与重启前一致。

## Appendix A. Rust binding

- 捕获在 `workspace_tools/files/write.rs` 与 `edit.rs`：原有的锁内阻塞流程拆为“准备”与“发布”两段，
  中间异步调用 `SessionFilesHost::put`；`FileTools` 持有可选的前像存储（Session id 与 `SessionFilesHost`）。
- 方法在 `workspace_tools/rewind.rs`：经 `TreeCommands::get(branchId)` 取得 `Branch`，以
  `Branch::query` 读取谓词 `turn_id` 下的 `turn/start`、`turn/end`，再分页读取
  `step/tool-call`、`step/tool-result`；路径锁用 `FileWriteCoordinator::lock`；替换用 `write::write_target`。
- 还原记录：`Branch::input(InputMessageData { blocks: [raw(marker, None)], mode: Some(InputMode::Append), .. })`。
- 指南条目：`GuideEntries::register(ctx, GuideScope::Session, GuideEntry { markers: ["dim-file-rewind"], .. })`。
