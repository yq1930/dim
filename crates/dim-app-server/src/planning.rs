//! Planning ([design](../../../docs/design/plugins/Planning.md)): the task list
//! (`todo_write`) and plan mode (`session.plan.*`, `exit_plan_mode`). Both keep
//! their state as keyed blocks of the branch's own inputs, so the model reads
//! the latest of each in context, compaction replays it, and the state is
//! rebuilt by reading the branch's inputs back. The only memory the part
//! holds is the live waits of `exit_plan_mode` calls.
use dim_agent_capabilities::{
    host::{HostError, HostFuture, ToolExecutionContext},
    methods::{Interruption, MethodDecl, RpcError, SessionMethods, params, reply},
    permission::{Permission, PermissionState, PermissionUpdateError},
    timeline_tree::TreeCommands,
    tool_registry::{ToolContribution, ToolHandler, ToolRegistry, definition},
};
use dim_agent_runtime::{
    AgentBuilder, AgentError, CAPABILITY_RELOADING, Feature, Names, SessionConfig, TimelineQuery,
    TimelineQueryDirection, TimelineQueryOrderBy, TimelineQueryScope,
};
use dim_agent_types::{
    EventKind, InputBlock, InputMessageData, InputMode, PermissionPolicy, PermissionPreset,
    ToolCall, ToolDefinition, ToolResult, ToolSource,
};
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    num::NonZeroU64,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const TODO_TOOL: &str = "todo_write";
const EXIT_TOOL: &str = "exit_plan_mode";
const NAMESPACE: &str = "session.plan";
const SET: &str = "session.plan.set";
const RESPOND: &str = "session.plan.respond";
/// The keys of the task list's block and of plan mode's block.
const TODO_KEY: &str = "todo:list";
const PLAN_KEY: &str = "plan:mode";
const MAX_TODOS: usize = 50;
const MAX_TODO_CHARS: usize = 300;
const MAX_PLAN_CHARS: usize = 40_000;
/// Inputs read per page while looking for the branch's latest plan block.
const PLAN_PAGE: u64 = 200;

const TODO_DESCRIPTION: &str = "Create or replace the task list for the current work; each call sends the whole list. The user sees the list while you work. Use it when the work has three or more steps or the user gives several tasks, not for a single quick step. Keep one task in_progress at a time, mark each completed as soon as it is done, and add tasks you discover.";
const EXIT_DESCRIPTION: &str = "Submit the plan for the user's review and leave plan mode. Call it only while plan mode is on and the plan is complete: markdown that starts with a # heading and names the changes, the files and how they will be checked. The user either approves it, which turns plan mode off so you carry it out, or asks for changes, which keeps plan mode on.";
const PLAN_ON: &str = "Plan mode is on. Investigate and design without changing anything: read and search, but make no edits and run no commands that change files or state; the session stays read-only until the user approves a plan. Ask the user when a decision is theirs. When the plan is complete, call exit_plan_mode with it.";
const PLAN_OFF: &str =
    "Plan mode is off. Changes are allowed again under the session's permissions.";
const APPROVED: &str = "The user approved the plan. Plan mode is off: carry out the plan now.";
const NOT_PLANNING: &str =
    "Plan mode is off, so there is no plan to submit; continue with the work.";
const NO_REVIEWER: &str = "No one can review a plan in this session. Plan mode stays on.";
const CANCELLED: &str = "The plan review was cancelled before the user decided.";

/// Installs Planning in the profile: one Session part with both tools and the
/// `session.plan` namespace.
pub fn install(builder: &mut AgentBuilder) {
    builder.require_feature(Feature::new("Planning").session("Planning", |names| {
        Ok(Box::new(PlanningPlugin::new(names)?))
    }));
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TodoItem {
    content: String,
    status: TodoStatus,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TodoInput {
    todos: Vec<TodoItem>,
}

/// `&`, `<`, `>` and `"` as entities, so a block's text cannot close or
/// forge its element.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn block_input(text: String, key: &str, retire: bool) -> InputMessageData {
    InputMessageData {
        submission_id: Some(format!("planning:{}", uuid::Uuid::new_v4())),
        user: None,
        blocks: vec![InputBlock::Raw {
            text,
            key: Some(key.to_owned()),
            retire,
        }],
        mode: Some(InputMode::Append),
    }
}

/// The checked list, or the reason the model reads.
fn parse_todos(call: &ToolCall) -> Result<Vec<TodoItem>, String> {
    let input: TodoInput = serde_json::from_value(Value::Object(call.input.clone()))
        .map_err(|error| format!("Invalid todo_write input: {error}"))?;
    if input.todos.len() > MAX_TODOS {
        return Err(format!("todos holds at most {MAX_TODOS} tasks"));
    }
    input
        .todos
        .into_iter()
        .map(|item| {
            let content = item
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if content.is_empty() || content.chars().count() > MAX_TODO_CHARS {
                return Err(format!(
                    "each task's content must be 1 to {MAX_TODO_CHARS} characters"
                ));
            }
            Ok(TodoItem {
                content,
                status: item.status,
            })
        })
        .collect()
}

/// The task list's block: one line per task, `[x]` done, `[>]` in progress,
/// `[ ]` pending.
fn todo_block(todos: &[TodoItem]) -> String {
    let completed = todos
        .iter()
        .filter(|item| item.status == TodoStatus::Completed)
        .count();
    let lines: Vec<String> = todos
        .iter()
        .map(|item| {
            let mark = match item.status {
                TodoStatus::Completed => "[x]",
                TodoStatus::InProgress => "[>]",
                TodoStatus::Pending => "[ ]",
            };
            format!("{mark} {}", escape(&item.content))
        })
        .collect();
    format!(
        "<dim-todo completed=\"{completed}\" total=\"{}\">\n{}\n</dim-todo>",
        todos.len(),
        lines.join("\n")
    )
}

fn todo_summary(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "Task list cleared.".to_owned();
    }
    let completed = todos
        .iter()
        .filter(|item| item.status == TodoStatus::Completed)
        .count();
    let active: Vec<&str> = todos
        .iter()
        .filter(|item| item.status == TodoStatus::InProgress)
        .map(|item| item.content.as_str())
        .collect();
    let mut summary = format!(
        "Task list updated: {completed} of {} completed.",
        todos.len()
    );
    if !active.is_empty() {
        summary.push_str(&format!(" In progress: {}.", active.join("; ")));
    }
    summary
}

/// What the branch's latest plan block says.
#[derive(Default)]
struct PlanState {
    active: bool,
    /// The preset plan mode replaced, restored when it ends.
    restore: Option<PermissionPreset>,
}

fn attribute<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let head = text.split('>').next()?;
    let start = head.find(&format!(" {name}=\""))? + name.len() + 3;
    let length = head[start..].find('"')?;
    Some(&head[start..start + length])
}

impl PlanState {
    fn of(text: &str) -> Self {
        Self {
            active: attribute(text, "status") == Some("active"),
            restore: attribute(text, "restore")
                .and_then(|preset| serde_json::from_value(Value::from(preset)).ok()),
        }
    }
}

fn plan_on(restore: PermissionPreset) -> InputMessageData {
    let restore = serde_json::to_value(restore)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    block_input(
        format!("<dim-plan status=\"active\" restore=\"{restore}\">\n{PLAN_ON}\n</dim-plan>"),
        PLAN_KEY,
        false,
    )
}

fn plan_off() -> InputMessageData {
    block_input(
        format!("<dim-plan status=\"off\">\n{PLAN_OFF}\n</dim-plan>"),
        PLAN_KEY,
        true,
    )
}

fn permission_error(error: PermissionUpdateError) -> RpcError {
    match error {
        PermissionUpdateError::Conflict { .. } => {
            RpcError::app(-32000, "permission_conflict", error.to_string())
        }
        PermissionUpdateError::Closed => RpcError::reloading(),
        PermissionUpdateError::Failed(_) => {
            RpcError::app(-32000, "permission_failed", error.to_string())
        }
    }
}

/// How the user decided an `exit_plan_mode` call.
enum Decision {
    Approve,
    Revise(Option<String>),
}

#[derive(Default)]
struct Waiters {
    /// Set by the unload; no call registers afterwards.
    closed: bool,
    /// By `(branch_id, request_id, call_id)`.
    table: HashMap<(String, String, String), oneshot::Sender<Decision>>,
}

struct Planning {
    config: Arc<SessionConfig>,
    tree: Arc<TreeCommands>,
    waiters: Mutex<Waiters>,
    /// Serializes entering and leaving plan mode, which read before they write.
    transitions: tokio::sync::Mutex<()>,
}

impl Planning {
    async fn plan_state(&self, branch: &str) -> Result<PlanState, AgentError> {
        let mut offset = 0;
        loop {
            let reply = self
                .tree
                .query(
                    branch.to_owned(),
                    TimelineQuery {
                        scope: TimelineQueryScope::Branch,
                        from_event: None,
                        from_sequence: None,
                        order_by: TimelineQueryOrderBy::Sequence,
                        direction: TimelineQueryDirection::Descending,
                        include_publishing: true,
                        kinds: vec!["input/message".to_owned()],
                        flags: Vec::new(),
                        event_id: None,
                        predicate: None,
                        limit: NonZeroU64::new(PLAN_PAGE).expect("nonzero page"),
                        offset,
                    },
                )
                .await?;
            for event in &reply.events {
                let EventKind::InputMessage(data) = &event.kind else {
                    continue;
                };
                for block in &data.blocks {
                    let InputBlock::Raw { text, key, .. } = block;
                    if key.as_deref() == Some(PLAN_KEY) {
                        return Ok(PlanState::of(text));
                    }
                }
            }
            if (reply.events.len() as u64) < PLAN_PAGE {
                return Ok(PlanState::default());
            }
            offset += PLAN_PAGE;
        }
    }

    /// Switches `branch` to `preset` from its dispatched policy, keeping the
    /// tool lists.
    async fn set_preset(&self, branch: &str, preset: PermissionPreset) -> Result<(), RpcError> {
        let line = self.tree.get(branch)?;
        let state = line
            .get::<PermissionState>()
            .ok_or_else(RpcError::reloading)?;
        if state.policy.preset == preset {
            return Ok(());
        }
        line.service::<Permission>()?
            .update(
                state.version,
                PermissionPolicy {
                    preset,
                    ..state.policy
                },
            )
            .await
            .map_err(permission_error)?;
        Ok(())
    }

    /// Turns plan mode on: the branch becomes read-only and its plan block
    /// records the preset to restore. False when it was already on.
    async fn enter(&self, branch: &str) -> Result<bool, RpcError> {
        let _transition = self.transitions.lock().await;
        if self.plan_state(branch).await?.active {
            return Ok(false);
        }
        let restore = self
            .tree
            .get(branch)?
            .get::<PermissionState>()
            .ok_or_else(RpcError::reloading)?
            .policy
            .preset;
        self.set_preset(branch, PermissionPreset::ReadOnly).await?;
        if let Err(error) = self.tree.input(branch.to_owned(), plan_on(restore)).await {
            let _ = self.set_preset(branch, restore).await;
            return Err(error.into());
        }
        Ok(true)
    }

    /// Turns plan mode off. The preset it replaced comes back only while the
    /// branch is still read-only: a preset the user chose meanwhile stays.
    /// False when it was already off.
    async fn leave(&self, branch: &str) -> Result<bool, RpcError> {
        let _transition = self.transitions.lock().await;
        let state = self.plan_state(branch).await?;
        if !state.active {
            return Ok(false);
        }
        let read_only = self
            .tree
            .get(branch)?
            .get::<PermissionState>()
            .is_some_and(|current| current.policy.preset == PermissionPreset::ReadOnly);
        if let Some(restore) = state.restore
            && read_only
        {
            self.set_preset(branch, restore).await?;
        }
        self.tree.input(branch.to_owned(), plan_off()).await?;
        Ok(true)
    }

    async fn todo(&self, call: ToolCall, context: ToolExecutionContext) -> ToolResult {
        let todos = match parse_todos(&call) {
            Ok(todos) => todos,
            Err(reason) => return ToolResult::error(reason),
        };
        let input = block_input(todo_block(&todos), TODO_KEY, todos.is_empty());
        if let Err(error) = self.tree.input(context.branch_id, input).await {
            return ToolResult::error(format!("The task list was not recorded: {error}"));
        }
        ToolResult {
            structured_content: json!({ "todos": todos }).as_object().cloned(),
            ..ToolResult::text(todo_summary(&todos))
        }
    }

    async fn exit(
        &self,
        call: ToolCall,
        context: ToolExecutionContext,
        cancel: CancellationToken,
    ) -> Result<ToolResult, HostError> {
        let plan = match call.input.get("plan") {
            Some(Value::String(plan)) if !plan.trim().is_empty() => plan,
            _ => {
                return Ok(ToolResult::error(
                    "exit_plan_mode requires plan: the complete plan as markdown.",
                ));
            }
        };
        if plan.chars().count() > MAX_PLAN_CHARS {
            return Ok(ToolResult::error(format!(
                "The plan exceeds {MAX_PLAN_CHARS} characters; shorten it."
            )));
        }
        let branch = context.branch_id.clone();
        let state = self
            .plan_state(&branch)
            .await
            .map_err(|error| HostError::failed(error.to_string()))?;
        if !state.active {
            return Ok(ToolResult::error(NOT_PLANNING));
        }
        if !self.config.interactive {
            return Ok(ToolResult::error(NO_REVIEWER));
        }
        if cancel.is_cancelled() {
            return Err(HostError::Cancelled(CANCELLED.into()));
        }
        let key = (branch.clone(), context.request_id, call.id);
        let (sender, mut decided) = oneshot::channel();
        {
            let mut waiters = self.waiters.lock().unwrap();
            if waiters.closed {
                return Err(HostError::Cancelled(CAPABILITY_RELOADING.into()));
            }
            if waiters.table.contains_key(&key) {
                return Ok(ToolResult::error(format!(
                    "Invalid exit_plan_mode input: duplicate call id {}",
                    key.2
                )));
            }
            waiters.table.insert(key.clone(), sender);
        }
        let decision = tokio::select! {
            biased;
            decision = &mut decided => decision,
            _ = cancel.cancelled() => {
                if self.waiters.lock().unwrap().table.remove(&key).is_some() {
                    return Err(HostError::Cancelled(CANCELLED.into()));
                }
                decided.await
            }
        };
        match decision.map_err(|_| HostError::Cancelled(CAPABILITY_RELOADING.into()))? {
            Decision::Approve => {
                self.leave(&branch)
                    .await
                    .map_err(|error| HostError::failed(error.message))?;
                Ok(ToolResult {
                    structured_content: json!({ "approved": true }).as_object().cloned(),
                    ..ToolResult::text(APPROVED)
                })
            }
            Decision::Revise(feedback) => {
                let text = match &feedback {
                    Some(feedback) => format!(
                        "The user did not approve the plan yet and wants it revised: {feedback}\nPlan mode stays on. Revise the plan and call exit_plan_mode again."
                    ),
                    None => "The user did not approve the plan yet and wants to keep planning. Plan mode stays on: ask what to change, or revise the plan and call exit_plan_mode again.".to_owned(),
                };
                Ok(ToolResult {
                    structured_content: json!({ "approved": false, "feedback": feedback })
                        .as_object()
                        .cloned(),
                    ..ToolResult::text(text)
                })
            }
        }
    }

    fn respond(&self, value: Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Params {
            /// Read by Rpc to route the call; the provider serves this Session.
            #[serde(rename = "sessionId")]
            _session_id: String,
            branch_id: String,
            /// The `request_id` of the call's `step/tool-call`.
            request_id: String,
            call_id: String,
            approved: bool,
            feedback: Option<String>,
        }
        let p: Params = params(value)?;
        let feedback = p
            .feedback
            .map(|feedback| feedback.trim().to_owned())
            .filter(|feedback| !feedback.is_empty());
        if p.approved && feedback.is_some() {
            return Err(RpcError::invalid(
                "feedback is sent only with approved false",
            ));
        }
        let sender = self
            .waiters
            .lock()
            .unwrap()
            .table
            .remove(&(p.branch_id, p.request_id, p.call_id))
            .ok_or_else(|| {
                RpcError::app(
                    -32000,
                    "plan_not_pending",
                    "No exit_plan_mode call is waiting for this decision",
                )
            })?;
        let _ = sender.send(if p.approved {
            Decision::Approve
        } else {
            Decision::Revise(feedback)
        });
        reply(json!({}))
    }

    async fn set(&self, value: Value) -> Result<Value, RpcError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Params {
            #[serde(rename = "sessionId")]
            _session_id: String,
            #[serde(default = "main")]
            branch_id: String,
            active: bool,
        }
        fn main() -> String {
            "main".to_owned()
        }
        let p: Params = params(value)?;
        let changed = if p.active {
            self.enter(&p.branch_id).await?
        } else {
            self.leave(&p.branch_id).await?
        };
        reply(json!({ "active": p.active, "changed": changed }))
    }
}

struct TodoWrite(Arc<Planning>);
impl ToolHandler for TodoWrite {
    fn execute(
        &self,
        call: ToolCall,
        context: ToolExecutionContext,
        _cancel: CancellationToken,
    ) -> HostFuture<'_, ToolResult> {
        Box::pin(async move { Ok(self.0.todo(call, context).await) })
    }
}

struct ExitPlanMode(Arc<Planning>);
impl ToolHandler for ExitPlanMode {
    fn execute(
        &self,
        call: ToolCall,
        context: ToolExecutionContext,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ToolResult> {
        Box::pin(self.0.exit(call, context, cancel))
    }
}

fn session_tool(name: &str, description: &str, schema: Value) -> ToolDefinition {
    let mut tool = definition(name, description, schema);
    tool.source = ToolSource::Extension;
    tool.risk = Some("session".into());
    tool
}

fn todo_definition() -> ToolDefinition {
    session_tool(
        TODO_TOOL,
        TODO_DESCRIPTION,
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["todos"],
            "properties": {
                "todos": {
                    "type": "array",
                    "maxItems": MAX_TODOS,
                    "description": "The whole list, in order; an empty list clears it.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["content", "status"],
                        "properties": {
                            "content": {"type": "string", "maxLength": MAX_TODO_CHARS, "description": "The task as a short imperative line."},
                            "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                        }
                    }
                }
            }
        }),
    )
}

fn exit_definition() -> ToolDefinition {
    session_tool(
        EXIT_TOOL,
        EXIT_DESCRIPTION,
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["plan"],
            "properties": {
                "plan": {"type": "string", "maxLength": MAX_PLAN_CHARS, "description": "The complete plan as markdown, starting with a # heading."}
            }
        }),
    )
}

/// The Planning Session part: `todo_write` and `exit_plan_mode` registered
/// with `toolRegistry` and `session.plan` with `sessionMethods`.
pub struct PlanningPlugin {
    config: Key<SessionConfig>,
    tree: Key<TreeCommands>,
    tools: Key<ToolRegistry>,
    methods: Key<SessionMethods>,
}
impl PlanningPlugin {
    pub fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            config: names.key()?,
            tree: names.key()?,
            tools: names.key()?,
            methods: names.key()?,
        })
    }
}
impl DynPlugin for PlanningPlugin {
    fn name(&self) -> &str {
        "Planning"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.config.erase(),
            self.tree.erase(),
            self.tools.erase(),
            self.methods.erase(),
        ]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let planning = Arc::new(Planning {
                config: ctx.require_at(&self.config)?,
                tree: ctx.require_at(&self.tree)?,
                waiters: Mutex::default(),
                transitions: tokio::sync::Mutex::new(()),
            });
            let tools = ctx.require_at(&self.tools)?;
            let methods = ctx.require_at(&self.methods)?;
            let owner = planning.clone();
            ctx.effect(move || async move {
                let mut waiters = owner.waiters.lock().unwrap();
                waiters.closed = true;
                waiters.table.clear();
                Ok(())
            })?;
            tools.register(
                ctx,
                ToolContribution {
                    definition: todo_definition(),
                    handler: Arc::new(TodoWrite(planning.clone())),
                },
            )?;
            tools.register(
                ctx,
                ToolContribution {
                    definition: exit_definition(),
                    handler: Arc::new(ExitPlanMode(planning.clone())),
                },
            )?;
            methods.register(
                ctx,
                NAMESPACE,
                vec![
                    MethodDecl::new(SET, true, Interruption::Report),
                    MethodDecl::new(RESPOND, true, Interruption::Report),
                ],
                Arc::new(move |method, value, _, _| {
                    let planning = planning.clone();
                    Box::pin(async move {
                        if method == RESPOND {
                            planning.respond(value)
                        } else {
                            planning.set(value).await
                        }
                    })
                }),
            )?;
            Ok(Effect::Done)
        })
    }
}
