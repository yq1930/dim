//! The Session part: the `schedule` tool, which every Session declares. The
//! handler refuses non-interactive Sessions and Collaboration members before
//! it calls the process service ([design §1.1, A.2](../../../docs/design/plugins/Scheduler.md)).
use crate::methods::{Caller, ScheduleJobs, ToolRequest};
use dim_agent_capabilities::{
    host::{HostFuture, ToolExecutionContext},
    prompt_guide::{GuideEntries, GuideEntry, GuideScope},
    tool_registry::{ToolContribution, ToolHandler, ToolRegistry, definition},
};
use dim_agent_runtime::{CollaborationRole, Names, SessionConfig, SessionIdentity};
use dim_agent_types::{ToolCall, ToolDefinition, ToolResult, ToolSource};
use rutis::Ctx;
use rutis::host::{BoxFuture, CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) const DESCRIPTION: &str = "Manage scheduled jobs across this host, including other workspaces. Every create/update, even a rename, replaces the saving branch and authority cap; future reports go to this branch. An omitted policy keeps its stored value, whose default can change with the saving origin. Inspect the old-to-new authority transition in the result.";

const UNAVAILABLE: &str = "schedule is available only in interactive leader Sessions.";
const UNLOADING_BEFORE_CALL: &str = "tool provider generation is unloading; this call was not made. The tool is offered again once its provider is active; a later request uses the current provider.";
const UNLOADING_AFTER_CALL: &str = "tool provider generation is unloading; this call may already have taken effect. Check the operation's state before repeating it with the current provider.";
const CANCELLED_BEFORE_HANDOVER: &str = "schedule: cancelled before handover";
/// The `dim-scheduler` guide entry (design §5.2), verbatim.
const ENTRY: &str = "<dim-scheduler kind=\"scheduled-run\" job=\"nightly-build\" run=\"run-7f2\">: a scheduled job's saved prompt, delivered when the job runs, and <dim-scheduler kind=\"report\" job=\"nightly-build\" run=\"run-7f2\">: how that run ended. job names the job, run names the run, in both. In the scheduled-run tag the body is the job's name, and the message after the tag is the prompt saved for the job: the request this run carries out, in the operator's channel. In the report tag the body states the run's outcome and, when known, the session it ran in: the scheduler's account of how the run ended, information, not a request. The report's last line is not part of that account: it is guidance on what to do next.";

fn declaration(description: &str) -> ToolDefinition {
    let mut tool = definition(
        "schedule",
        description,
        json!({
            "type": "object",
            "required": ["action"],
            "additionalProperties": false,
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "create", "update", "delete", "set_enabled", "run_now", "history"]
                },
                "job_id": {"type": "string"},
                "enabled": {"type": "boolean"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                "job": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "name": {"type": "string"},
                        "schedule": {"type": "object"},
                        "prompt": {"type": "string"},
                        "cwd": {"type": "string"},
                        "model": {
                            "type": "object",
                            "required": ["providerId", "modelId"],
                            "additionalProperties": false,
                            "properties": {
                                "providerId": {"type": "string"},
                                "modelId": {"type": "string"}
                            }
                        },
                        "mode": {"type": "string", "enum": ["create", "reuse"]},
                        "policy": {"type": "string", "enum": ["read-only", "ask", "workspace-write", "full-access"]},
                        "max_run_minutes": {"type": "integer", "minimum": 1}
                    }
                }
            }
        }),
    );
    tool.source = ToolSource::Extension;
    tool.risk = Some("persistent_configuration".into());
    tool
}

pub struct SchedulerSession {
    jobs: Key<ScheduleJobs>,
    tools: Key<ToolRegistry>,
    guide: Key<GuideEntries>,
    config: Key<SessionConfig>,
    identity: Key<SessionIdentity>,
    description: String,
}

impl SchedulerSession {
    pub fn new(names: &Names<'_>, description: String) -> rutis::Result<Self> {
        Ok(Self {
            jobs: names.key()?,
            tools: names.key()?,
            guide: names.key()?,
            config: names.key()?,
            identity: names.key()?,
            description,
        })
    }
}

impl DynPlugin for SchedulerSession {
    fn name(&self) -> &str {
        "SchedulerSession"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.jobs.erase(),
            self.tools.erase(),
            self.guide.erase(),
            self.config.erase(),
            self.identity.erase(),
        ]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let jobs = ctx.require_at(&self.jobs)?;
            let tools = ctx.require_at(&self.tools)?;
            let config = ctx.require_at(&self.config)?;
            let identity = ctx.require_at(&self.identity)?;
            let closing = ctx.cancellation_token();
            ctx.require_at(&self.guide)?.register(
                ctx,
                GuideScope::Session,
                GuideEntry {
                    markers: vec!["dim-scheduler".into()],
                    order: 110,
                    text: ENTRY.into(),
                },
            )?;
            tools.register(
                ctx,
                ToolContribution {
                    definition: declaration(&self.description),
                    handler: Arc::new(ScheduleTool {
                        jobs,
                        config,
                        session_id: identity.session_id.clone(),
                        closing,
                    }),
                },
            )?;
            Ok(Effect::Done)
        })
    }
}

struct ScheduleTool {
    jobs: Arc<ScheduleJobs>,
    config: Arc<SessionConfig>,
    session_id: String,
    /// Cancelled when this generation starts to unload.
    closing: CancellationToken,
}

impl ToolHandler for ScheduleTool {
    fn execute(
        &self,
        call: ToolCall,
        context: ToolExecutionContext,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ToolResult> {
        Box::pin(async move {
            if self.closing.is_cancelled() {
                return Ok(ToolResult::error(UNLOADING_BEFORE_CALL));
            }
            if !self.config.interactive
                || self.config.collaboration_role == CollaborationRole::Member
            {
                return Ok(ToolResult::error(UNAVAILABLE));
            }
            let request: ToolRequest = match serde_json::from_value(Value::Object(call.input)) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(ToolResult::error(format!(
                        "schedule: schedule_invalid\n{error}"
                    )));
                }
            };
            let caller = Caller {
                session_id: self.session_id.clone(),
                branch_id: context.branch_id,
                cwd: self.config.cwd.clone(),
            };
            if self.closing.is_cancelled() {
                return Ok(ToolResult::error(UNLOADING_BEFORE_CALL));
            }
            if cancel.is_cancelled() {
                return Ok(ToolResult::error(CANCELLED_BEFORE_HANDOVER));
            }
            let reply = self.jobs.call(request, caller).await;
            if self.closing.is_cancelled() {
                return Ok(ToolResult::error(UNLOADING_AFTER_CALL));
            }
            Ok(match reply {
                Ok(reply) => {
                    let mut result = ToolResult::text(reply.content);
                    result.structured_content = reply.records.as_object().cloned();
                    result
                }
                Err(error) => {
                    ToolResult::error(format!("schedule: {}\n{}", error.code, error.message))
                }
            })
        })
    }
}
