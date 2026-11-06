# Planning

Status: Ready. Delivery status is tracked in the issue, not here.

Scope: Session. Built-in part of the App Server product profile ([planning.rs](../../../crates/dim-app-server/src/planning.rs)).
Rules: [design rules](../../architecture/rust-agent/principles.md), [ADR 002](../../adr/002-plugin-scope.md).

Planning gives the agent two ways to organize work that the user can follow: a task list the model keeps up to date with `todo_write`, and plan mode, in which the agent only investigates until the user approves the plan it submits with `exit_plan_mode`. Both replace the former "not planned" entries for the Todo status and the `plan` Skill ([overview §1](../overview.md#1-scope-and-premises)). The task list follows DeepSeek Harness's whole-list `todo_write`; plan mode follows its user-reviewed `exit_plan_mode` and ZCode's read-only planning.

## 1. Scope and non-goals

Planning owns its two tools, the Session methods `session.plan.set` and `session.plan.respond`, and the live waits of `exit_plan_mode` calls. It does not own the permission policy it switches (Permission's), the inputs that carry its state (each branch's Timeline) or how a client draws them.

Non-goals: the model entering plan mode by itself, a plan file kept outside the conversation, a task list shared between branches, and the TUI's plan-mode controls. A TUI Session never enters plan mode, so `exit_plan_mode` there only reports that plan mode is off.

## 2. Own contract

**`todo_write {todos: [{content, status}]}`.** Replaces the calling branch's task list with the whole list it sends: at most 50 tasks, each `content` 1 to 300 characters after collapsing whitespace, `status` one of `pending`, `in_progress`, `completed`. It writes one input on the calling branch and returns a one-line summary (`Task list updated: 2 of 5 completed. In progress: …`) with `structuredContent {todos}`. An empty list clears it. Risk `session`.

**`exit_plan_mode {plan}`.** `plan` is the complete plan as markdown, at most 40,000 characters. While plan mode is off it fails at once with `Plan mode is off, so there is no plan to submit; continue with the work.`; in a non-interactive Session it fails without waiting. Otherwise it waits for the user's decision:

- approved: plan mode ends (below) and the result says `The user approved the plan. Plan mode is off: carry out the plan now.` with `structuredContent {approved: true}`;
- revise: plan mode stays on and the result carries the user's feedback, if any, with `structuredContent {approved: false, feedback}`.

An interrupt, a close or a reload cancels the wait as for `ask_user` ([Questions §4](Questions.md#4-timing-and-conflicts)). Risk `session`, so read-only admits it.

**`session.plan.set {sessionId, branchId?, active}`** (mutating, `branchId` defaults to `main`) turns plan mode on or off and replies `{active, changed}`; `changed` is false when the branch already was in that mode. **`session.plan.respond {sessionId, branchId, requestId, callId, approved, feedback?}`** decides a waiting `exit_plan_mode` call, named as `session.question.respond` names an `ask_user` call; `feedback` goes only with `approved: false`, and a call that is not waiting fails `plan_not_pending`.

**Plan mode on.** The branch's permission preset becomes `read-only` and its plan block records the preset it replaced. **Plan mode off** (approval or the user): that preset comes back only while the branch is still `read-only`, so a preset the user chose meanwhile stays, and the plan block says plan mode is off. A permission conflict fails the method with `permission_conflict` and changes nothing.

## 3. Events and state

Planning writes no event type of its own. Each state change is one `input/message` on the branch in `append` mode, made of one raw block with a key ([InputBlock](../../../crates/dim-agent-types/src/events.rs)):

| Key | Block | Retired |
| --- | --- | --- |
| `todo:list` | `<dim-todo completed="2" total="5">` then one line per task: `[x]` completed, `[>]` in progress, `[ ]` pending | When the list is emptied |
| `plan:mode` | `<dim-plan status="active" restore="<preset>">` with the plan-mode instruction | No |
| `plan:mode` | `<dim-plan status="off">` saying changes are allowed again | Yes |

The key makes the latest block the one in context, and compaction replays it, so a long task keeps its list and plan mode survives compaction. A retired off block is delivered once and not replayed. The state is rebuilt from the Timeline: Planning reads a branch's latest `plan:mode` block from its own `input/message` facts whenever it decides, and clients fold both keys from the events they already receive. The only memory is the table of waiting `exit_plan_mode` calls, live I/O waits as in Questions.

## 4. Dependencies

| Service | Use |
| --- | --- |
| `S:SessionConfig` | `interactive`, read when a plan is submitted |
| `S:TreeCommands` | Write the branch inputs, query the branch's inputs, and reach the branch's `Permission` and `PermissionState` ([Permission](../../architecture/rust-agent/plugins/Permission.md)) |
| `S:ToolRegistry` | Register both tools |
| `S:SessionMethods` | Register `session.plan` |

## 5. Cross-module requirements

None. Planning uses the published Branch facade of Permission, which the client method `session.permission.update` already uses.

## 6. Rule check

| Rule | How this design complies |
| --- | --- |
| §1 Capabilities and App Server boundary | One Session part in the App Server's product profile, installed next to Questions; no Capability changes |
| §4.1 Command events for others' state | Permission's state changes only through its own `update`, with its version check |
| §4.2–4.3 No recall, no blocking callbacks | A decision is handed to the waiting call once under the lock; no before callback |
| §4.4 No business meaning in core | Timeline, Dispatcher, ToolExecutor and Rpc have no Planning case |
| §4.5 Atomic batches, ordered dispatch | Each change is one input; the tool results commit in ToolExecutor's batch |
| §4.6 Context only from handed-off events | The model reads the state only through adopted inputs and tool results (AGENTS rule 10) |
| §4.7 Public services only | Reads `PermissionState` and calls `Permission` through the Branch facade; reads no private state |
| §4.8 Infrastructure unchanged | No change to Timeline, envelope, assembly or facades |
| §4.9 Only facts later decisions use | The plan block is the fact plan mode is decided from; the task list block is what the model and clients read |
| ADR 002 D1 Scope | Tools, methods and waits are Session services; the state lives in each branch's Timeline |
| ADR 002 D2 No wiring modules | Registers with ToolRegistry and `sessionMethods` directly |
| ADR 002 D3 Scope dependencies | Depends only on Session services; nothing depends on Planning |
| ADR 002 D4 Configuration | No configuration |
| ADR 002 D5 Language-neutral contracts | Tools, methods and blocks are data (§2, §3) |
| ADR 003 Approval scope | Both tools are agent decisions admitted under risk `session`; switching the preset is a user action through a client method |

## 7. Ablation review

| Element | Result |
| --- | --- |
| A `todo/write` or `plan/mode` extension event | Not added: the keyed input both informs the model and is the fact, as Goal's blocks are |
| A plan-state cache in the part | Not added: the latest `plan:mode` block is read when needed, so a reload or a fork needs no rebuild |
| `enter_plan_mode` for the model | Not added: plan mode is the user's choice; the agent can still ask |
| A `todo_read` tool | Not added: the latest list is in context through its keyed block |
| Switching the preset | Kept: without it plan mode is advice the model may ignore, and the user expects no change before approval |
