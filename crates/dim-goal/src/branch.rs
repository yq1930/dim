//! Goal's branch part (design §1, A.6, Pursuit §4): owns this branch's fold,
//! decides continuation from consumed state only, and writes through owned
//! tasks. On a branch Goal created for an evaluation it takes the evaluator
//! role instead (Pursuit §4.2).

use crate::fold::{self, Conclusion, ConclusionAction, Decision, Fold, TurnEnd};
use crate::text::{self, GoalTexts};
use dim_agent_capabilities::timeline_tree::TreeCommands;
use dim_agent_runtime::{
    AgentError, BoxFuture, BranchLifecycle, BranchOptions, Dispatcher, Names, SessionLifecycle,
    Timeline, TimelineIdentity, TimelineQuery, TimelineQueryDirection, TimelineQueryOrderBy,
    TimelineQueryPredicate, TimelineQueryScope,
};
use dim_agent_types::{
    AgentEvent, BranchSource, EventKind, InputBlock, InterruptScope, TurnEndData, TurnOutcome,
};
use dim_app_server::settings::{SettingsScope, SettingsService};
use rutis::Ctx;
use rutis::host::{CtxHost, DynPlugin, Effect, Key, TypeKey};
use serde::Deserialize;
use std::{
    collections::HashSet,
    num::NonZeroU64,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const SOURCE: &str = "goal";

/// The facts the fold consumes (A.6).
const KEYS: &[&str] = &[
    "branch/start",
    "session/forked",
    "branch/create",
    "branch/create-failed",
    "branch/ended",
    "input/message",
    "input/consumed",
    "input/deleted",
    "turn/start",
    "turn/end",
    "stop/interrupt",
];

/// Inherited `input/message` facts one page of the new-branch check keeps.
const INHERITED_PAGE: u64 = 64;
/// Facts one page of the rebuild reads.
const REBUILD_PAGE: u64 = 256;
/// Facts one page of the reply read keeps.
const REPLY_PAGE: u64 = 256;

/// The Settings document Goal declares.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    #[serde(default = "default_limit")]
    max_continuations: u64,
}

fn default_limit() -> u64 {
    20
}

pub struct GoalBranch {
    dispatcher: Key<Dispatcher>,
    timeline: Key<Timeline>,
    tree: Key<TreeCommands>,
    settings: Key<SettingsService>,
    identity: Key<TimelineIdentity>,
    phase: Key<BranchLifecycle>,
    session: Key<SessionLifecycle>,
    texts: Arc<GoalTexts>,
}

impl GoalBranch {
    pub fn new(names: &Names<'_>, texts: Arc<GoalTexts>) -> rutis::Result<Self> {
        Ok(Self {
            dispatcher: names.key()?,
            timeline: names.key()?,
            tree: names.key()?,
            settings: names.key()?,
            identity: names.key()?,
            phase: names.key()?,
            session: names.key()?,
            texts,
        })
    }
}

impl DynPlugin for GoalBranch {
    fn name(&self) -> &str {
        "GoalBranch"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.dispatcher.erase(),
            self.timeline.erase(),
            self.tree.erase(),
            self.settings.erase(),
            self.identity.erase(),
            self.phase.erase(),
            self.session.erase(),
        ]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let dispatcher = ctx.require_at(&self.dispatcher)?;
            let timeline = ctx.require_at(&self.timeline)?;
            let tree = ctx.require_at(&self.tree)?;
            let settings = ctx.require_at(&self.settings)?;
            let identity = ctx.require_at(&self.identity)?;
            let phase = ctx.require_at(&self.phase)?;
            let session = ctx.require_at(&self.session)?;
            let stop = CancellationToken::new();
            let (resolved, _) = watch::channel(false);
            let owner = Arc::new(Owner {
                branch_id: identity.branch_id.clone(),
                timeline,
                tree,
                settings,
                texts: self.texts.clone(),
                phase: phase.clone(),
                session: session.clone(),
                stop: stop.clone(),
                tasks: TaskTracker::new(),
                resolved,
                state: Mutex::new(State::default()),
            });
            let cleanup = owner.clone();
            ctx.effect(move || async move {
                cleanup.stop.cancel();
                cleanup.tasks.close();
                cleanup.tasks.wait().await;
                Ok(())
            })?;
            let closing = owner.clone();
            let lifecycle = phase.clone();
            owner.tasks.spawn(async move {
                tokio::select! {
                    _ = lifecycle.closing() => closing.stop.cancel(),
                    _ = closing.stop.cancelled() => {},
                }
            });
            // Who this branch is, and whether it is an evaluator line, is read
            // once and before any decision: every decision waits for it.
            let resolve = owner.clone();
            owner.tasks.spawn(async move {
                if resolve.phase.activated().await
                    && !resolve.stopped()
                    && let Ok(info) = resolve.tree.info(resolve.branch_id.clone())
                {
                    let evaluation = match &info.source {
                        Some(BranchSource::Host { operation_id }) => {
                            fold::evaluation_of_operation(operation_id)
                        }
                        _ => None,
                    };
                    if let (Some(parent), Some(evaluation)) = (info.parent.clone(), evaluation) {
                        resolve.state.lock().unwrap().role = Some(Role {
                            branch: parent,
                            evaluation,
                        });
                    }
                }
                resolve.resolved.send_replace(true);
            });
            let rebuilding = owner.clone();
            let consuming = owner.clone();
            dispatcher
                .consume(
                    ctx,
                    "Goal".into(),
                    KEYS.iter().map(|key| (*key).to_owned()).collect(),
                    Arc::new(move || {
                        let owner = rebuilding.clone();
                        Box::pin(async move { owner.rebuild().await })
                    }),
                    Arc::new(move |event| {
                        let owner = consuming.clone();
                        Box::pin(async move {
                            owner.ingest(&event, true);
                            Ok(())
                        })
                    }),
                )
                .await?;
            Ok(Effect::Done)
        })
    }
}

#[derive(Clone)]
struct Role {
    /// The branch the evaluator reports to.
    branch: String,
    evaluation: u64,
}

#[derive(Default)]
struct State {
    fold: Fold,
    /// The opening event the fold started at.
    start: Option<u64>,
    role: Option<Role>,
    /// The decision keys whose work is running: each runs once at a time.
    in_flight: HashSet<String>,
}

struct Owner {
    branch_id: String,
    timeline: Arc<Timeline>,
    tree: Arc<TreeCommands>,
    settings: Arc<SettingsService>,
    texts: Arc<GoalTexts>,
    phase: Arc<BranchLifecycle>,
    session: Arc<SessionLifecycle>,
    stop: CancellationToken,
    tasks: TaskTracker,
    /// Set once the branch's identity is read; every decision waits for it.
    resolved: watch::Sender<bool>,
    state: Mutex<State>,
}

impl Owner {
    fn stopped(&self) -> bool {
        self.stop.is_cancelled()
    }

    async fn resolved(&self) {
        let mut resolved = self.resolved.subscribe();
        while !*resolved.borrow() {
            if resolved.changed().await.is_err() {
                return;
            }
        }
    }

    /// Runs `work` once under `key` while the branch is active, and records a
    /// failed write without letting it stop the branch.
    fn task<F>(self: &Arc<Self>, key: String, work: F)
    where
        F: FnOnce(Arc<Self>) -> BoxFuture<'static, Result<(), AgentError>> + Send + 'static,
    {
        if self.stopped() {
            return;
        }
        if !self.state.lock().unwrap().in_flight.insert(key.clone()) {
            return;
        }
        let owner = self.clone();
        let phase = self.phase.clone();
        self.tasks.spawn(async move {
            if phase.activated().await
                && !owner.stopped()
                && let Err(error) = work(owner.clone()).await
            {
                tracing::warn!(key = %key, %error, "Goal write failed");
            }
            owner.state.lock().unwrap().in_flight.remove(&key);
        });
    }

    /// The continuation limit. A refused or unreadable document is logged and
    /// leaves the turn without a decision.
    async fn limit(&self) -> Option<u64> {
        match self
            .settings
            .read_declared(SOURCE, SettingsScope::User, &self.stop)
            .await
        {
            Ok(snapshot) => match serde_json::from_value::<Document>(snapshot.document) {
                Ok(document) => Some(document.max_continuations),
                Err(error) => {
                    tracing::warn!(%error, "Goal settings document is invalid");
                    None
                }
            },
            Err(error) => {
                tracing::warn!(%error, "Goal settings read failed");
                None
            }
        }
    }

    /// Folds this branch's facts through the cutoff of one read, unpublished
    /// tail included, and releases only what lies above it (A.6).
    async fn rebuild(self: &Arc<Self>) -> Result<u64, AgentError> {
        let mut from = None;
        let through;
        loop {
            let reply = self
                .timeline
                .query(read_page(
                    TimelineQueryScope::Branch,
                    from,
                    REBUILD_PAGE,
                    true,
                ))
                .await?;
            for event in &reply.events {
                self.ingest(event, false);
            }
            match reply.events.last().map(|event| event.sequence) {
                Some(last) if (reply.events.len() as u64) >= REBUILD_PAGE => from = Some(last + 1),
                _ => {
                    through = reply.through;
                    break;
                }
            }
        }
        // The decisions the previous generation owed are taken once, before
        // any live event.
        self.decide_end();
        self.settle();
        let start = self.state.lock().unwrap().start;
        if let Some(start) = start {
            self.decide_start(start);
        }
        Ok(through)
    }

    /// Folds one consumed local event and takes the decisions it carries. The
    /// rebuild's facts take decisions only through its closing steps.
    fn ingest(self: &Arc<Self>, event: &AgentEvent, live: bool) {
        let turn_start = self.state.lock().unwrap().fold.turn_start;
        self.state.lock().unwrap().fold.apply(event);
        let sequence = event.sequence;
        if fold::is_opening(&event.kind) {
            self.state.lock().unwrap().start = Some(sequence);
            if live {
                self.decide_start(sequence);
            }
            return;
        }
        if !live {
            return;
        }
        match &event.kind {
            EventKind::BranchCreateFailed(data) => {
                let branch = match data {
                    dim_agent_types::BranchCreateFailedData::Failed { branch, .. } => branch,
                    dim_agent_types::BranchCreateFailedData::Cancelled { branch, .. } => branch,
                };
                let evaluation = self.state.lock().unwrap().fold.evaluation_of(branch);
                if let Some(evaluation) = evaluation {
                    self.evaluation_failed(evaluation);
                }
            }
            EventKind::BranchCreate(_) => self.settle(),
            EventKind::BranchEnded(data) => {
                let evaluation = self.state.lock().unwrap().fold.evaluation_of(&data.branch);
                if let Some(evaluation) = evaluation {
                    let state = self.state.lock().unwrap();
                    if state.fold.is_current(evaluation)
                        && !state.fold.conclusions.contains_key(&evaluation)
                    {
                        drop(state);
                        self.evaluation_failed(evaluation);
                    }
                }
            }
            EventKind::TurnEnd(data) => self.turn_decision(sequence, turn_start, data.clone()),
            EventKind::StopInterrupt(data) if data.scope == InterruptScope::Turn => {
                self.interrupted(sequence)
            }
            EventKind::InputMessage(data) => {
                if let Some(conclusion) = data.blocks.first().and_then(|block| match block {
                    InputBlock::Raw { text, .. } => fold::conclusion_of(text),
                }) {
                    self.concluded(sequence, conclusion);
                }
                self.settle();
            }
            EventKind::InputDeleted(_) => self.decide_end(),
            _ => {}
        }
    }

    /// Clears a goal inherited from another branch, once, on a branch that
    /// has no goal history of its own (§3). The inherited read is owned work:
    /// it waits for activation, runs once per `at` and not after a stop. A
    /// failed read writes nothing and is logged with its key, and the next
    /// load decides the opening again.
    fn decide_start(self: &Arc<Self>, at: u64) {
        let (decided, local_blocks, copied) = {
            let state = self.state.lock().unwrap();
            (
                state.fold.is_decided(at),
                state.fold.local_blocks,
                state.fold.copied_live,
            )
        };
        if decided || local_blocks > 0 {
            return;
        }
        if let Some(live) = copied {
            if live {
                self.started_clear(at);
            }
            return;
        }
        self.task(format!("start:{at}"), move |owner| {
            Box::pin(async move {
                if owner.inherited_live(at).await? {
                    owner.started_clear(at);
                }
                Ok(())
            })
        });
    }

    fn started_clear(self: &Arc<Self>, at: u64) {
        self.task(format!("clear:{at}"), move |owner| {
            Box::pin(async move {
                let write = {
                    let state = owner.state.lock().unwrap();
                    !state.fold.is_decided(at) && state.fold.local_blocks == 0
                };
                if !write {
                    return Ok(());
                }
                owner
                    .tree
                    .input(
                        owner.branch_id.clone(),
                        text::start_clear(&owner.branch_id, at),
                    )
                    .await?;
                Ok(())
            })
        });
    }

    fn interrupted(self: &Arc<Self>, at: u64) {
        self.task(format!("interrupt:{at}"), move |owner| {
            Box::pin(async move {
                let decision = {
                    let state = owner.state.lock().unwrap();
                    fold::decide_interrupt(&state.fold, at)
                };
                if let Some(Decision::Pause { reason }) = decision {
                    owner
                        .tree
                        .input(
                            owner.branch_id.clone(),
                            text::automatic_pause(&owner.branch_id, reason, at),
                        )
                        .await?;
                }
                Ok(())
            })
        });
    }

    /// Whether this branch already committed its `branch/create` for `after`:
    /// from then on the creation records its own outcome, so a failed wait
    /// only means the caller stopped waiting (Pursuit §4.1).
    async fn created(&self, after: u64) -> Result<bool, AgentError> {
        let reply = self
            .timeline
            .query(TimelineQuery {
                kinds: vec!["branch/create".into()],
                predicate: Some(TimelineQueryPredicate {
                    path: "source.operation_id".into(),
                    value: fold::evaluation_operation(after).into(),
                }),
                limit: NonZeroU64::new(1).expect("one fact is a page"),
                ..read_page(TimelineQueryScope::Branch, Some(after), 1, true)
            })
            .await?;
        Ok(!reply.events.is_empty())
    }

    /// The turn-end decision (A.4, Pursuit §4.1): an evaluator creation when
    /// the judge asks for one, otherwise the continuation limit.
    async fn do_turn_end(self: &Arc<Self>, after: u64) -> Result<(), AgentError> {
        let (request, evaluation, objective) = {
            let state = self.state.lock().unwrap();
            let fold = &state.fold;
            if fold.turn_end.map(|end| end.sequence) != Some(after) {
                return Ok(());
            }
            (
                fold.request_in_turn(after)
                    .map(|request| request.summary.clone()),
                fold::decide_evaluation(fold, after),
                fold.objective.clone().unwrap_or_default(),
            )
        };
        if let Some(summary) = request
            && evaluation
        {
            let cancel = self.stop.clone();
            let cancelled: Arc<
                dyn Fn() -> BoxFuture<'static, Result<(), rutis::Error>> + Send + Sync,
            > = Arc::new(move || {
                let cancel = cancel.clone();
                Box::pin(async move {
                    cancel.cancelled().await;
                    Ok(())
                })
            });
            let options = BranchOptions {
                prompt: text::evaluator_prompt(
                    &self.branch_id,
                    &after.to_string(),
                    &objective,
                    &summary,
                ),
                source: BranchSource::Host {
                    operation_id: fold::evaluation_operation(after),
                },
            };
            if self
                .tree
                .create(self.branch_id.clone(), after, options, cancelled)
                .await
                .is_err()
                && !self.stopped()
                && !self.created(after).await?
                && self.state.lock().unwrap().fold.is_current(after)
            {
                self.tree
                    .input(
                        self.branch_id.clone(),
                        text::automatic_pause(&self.branch_id, "evaluation_failed", after),
                    )
                    .await?;
            }
            self.settle();
            return Ok(());
        }
        let Some(limit) = self.limit().await else {
            return Ok(());
        };
        if self.stopped() {
            return Ok(());
        }
        let decision = {
            let state = self.state.lock().unwrap();
            if state.fold.turn_end.map(|end| end.sequence) != Some(after) {
                return Ok(());
            }
            fold::decide_turn_end(&state.fold, limit)
        };
        let Some(decision) = decision else {
            return Ok(());
        };
        let input = match decision {
            Decision::Continuation { count } => {
                text::continuation(&self.branch_id, count, limit, after, &self.texts)
            }
            Decision::Pause { reason } => text::automatic_pause(&self.branch_id, reason, after),
        };
        self.tree.input(self.branch_id.clone(), input).await?;
        Ok(())
    }

    /// The latest `turn/end` without a live decision: an open decision the
    /// rebuild still owes after an input was deleted (§5).
    fn decide_end(self: &Arc<Self>) {
        let end = self.state.lock().unwrap().fold.turn_end;
        let Some(TurnEnd { sequence, .. }) = end else {
            return;
        };
        let decide = {
            let state = self.state.lock().unwrap();
            state.fold.running() && !state.fold.is_decided(sequence)
        };
        if !decide {
            return;
        }
        self.task(format!("turn:{sequence}"), move |owner| {
            Box::pin(async move {
                owner.resolved().await;
                let skip = {
                    let state = owner.state.lock().unwrap();
                    state.role.is_some()
                        || state.fold.turn_end.map(|end| end.sequence) != Some(sequence)
                };
                if owner.stopped() || skip {
                    return Ok(());
                }
                owner.do_turn_end(sequence).await
            })
        });
    }

    /// The decision a live `turn/end` at `sequence` takes: the evaluator
    /// role's report, or the branch's own turn-end decision.
    fn turn_decision(self: &Arc<Self>, sequence: u64, turn_start: u64, end: TurnEndData) {
        self.task(format!("turn:{sequence}"), move |owner| {
            Box::pin(async move {
                owner.resolved().await;
                if owner.stopped() {
                    return Ok(());
                }
                let (role, decide) = {
                    let state = owner.state.lock().unwrap();
                    (
                        state.role.clone(),
                        state.fold.running() && !state.fold.is_decided(sequence),
                    )
                };
                if role.is_some() {
                    return owner.evaluated(sequence, turn_start, &end).await;
                }
                if decide {
                    owner.do_turn_end(sequence).await
                } else {
                    Ok(())
                }
            })
        });
    }

    /// The evaluator line's report for the turn that ended at `sequence`
    /// (Pursuit §4.2, §5).
    async fn evaluated(
        self: &Arc<Self>,
        sequence: u64,
        turn_start: u64,
        end: &TurnEndData,
    ) -> Result<(), AgentError> {
        let role = self.state.lock().unwrap().role.clone();
        let Some(role) = role else {
            return Ok(());
        };
        if self
            .state
            .lock()
            .unwrap()
            .fold
            .turn_end
            .map(|end| end.sequence)
            != Some(sequence)
        {
            return Ok(());
        }
        if !matches!(end.outcome, TurnOutcome::Completed { .. }) {
            return self
                .report(&role, "failed", "the evaluator's turn did not complete")
                .await;
        }
        let text = self.reply(turn_start, sequence, &end.turn_id).await?;
        if let Some((result, body)) = fold::reply_conclusion(&text) {
            return self.report(&role, result_name(result), &body).await;
        }
        if self.state.lock().unwrap().fold.turn_ends == 1 {
            self.tree
                .input(
                    self.branch_id.clone(),
                    text::evaluate_again(&self.branch_id, sequence),
                )
                .await?;
            return Ok(());
        }
        self.report(&role, "continue", &fold::truncate(&text)).await
    }

    /// Writes one conclusion about this evaluator's evaluation to the branch
    /// it reports to.
    async fn report(&self, role: &Role, result: &str, text: &str) -> Result<(), AgentError> {
        self.tree
            .input(
                role.branch.clone(),
                text::conclusion(
                    &role.branch,
                    &self.branch_id,
                    &role.evaluation.to_string(),
                    result,
                    text,
                ),
            )
            .await?;
        Ok(())
    }

    /// The reply of the turn that ended at `through`: the text deltas of the
    /// request its last `step/model-start` names (Pursuit §4.2).
    async fn reply(
        &self,
        turn_start: u64,
        through: u64,
        turn_id: &str,
    ) -> Result<String, AgentError> {
        let mut events = Vec::new();
        let mut from = turn_start;
        loop {
            let page = self
                .timeline
                .query(TimelineQuery {
                    kinds: vec!["step/model-start".into(), "step/text".into()],
                    predicate: Some(TimelineQueryPredicate {
                        path: "turn_id".into(),
                        value: turn_id.into(),
                    }),
                    ..read_page(TimelineQueryScope::Branch, Some(from), REPLY_PAGE, false)
                })
                .await?;
            events.extend(
                page.events
                    .iter()
                    .filter(|event| event.sequence <= through)
                    .cloned(),
            );
            let Some(last) = page.events.last() else {
                break;
            };
            if (page.events.len() as u64) < REPLY_PAGE || last.sequence >= through {
                break;
            }
            from = last.sequence + 1;
        }
        let mut request = None;
        for event in &events {
            if let EventKind::ModelStart(data) = &event.kind
                && data.turn_id == turn_id
            {
                request = Some(data.request_id.clone());
            }
        }
        let Some(request) = request else {
            return Ok(String::new());
        };
        let mut reply = String::new();
        for event in &events {
            if let EventKind::Text(data) = &event.kind
                && data.turn_id == turn_id
                && data.request_id == request
            {
                reply.push_str(&data.delta);
            }
        }
        Ok(reply)
    }

    fn evaluation_failed(self: &Arc<Self>, evaluation: u64) {
        self.task(format!("failed:{evaluation}"), move |owner| {
            Box::pin(async move {
                let current = owner.state.lock().unwrap().fold.is_current(evaluation);
                if !current {
                    return Ok(());
                }
                owner
                    .tree
                    .input(
                        owner.branch_id.clone(),
                        text::automatic_pause(&owner.branch_id, "evaluation_failed", evaluation),
                    )
                    .await?;
                Ok(())
            })
        });
    }

    /// The conclusion the evaluator wrote decides the evaluation it names
    /// (Pursuit §4.1).
    fn concluded(self: &Arc<Self>, sequence: u64, conclusion: Conclusion) {
        self.task(format!("conclusion:{sequence}"), move |owner| {
            Box::pin(async move {
                let action = {
                    let state = owner.state.lock().unwrap();
                    fold::decide_conclusion(&state.fold, sequence, &conclusion)
                };
                let Some(action) = action else {
                    return Ok(());
                };
                let evaluation = conclusion.evaluation;
                let named = evaluation.to_string();
                match action {
                    ConclusionAction::Failed => {
                        owner
                            .tree
                            .input(
                                owner.branch_id.clone(),
                                text::automatic_pause(
                                    &owner.branch_id,
                                    "evaluation_failed",
                                    evaluation,
                                ),
                            )
                            .await?;
                    }
                    ConclusionAction::Completed(text) => {
                        owner
                            .tree
                            .input(
                                owner.branch_id.clone(),
                                text::evaluated(&owner.branch_id, "completed", &named, &text),
                            )
                            .await?;
                    }
                    ConclusionAction::Blocked(text) => {
                        owner
                            .tree
                            .input(
                                owner.branch_id.clone(),
                                text::evaluated(&owner.branch_id, "blocked", &named, &text),
                            )
                            .await?;
                    }
                    ConclusionAction::Continue => {
                        let Some(limit) = owner.limit().await else {
                            return Ok(());
                        };
                        if owner.stopped() {
                            return Ok(());
                        }
                        let decision = {
                            let state = owner.state.lock().unwrap();
                            if fold::decide_conclusion(&state.fold, sequence, &conclusion)
                                != Some(ConclusionAction::Continue)
                            {
                                return Ok(());
                            }
                            fold::decide_evaluated_continuation(&state.fold, limit)
                        };
                        let Some(decision) = decision else {
                            return Ok(());
                        };
                        let input = match decision {
                            Decision::Continuation { count } => text::evaluated_continuation(
                                &owner.branch_id,
                                count,
                                limit,
                                evaluation,
                                &named,
                            ),
                            Decision::Pause { reason } => {
                                text::automatic_pause(&owner.branch_id, reason, evaluation)
                            }
                        };
                        owner.tree.input(owner.branch_id.clone(), input).await?;
                    }
                }
                Ok(())
            })
        });
    }

    /// Ends the evaluator lines no evaluation needs any more (Pursuit §13 E2).
    fn settle(self: &Arc<Self>) {
        let evaluations: Vec<(u64, String)> = {
            let state = self.state.lock().unwrap();
            state
                .fold
                .evaluations
                .iter()
                .map(|(evaluation, branch)| (*evaluation, branch.clone()))
                .collect()
        };
        for (evaluation, evaluator) in evaluations {
            let settled = {
                let state = self.state.lock().unwrap();
                state.fold.ended.contains(&evaluator)
                    || state.fold.failed.contains(&evaluator)
                    || !(state.fold.conclusions.contains_key(&evaluation)
                        || !state.fold.is_current(evaluation))
            };
            if settled {
                continue;
            }
            self.task(format!("settle:{evaluation}"), move |owner| {
                Box::pin(async move {
                    owner.resolved().await;
                    if !owner.session.activated().await {
                        return Ok(());
                    }
                    let skip = {
                        let state = owner.state.lock().unwrap();
                        state.fold.ended.contains(&evaluator)
                            || !(state.fold.conclusions.contains_key(&evaluation)
                                || !state.fold.is_current(evaluation))
                    };
                    if owner.stopped() || skip {
                        return Ok(());
                    }
                    match owner
                        .tree
                        .end(
                            evaluator.clone(),
                            BranchSource::Host {
                                operation_id: fold::evaluation_operation(evaluation),
                            },
                        )
                        .await
                    {
                        Ok(()) | Err(AgentError::BranchEnded(_)) => Ok(()),
                        Err(error) => Err(error),
                    }
                })
            });
        }
    }

    /// The goal an inherited prefix left live, read newest first from the
    /// chain, if the branch has no copied facts to answer it (A11).
    async fn inherited_live(&self, at: u64) -> Result<bool, AgentError> {
        let mut offset = 0;
        loop {
            let mut query = read_page(TimelineQueryScope::Chain, Some(at), INHERITED_PAGE, false);
            query.kinds = vec!["input/message".into()];
            query.direction = TimelineQueryDirection::Descending;
            query.offset = offset;
            let reply = self.timeline.query(query).await?;
            for (index, event) in reply.events.iter().enumerate() {
                if reply.positions[index].branch_id == self.branch_id {
                    continue;
                }
                let EventKind::InputMessage(data) = &event.kind else {
                    continue;
                };
                let mut live = None;
                for block in &data.blocks {
                    let InputBlock::Raw {
                        text: block_text,
                        key,
                        retire,
                    } = block;
                    if text::marker(block_text).is_some()
                        && key.as_deref().is_some_and(|key| {
                            key == text::OBJECTIVE_KEY || key == text::STATUS_KEY
                        })
                    {
                        live = Some(!retire);
                    }
                }
                if let Some(live) = live {
                    return Ok(live);
                }
            }
            if (reply.events.len() as u64) < INHERITED_PAGE {
                return Ok(false);
            }
            offset += reply.events.len() as u64;
        }
    }
}

/// One page of this branch's facts, oldest first from `from` in ascending
/// reads; the direction is the caller's.
fn read_page(
    scope: TimelineQueryScope,
    from: Option<u64>,
    limit: u64,
    include_publishing: bool,
) -> TimelineQuery {
    TimelineQuery {
        scope,
        from_event: None,
        from_sequence: from,
        order_by: TimelineQueryOrderBy::Sequence,
        direction: TimelineQueryDirection::Ascending,
        include_publishing,
        kinds: Vec::new(),
        flags: Vec::new(),
        event_id: None,
        predicate: None,
        limit: NonZeroU64::new(limit).expect("a page keeps at least one fact"),
        offset: 0,
    }
}

fn result_name(result: fold::ConclusionKind) -> &'static str {
    match result {
        fold::ConclusionKind::Done => "done",
        fold::ConclusionKind::Continue => "continue",
        fold::ConclusionKind::Blocked => "blocked",
        fold::ConclusionKind::Failed => "failed",
    }
}
