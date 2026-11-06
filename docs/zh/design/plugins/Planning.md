# Planning

状态：Ready。交付状态在 issue 中跟踪，不写在这里。

作用域：Session。App Server 产品装配中的内置部件（[planning.rs](../../../../crates/dim-app-server/src/planning.rs)）。
规则：[设计规则](../../../architecture/rust-agent/principles.md)、[ADR 002](../../../adr/002-plugin-scope.md)。

Planning 为 agent 提供两种用户可以跟进的工作组织方式：模型用 `todo_write` 持续更新的任务清单；以及计划模式，在该模式下 agent 只做调查，直到用户批准它用 `exit_plan_mode` 提交的计划。两者取代 Todo 状态与 `plan` Skill 原先的“不做”条目（[总体设计 §1](../overview.md#1-范围与前提)）。任务清单参照 DeepSeek Harness 整体替换的 `todo_write`；计划模式参照其经用户评审的 `exit_plan_mode` 与 ZCode 的只读规划。

## 1. 范围与非目标

Planning 拥有两个工具、Session 方法 `session.plan.set` 与 `session.plan.respond`，以及 `exit_plan_mode` 调用的等待。它不拥有自己切换的权限策略（属于 Permission）、承载其状态的输入（属于各分支的 Timeline），也不规定客户端如何绘制。

非目标：模型自行进入计划模式、对话之外保存的计划文件、跨分支共享的任务清单，以及 TUI 的计划模式控件。TUI 中的 Session 不会进入计划模式，因此 `exit_plan_mode` 只会报告计划模式未开启。

## 2. 自有契约

**`todo_write {todos: [{content, status}]}`。** 用它发送的完整清单替换调用分支的任务清单：最多 50 项，每项 `content` 在合并空白后为 1 到 300 个字符，`status` 取 `pending`、`in_progress`、`completed` 之一。它在调用分支上写一条输入，返回一行摘要（`Task list updated: 2 of 5 completed. In progress: …`）以及 `structuredContent {todos}`。空清单表示清除。风险为 `session`。

**`exit_plan_mode {plan}`。** `plan` 是完整的 markdown 计划，至多 40,000 个字符。计划模式未开启时立即失败并返回 `Plan mode is off, so there is no plan to submit; continue with the work.`；非交互 Session 不等待直接失败。其余情况等待用户决定：

- 批准：计划模式结束（见下），结果为 `The user approved the plan. Plan mode is off: carry out the plan now.`，`structuredContent {approved: true}`；
- 修改：计划模式保持开启，结果携带用户的反馈（如有），`structuredContent {approved: false, feedback}`。

中断、关闭或重载会像 `ask_user` 一样取消等待（[Questions §4](Questions.md)）。风险为 `session`，因此只读预设允许它。

**`session.plan.set {sessionId, branchId?, active}`**（写操作，`branchId` 默认 `main`）开启或关闭计划模式，回复 `{active, changed}`；分支已处于该模式时 `changed` 为 false。**`session.plan.respond {sessionId, branchId, requestId, callId, approved, feedback?}`** 决定一个等待中的 `exit_plan_mode` 调用，命名方式与 `session.question.respond` 命名 `ask_user` 调用相同；`feedback` 只随 `approved: false` 发送，没有等待的调用时返回 `plan_not_pending`。

**开启计划模式。** 分支的权限预设变为 `read-only`，其计划块记录被替换的预设。**关闭计划模式**（批准或用户关闭）：仅当分支仍为 `read-only` 时恢复该预设，用户期间自行选择的预设保持不变；计划块说明计划模式已关闭。权限冲突时方法以 `permission_conflict` 失败且不做任何改变。

## 3. 事件与状态

Planning 不写自有事件类型。每次状态变化是分支上一条 `append` 模式的 `input/message`，由一个带 key 的原始块组成（[InputBlock](../../../../crates/dim-agent-types/src/events.rs)）：

| Key | 块 | 是否 retire |
| --- | --- | --- |
| `todo:list` | `<dim-todo completed="2" total="5">`，之后每项一行：`[x]` 已完成，`[>]` 进行中，`[ ]` 待办 | 清单被清空时 |
| `plan:mode` | `<dim-plan status="active" restore="<preset>">` 加计划模式说明 | 否 |
| `plan:mode` | `<dim-plan status="off">` 说明可以再次修改 | 是 |

key 使最新的块留在上下文中，压缩会重放它，因此长任务保留清单，计划模式在压缩后依然有效。retire 的关闭块只投递一次，不再重放。状态可从 Timeline 重建：Planning 每次决策时从分支自己的 `input/message` 事实中读取最新的 `plan:mode` 块，客户端则从已收到的事件中折叠这两个 key。唯一的内存状态是等待中的 `exit_plan_mode` 调用表，与 Questions 一样是活动的 I/O 等待。

## 4. 依赖

| 服务 | 用途 |
| --- | --- |
| `S:SessionConfig` | 提交计划时读取 `interactive` |
| `S:TreeCommands` | 写分支输入、查询分支输入，并经分支访问 `Permission` 与 `PermissionState`（[Permission](../../../architecture/rust-agent/plugins/Permission.md)） |
| `S:ToolRegistry` | 注册两个工具 |
| `S:SessionMethods` | 注册 `session.plan` |

## 5. 跨模块要求

无。Planning 使用 Permission 已公开的 Branch 门面，客户端方法 `session.permission.update` 也在使用它。

## 6. 规则检查

| 规则 | 本设计如何符合 |
| --- | --- |
| §1 Capability 与 App Server 边界 | App Server 产品装配中的一个 Session 部件，与 Questions 一同安装；不改动 Capability |
| §4.1 用命令事件改他人状态 | Permission 的状态只经它自己的 `update` 改变，并带版本检查 |
| §4.2–4.3 不撤回、不阻塞回调 | 决定在锁内只交给等待的调用一次；没有 before 回调 |
| §4.4 核心不含业务含义 | Timeline、Dispatcher、ToolExecutor 和 Rpc 没有 Planning 分支 |
| §4.5 原子批次、有序分发 | 每次变化是一条输入；工具结果在 ToolExecutor 的批次中提交 |
| §4.6 上下文只来自已交接事件 | 模型只经已采纳的输入与工具结果读取状态（AGENTS 规则 10） |
| §4.7 只用公开服务 | 经 Branch 门面读取 `PermissionState`、调用 `Permission`；不读私有状态 |
| §4.8 基础设施不变 | 不改 Timeline、信封、装配或门面 |
| §4.9 只记录后续决策会用的事实 | 计划块是判定计划模式的事实；任务清单块是模型与客户端读取的内容 |
| ADR 002 D1 作用域 | 工具、方法与等待是 Session 服务；状态存放在各分支的 Timeline |
| ADR 002 D2 不设接线模块 | 直接注册到 ToolRegistry 与 `sessionMethods` |
| ADR 002 D3 作用域依赖 | 只依赖 Session 服务；没有模块依赖 Planning |
| ADR 002 D4 配置 | 无配置 |
| ADR 002 D5 语言中立契约 | 工具、方法与块均为数据（§2、§3） |
| ADR 003 审批范围 | 两个工具都是 agent 的决定，以 `session` 风险准入；切换预设是用户经客户端方法的操作 |

## 7. 消融审查

| 元素 | 结果 |
| --- | --- |
| `todo/write` 或 `plan/mode` 扩展事件 | 不添加：带 key 的输入既通知模型又是事实，与 Goal 的块相同 |
| 部件内的计划状态缓存 | 不添加：需要时读取最新的 `plan:mode` 块，重载或 fork 无需重建 |
| 供模型使用的 `enter_plan_mode` | 不添加：计划模式由用户选择；agent 仍可以询问 |
| `todo_read` 工具 | 不添加：最新清单经带 key 的块留在上下文中 |
| 切换预设 | 保留：没有它计划模式只是模型可以忽略的建议，而用户期望批准前不发生任何修改 |
