//! The no-Loop part harness: the real Dispatcher, Timeline,
//! TimelineTree, SessionMethods, ToolRegistry, PromptGuide, storage and
//! Settings, with only the parts Goal needs. The facts the Loop, InputQueue
//! and TimelineTree would write are the test's.
#![allow(dead_code)]
use dim_agent_capabilities::host::ToolExecutionContext;
use dim_agent_capabilities::methods::RpcError;
use dim_agent_capabilities::timeline_tree::TreeCommands;
use dim_agent_capabilities::{
    defaults,
    file_storage::FileStoragePlugin,
    methods::SessionMethods,
    prompt_guide::{GuideEntries, PromptGuidePlugin},
    timeline_tree::{TimelineTreePlugin, TreeTimelinePlugin},
    tool_registry::{ToolRegistry, ToolRegistryPlugin},
};
use dim_agent_runtime::testing::MinimalHost;
use dim_agent_runtime::{
    AgentBuilder, AgentConfig, AgentError, BoxFuture, BranchOptions, Dispatcher, Feature,
    LoopSnapshot, LoopState, MemoryTimelineStore, Names, PublishCallback, Session, SessionConfig,
    StoredBranch, StoredEvent, Timeline, TimelineIdentity, TimelineKey, TimelineQuery,
    TimelineQueryPage, TimelineStore, TimelineTrace,
};
use dim_agent_types::{
    AgentEvent, BranchSource, DeltaData, EventKind, InputBlock, InputMessageData, InputMode,
    StepData, ToolCall, ToolDefinition, ToolResult,
};
use dim_app_server::settings::{SettingsPlugin, SettingsService};
use rutis::Ctx;
use rutis::host::{CtxHost, DynPlugin, Effect, Key, TypeKey};
use rutis_loader::resolve::Build;
use serde_json::Value;
use std::{
    collections::HashMap,
    ops::RangeInclusive,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

/// The branch facts the test appends, and the services the assertions read.
#[derive(Default)]
pub struct Probe {
    timelines: Mutex<HashMap<String, Arc<Timeline>>>,
    commands: Mutex<HashMap<String, Arc<TreeCommands>>>,
    dispatchers: Mutex<HashMap<String, Arc<Dispatcher>>>,
    tools: Mutex<Option<Arc<ToolRegistry>>>,
    guide: Mutex<Option<Arc<GuideEntries>>>,
    store: Mutex<Option<Arc<dyn TimelineStore>>>,
}

impl Probe {
    pub fn timeline(&self, branch: &str) -> Arc<Timeline> {
        self.timelines
            .lock()
            .unwrap()
            .get(branch)
            .cloned()
            .unwrap_or_else(|| panic!("branch {branch} has no Timeline"))
    }
    pub fn has(&self, branch: &str) -> bool {
        self.timelines.lock().unwrap().contains_key(branch)
    }
    pub fn commands(&self, branch: &str) -> Arc<TreeCommands> {
        self.commands
            .lock()
            .unwrap()
            .get(branch)
            .cloned()
            .unwrap_or_else(|| panic!("branch {branch} has no TreeCommands"))
    }
    pub fn dispatcher(&self, branch: &str) -> Arc<Dispatcher> {
        self.dispatchers
            .lock()
            .unwrap()
            .get(branch)
            .cloned()
            .unwrap_or_else(|| panic!("branch {branch} has no Dispatcher"))
    }
    pub fn tools(&self) -> Arc<ToolRegistry> {
        self.tools.lock().unwrap().clone().expect("ToolRegistry")
    }
    pub fn guide(&self) -> Arc<GuideEntries> {
        self.guide.lock().unwrap().clone().expect("GuideEntries")
    }
    pub fn store(&self) -> Arc<dyn TimelineStore> {
        self.store.lock().unwrap().clone().expect("TimelineStore")
    }
}

struct ProbeSession {
    probe: Arc<Probe>,
    tools: Key<ToolRegistry>,
    guide: Key<GuideEntries>,
    store: Key<dyn TimelineStore>,
}
impl ProbeSession {
    fn new(names: &Names<'_>, probe: Arc<Probe>) -> rutis::Result<Self> {
        Ok(Self {
            probe,
            tools: names.key()?,
            guide: names.key()?,
            store: names.key()?,
        })
    }
}
impl DynPlugin for ProbeSession {
    fn name(&self) -> &str {
        "GoalProbeSession"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![self.tools.erase(), self.guide.erase(), self.store.erase()]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let tools = ctx.require_at(&self.tools)?;
            let guide = ctx.require_at(&self.guide)?;
            let store = ctx.require_at(&self.store)?;
            *self.probe.tools.lock().unwrap() = Some(tools);
            *self.probe.guide.lock().unwrap() = Some(guide);
            *self.probe.store.lock().unwrap() = Some(store);
            Ok(Effect::Done)
        })
    }
}

struct ProbeBranch {
    probe: Arc<Probe>,
    timeline: Key<Timeline>,
    commands: Key<TreeCommands>,
    dispatcher: Key<Dispatcher>,
    identity: Key<TimelineIdentity>,
}
impl ProbeBranch {
    fn new(names: &Names<'_>, probe: Arc<Probe>) -> rutis::Result<Self> {
        Ok(Self {
            probe,
            timeline: names.key()?,
            commands: names.key()?,
            dispatcher: names.key()?,
            identity: names.key()?,
        })
    }
}
impl DynPlugin for ProbeBranch {
    fn name(&self) -> &str {
        "GoalProbeBranch"
    }
    fn injects(&self) -> Vec<TypeKey> {
        vec![
            self.timeline.erase(),
            self.commands.erase(),
            self.dispatcher.erase(),
            self.identity.erase(),
        ]
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            let timeline = ctx.require_at(&self.timeline)?;
            let commands = ctx.require_at(&self.commands)?;
            let dispatcher = ctx.require_at(&self.dispatcher)?;
            let identity = ctx.require_at(&self.identity)?;
            let mut timelines = self.probe.timelines.lock().unwrap();
            timelines.insert(identity.branch_id.clone(), timeline);
            self.probe
                .commands
                .lock()
                .unwrap()
                .insert(identity.branch_id.clone(), commands);
            self.probe
                .dispatchers
                .lock()
                .unwrap()
                .insert(identity.branch_id.clone(), dispatcher);
            Ok(Effect::Done)
        })
    }
}

/// Every branch must provide the Loop's state, which Goal never reads. The
/// harness supplies an idle state instead of the AgentLoop, so no turn is
/// driven and the test owns every fact.
struct IdleLoop(tokio::sync::watch::Sender<LoopSnapshot>);
impl LoopState for IdleLoop {
    fn state(&self) -> LoopSnapshot {
        LoopSnapshot::default()
    }
    fn watch(&self) -> tokio::sync::watch::Receiver<LoopSnapshot> {
        self.0.subscribe()
    }
}

struct LoopStub {
    key: Key<dyn LoopState>,
    watch: tokio::sync::watch::Sender<LoopSnapshot>,
}
impl LoopStub {
    fn new(names: &Names<'_>) -> rutis::Result<Self> {
        Ok(Self {
            key: names.key()?,
            watch: tokio::sync::watch::channel(LoopSnapshot::default()).0,
        })
    }
}
impl DynPlugin for LoopStub {
    fn name(&self) -> &str {
        "GoalLoopStub"
    }
    fn injects(&self) -> Vec<TypeKey> {
        Vec::new()
    }
    fn apply<'a>(&'a self, ctx: &'a Ctx) -> BoxFuture<'a, rutis::Result<Effect>> {
        Box::pin(async move {
            ctx.provide_at(
                self.key.clone(),
                Arc::new(IdleLoop(self.watch.clone())) as Arc<dyn LoopState>,
            )?;
            Ok(Effect::Done)
        })
    }
}

/// A Session record being filled with facts, branch by branch, before a
/// restore loads it.
pub struct Seed {
    root: Ctx,
    store: Arc<dyn TimelineStore>,
    session: String,
    timelines: HashMap<String, Arc<Timeline>>,
}

impl Seed {
    /// Commits `kind` on `branch` and returns the fact, so a case can use its
    /// id or sequence in a later fact of the same history.
    pub fn append(&mut self, branch: &str, kind: EventKind) -> Arc<AgentEvent> {
        let timeline = self.timelines.entry(branch.to_owned()).or_insert_with(|| {
            Arc::new(Timeline::new(
                self.root.clone(),
                TimelineKey {
                    session_id: self.session.clone(),
                    branch_id: branch.to_owned(),
                },
                vec![],
                self.store.clone(),
            ))
        });
        timeline
            .append(kind.into())
            .unwrap_or_else(|error| panic!("seeding {branch}: {error}"))
    }

    /// Closes the seeding root; the facts stay in the store.
    pub async fn finish(self) {
        let _ = self.root.shutdown().await;
    }
}

/// A store that can refuse [`TimelineStore::query`] for one branch while
/// armed, returning the storage error a real store returns for a failed
/// read; every other call delegates to a memory store.
#[derive(Default)]
pub struct RefusingStore {
    memory: MemoryTimelineStore,
    armed: AtomicBool,
    branch: Mutex<Option<String>>,
    refused: AtomicUsize,
}

impl RefusingStore {
    /// Refuses queries on `branch` until [`Self::disarm`].
    pub fn arm(&self, branch: &str) {
        *self.branch.lock().unwrap() = Some(branch.to_owned());
        self.armed.store(true, Ordering::SeqCst);
    }

    pub fn disarm(&self) {
        self.armed.store(false, Ordering::SeqCst);
    }

    /// Queries refused since the store was created.
    pub fn refused(&self) -> usize {
        self.refused.load(Ordering::SeqCst)
    }
}

impl TimelineStore for RefusingStore {
    fn create_session<'a>(
        &'a self,
        session_id: &'a str,
        config: &'a SessionConfig,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.create_session(session_id, config)
    }
    fn rollback_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.rollback_session(session_id)
    }
    fn release_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.release_session(session_id)
    }
    fn session_config<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Option<SessionConfig>>> {
        self.memory.session_config(session_id)
    }
    fn branches<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Vec<StoredBranch>>> {
        self.memory.branches(session_id)
    }
    fn head<'a>(&'a self, key: &'a TimelineKey) -> BoxFuture<'a, dim_agent_runtime::Result<u64>> {
        self.memory.head(key)
    }
    fn append(
        &self,
        key: TimelineKey,
        events: Vec<StoredEvent>,
        publish: PublishCallback,
    ) -> dim_agent_runtime::Result<()> {
        self.memory.append(key, events, publish)
    }
    fn batch_end<'a>(
        &'a self,
        key: &'a TimelineKey,
        sequence: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.batch_end(key, sequence)
    }
    fn query<'a>(
        &'a self,
        key: &'a TimelineKey,
        query: &'a TimelineQuery,
        from: u64,
        through: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<TimelineQueryPage>> {
        // The branch command also reads the parent; only the inherited goal
        // read has this shape (input messages, newest first).
        let refused = self.armed.load(Ordering::SeqCst)
            && self.branch.lock().unwrap().as_deref() == Some(key.branch_id.as_str())
            && query.kinds == ["input/message".to_owned()]
            && query.direction == dim_agent_runtime::TimelineQueryDirection::Descending;
        if refused {
            self.refused.fetch_add(1, Ordering::SeqCst);
            return Box::pin(async move {
                Err(AgentError::Storage("the inherited read was refused".into()))
            });
        }
        self.memory.query(key, query, from, through)
    }
    fn read_segments<'a>(
        &'a self,
        session_id: &'a str,
        segments: Vec<(String, RangeInclusive<u64>)>,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Vec<Vec<StoredEvent>>>> {
        self.memory.read_segments(session_id, segments)
    }
    fn trace<'a>(
        &'a self,
        key: &'a TimelineKey,
        sequence: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Option<TimelineTrace>>> {
        self.memory.trace(key, sequence)
    }
}

/// A store that holds Goal's creation check — its one `branch/create` query —
/// until the case lets it go, so a case can commit a change and have the fold
/// consume it while that check waits; every other call delegates to a memory
/// store.
#[derive(Default)]
pub struct GatedStore {
    memory: MemoryTimelineStore,
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

impl GatedStore {
    /// The case's ends of the gate: it waits on the first until the creation
    /// check runs, then writes its change and lets the second go once the
    /// Dispatcher consumed it.
    pub fn gate(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let (release, held) = tokio::sync::oneshot::channel();
        *self.entered.lock().unwrap() = Some(entered);
        *self.release.lock().unwrap() = Some(held);
        (waiting, release)
    }
}

impl TimelineStore for GatedStore {
    fn create_session<'a>(
        &'a self,
        session_id: &'a str,
        config: &'a SessionConfig,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.create_session(session_id, config)
    }
    fn rollback_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.rollback_session(session_id)
    }
    fn release_session<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.release_session(session_id)
    }
    fn session_config<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Option<SessionConfig>>> {
        self.memory.session_config(session_id)
    }
    fn branches<'a>(
        &'a self,
        session_id: &'a str,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Vec<StoredBranch>>> {
        self.memory.branches(session_id)
    }
    fn head<'a>(&'a self, key: &'a TimelineKey) -> BoxFuture<'a, dim_agent_runtime::Result<u64>> {
        self.memory.head(key)
    }
    fn append(
        &self,
        key: TimelineKey,
        events: Vec<StoredEvent>,
        publish: PublishCallback,
    ) -> dim_agent_runtime::Result<()> {
        self.memory.append(key, events, publish)
    }
    fn batch_end<'a>(
        &'a self,
        key: &'a TimelineKey,
        sequence: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<()>> {
        self.memory.batch_end(key, sequence)
    }
    fn query<'a>(
        &'a self,
        key: &'a TimelineKey,
        query: &'a TimelineQuery,
        from: u64,
        through: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<TimelineQueryPage>> {
        let creation_check = query.kinds == ["branch/create".to_owned()]
            && query
                .predicate
                .as_ref()
                .is_some_and(|predicate| predicate.path == "source.operation_id");
        let armed = creation_check
            .then(|| {
                (
                    self.entered.lock().unwrap().take(),
                    self.release.lock().unwrap().take(),
                )
            })
            .and_then(|(entered, release)| entered.zip(release));
        let Some((entered, release)) = armed else {
            return self.memory.query(key, query, from, through);
        };
        Box::pin(async move {
            let _ = entered.send(());
            let _ = release.await;
            self.memory.query(key, query, from, through).await
        })
    }
    fn read_segments<'a>(
        &'a self,
        session_id: &'a str,
        segments: Vec<(String, RangeInclusive<u64>)>,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Vec<Vec<StoredEvent>>>> {
        self.memory.read_segments(session_id, segments)
    }
    fn trace<'a>(
        &'a self,
        key: &'a TimelineKey,
        sequence: u64,
    ) -> BoxFuture<'a, dim_agent_runtime::Result<Option<TimelineTrace>>> {
        self.memory.trace(key, sequence)
    }
}

/// One Session over the part harness, and the handles a case needs.
pub struct Env {
    host: MinimalHost,
    pub session: Session,
    pub probe: Arc<Probe>,
    home: PathBuf,
    refusing: Option<Arc<RefusingStore>>,
    gated: Option<Arc<GatedStore>>,
}

impl Env {
    pub async fn start() -> Self {
        Self::start_inner(None, None).await
    }

    /// [`Self::start`] on a store that can refuse one branch's queries.
    pub async fn start_refusing() -> Self {
        Self::start_inner(Some(Arc::new(RefusingStore::default())), None).await
    }

    /// [`Self::start`] on a store that can hold Goal's creation check.
    pub async fn start_gated() -> Self {
        Self::start_inner(None, Some(Arc::new(GatedStore::default()))).await
    }

    pub fn refusing(&self) -> &RefusingStore {
        self.refusing.as_ref().expect("a refusing store")
    }

    pub fn gated(&self) -> &GatedStore {
        self.gated.as_ref().expect("a gated store")
    }

    async fn start_inner(
        refusing: Option<Arc<RefusingStore>>,
        gated: Option<Arc<GatedStore>>,
    ) -> Self {
        let home = home();
        let probe = Arc::new(Probe::default());
        let mut builder = AgentBuilder::new(AgentConfig::default());
        dim_goal::install(&mut builder);
        dim_agent_capabilities::register_names(&mut builder);
        dim_agent_runtime::names::register::<SettingsService>(builder.catalog_mut(), None);
        builder
            .require_feature(Feature::new("SessionMethods").session(
                "SessionMethods",
                dim_agent_capabilities::methods::SessionMethodsPlugin::part,
            ))
            .require_feature(
                Feature::new("ToolRegistry").session("ToolRegistry", |names| {
                    Ok(Box::new(ToolRegistryPlugin::new(names)?))
                }),
            )
            .require_feature(Feature::new("PromptGuide").session("PromptGuide", |names| {
                Ok(Box::new(PromptGuidePlugin::new(names)?))
            }))
            .require_feature(
                Feature::new("TimelineTree")
                    .session("TimelineTree", |names| {
                        Ok(Box::new(TimelineTreePlugin::new(names)?))
                    })
                    .branch("TreeTimeline", |names| {
                        Ok(Box::new(TreeTimelinePlugin::new(names)?))
                    }),
            )
            .require_part(dim_goal::SESSION_PART)
            .require_part(dim_goal::BRANCH_PART)
            .require_feature(
                Feature::new("GoalLoop")
                    .branch("GoalLoop", |names| Ok(Box::new(LoopStub::new(names)?))),
            );
        let session_probe = probe.clone();
        builder.require_feature(
            Feature::new("GoalProbeSession").session("GoalProbeSession", move |names| {
                Ok(Box::new(ProbeSession::new(names, session_probe.clone())?))
            }),
        );
        let branch_probe = probe.clone();
        builder.require_feature(
            Feature::new("GoalProbeBranch").branch("GoalProbeBranch", move |names| {
                Ok(Box::new(ProbeBranch::new(names, branch_probe.clone())?))
            }),
        );
        defaults::tool_host(
            &mut builder,
            Arc::new(dim_agent_capabilities::adapters::MockTools),
        );
        if let Some(store) = &refusing {
            builder.timeline_store(store.clone());
        }
        if let Some(store) = &gated {
            builder.timeline_store(store.clone());
        }
        let catalog = builder.catalog().clone();
        let build = Build::default();
        let names = Names::copy(&catalog, &build);
        let settings = SettingsPlugin::new(&names, 0, vec![]).expect("settings plugin");
        let files = home.clone();
        let host = MinimalHost::start_with(&builder, move |root, _| {
            root.plugin(FileStoragePlugin::new(vec![files.clone()]));
            root.plugin(settings.clone());
            Ok(())
        })
        .await
        .expect("the part host assembles");
        let session = host
            .agent()
            .create_session(dim_agent_runtime::SessionConfig {
                cwd: home.to_string_lossy().into_owned(),
                tools: Some(vec!["goal_update".into(), "wait".into()]),
                ..Default::default()
            })
            .await
            .expect("the Session builds");
        let env = Self {
            host,
            session,
            probe,
            home,
            refusing,
            gated,
        };
        env.await_parts().await;
        env
    }

    pub fn home(&self) -> &std::path::Path {
        &self.home
    }

    /// The Settings document the Session reads `maxContinuations` from.
    pub fn set_limit(&self, limit: u64) {
        std::fs::write(
            self.home.join("goal.json"),
            format!("{{\"maxContinuations\":{limit}}}"),
        )
        .unwrap();
    }

    pub fn timeline(&self, branch: &str) -> Arc<Timeline> {
        self.probe.timeline(branch)
    }

    /// Commits one fact on `branch`, as the runtime would.
    pub fn emit(&self, branch: &str, kind: EventKind) -> Arc<AgentEvent> {
        self.timeline(branch)
            .append(kind.into())
            .unwrap_or_else(|error| panic!("append on {branch}: {error}"))
    }

    pub fn dispatcher(&self, branch: &str) -> Arc<Dispatcher> {
        self.probe.dispatcher(branch)
    }

    /// Waits until the branch's Dispatcher delivered `sequence` to every
    /// subscriber: the fold has consumed the fact.
    pub async fn dispatched(&self, branch: &str, sequence: u64) {
        let dispatcher = self.dispatcher(branch);
        tokio::time::timeout(Duration::from_secs(15), async {
            while dispatcher.sequence() < sequence {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("the Dispatcher consumes the change");
    }

    /// One `session.goal.*` call through the assembled SessionMethods.
    pub async fn goal(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let name = format!("session.goal.{method}");
        let methods = self
            .session
            .session_service::<SessionMethods>()
            .expect("SessionMethods");
        let provider = methods.provider(&name).expect("the Goal method");
        let accepted = provider
            .accept(name, params, CancellationToken::new(), |_| {})
            .expect("the method accepts");
        accepted.reply.await.expect("the method replies")
    }

    /// Opens a real child branch of `parent` at its head, with `prompt` and
    /// `source`, as the branch tool or Goal's own creation does.
    pub async fn open_branch(&self, parent: &str, prompt: &str, source: BranchSource) -> String {
        let at = self.timeline(parent).sequence();
        let command = self.probe.commands(parent);
        let branch = command
            .branch(
                parent.to_owned(),
                at,
                BranchOptions {
                    prompt: prompt.into(),
                    source,
                },
                CancellationToken::new(),
            )
            .await
            .expect("the branch opens");
        let id = branch.id().to_owned();
        tokio::time::timeout(Duration::from_secs(15), async {
            while !self.probe.has(&id) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the branch's parts activate");
        id
    }

    /// Ends `id` with `source`, as another line or the host does.
    pub async fn end_branch(&self, id: &str, source: BranchSource) {
        self.probe
            .commands("main")
            .end(id.to_owned(), source)
            .await
            .expect("the branch ends");
    }

    /// Closes the Session and restores it from its store: every part loads
    /// again over the history it left, as a process restart does.
    pub async fn restore(self) -> Self {
        let Self {
            host,
            session,
            probe,
            home,
            refusing,
            gated,
        } = self;
        let id = session.id().to_owned();
        session.close().await.expect("the Session closes");
        let session = host
            .agent()
            .restore_session(id)
            .await
            .expect("the Session restores");
        let env = Self {
            host,
            session,
            probe,
            home,
            refusing,
            gated,
        };
        env.await_parts().await;
        env
    }

    /// The Session id the harness created.
    pub fn session_id(&self) -> &str {
        self.session.id()
    }

    /// Starts a second Session record `id` in the shared store, to be filled
    /// before anything loads it: the history a restore replays.
    pub async fn seed_new(&self, id: &str) -> Seed {
        let store = self.probe.store();
        let config = SessionConfig {
            cwd: self.home.to_string_lossy().into_owned(),
            tools: Some(vec!["goal_update".into(), "wait".into()]),
            ..Default::default()
        };
        store
            .create_session(id, &config)
            .await
            .expect("the seeded session record");
        Seed {
            root: Ctx::root().expect("a seeding root"),
            store,
            session: id.to_owned(),
            timelines: HashMap::new(),
        }
    }

    /// Closes this Session and restores the seeded Session `id` in its place.
    pub async fn restore_seeded(self, id: &str) -> Self {
        let Self {
            host,
            session,
            probe,
            home,
            refusing,
            gated,
        } = self;
        session.close().await.expect("the Session closes");
        let session = host
            .agent()
            .restore_session(id.to_owned())
            .await
            .expect("the seeded Session restores");
        let env = Self {
            host,
            session,
            probe,
            home,
            refusing,
            gated,
        };
        env.await_parts().await;
        env
    }

    /// Waits until the Session's parts are mounted and Goal's methods landed.
    async fn await_parts(&self) {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                if self.probe.has("main") && self.probe.tools.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("main and its parts activate");
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let methods = self.session.session_service::<SessionMethods>();
                if methods
                    .as_ref()
                    .is_ok_and(|methods| methods.provider("session.goal.set").is_some())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session.goal.* registers");
    }

    /// Whether the harness has `id`'s Timeline: its parts are mounted.
    pub fn has_branch(&self, id: &str) -> bool {
        self.probe.has(id)
    }

    /// The input message on `branch` whose first block contains `needle`.
    pub async fn written_input(&self, branch: &str, needle: &str) -> InputMessageData {
        self.all_events(branch)
            .await
            .into_iter()
            .find_map(|event| match &event.kind {
                EventKind::InputMessage(data)
                    if data
                        .blocks
                        .first()
                        .is_some_and(|block| raw_text(block).contains(needle)) =>
                {
                    Some(data.clone())
                }
                _ => None,
            })
            .expect("the input")
    }

    /// The `branch/create` facts on `branch`, oldest first.
    pub async fn creates(&self, branch: &str) -> Vec<dim_agent_types::BranchCreateData> {
        self.all_events(branch)
            .await
            .into_iter()
            .filter_map(|event| match &event.kind {
                EventKind::BranchCreate(data) => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `branch/ended` facts on `branch`, oldest first.
    pub async fn ended(&self, branch: &str) -> Vec<dim_agent_types::BranchEndedData> {
        self.all_events(branch)
            .await
            .into_iter()
            .filter_map(|event| match &event.kind {
                EventKind::BranchEnded(data) => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    /// The `goal_update` tool through the assembled ToolRegistry.
    pub async fn tool(&self, branch: &str, input: Value) -> ToolResult {
        self.probe
            .tools()
            .contributed("goal_update")
            .expect("goal_update is registered")
            .handler
            .execute(
                ToolCall {
                    id: "goal-call".into(),
                    name: "goal_update".into(),
                    input: input.as_object().cloned().unwrap_or_default(),
                },
                ToolExecutionContext {
                    turn_id: "turn".into(),
                    request_id: "request".into(),
                    branch_id: branch.into(),
                },
                CancellationToken::new(),
            )
            .await
            .expect("the tool runs")
    }

    /// The tool declarations the model would see.
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.probe.tools().definitions()
    }

    /// The guide entry composition for `branch`: what the model's System
    /// carries.
    pub fn guide(&self, branch: &str) -> String {
        self.probe.guide().compose(branch)
    }

    /// The Goal inputs on `branch`, in order.
    pub async fn writes(&self, branch: &str) -> Vec<Block> {
        self.all_events(branch)
            .await
            .into_iter()
            .filter_map(|event| match &event.kind {
                EventKind::InputMessage(data) => data.blocks.first().and_then(|block| {
                    let InputBlock::Raw { text, .. } = block;
                    text.starts_with("<dim-goal ").then(|| Block {
                        id: event.id.clone(),
                        sequence: event.sequence,
                        marker: text.lines().next().unwrap().to_owned(),
                        text: text.clone(),
                        blocks: data.blocks.iter().map(raw_text).collect(),
                        mode: data.effective_mode(),
                    })
                }),
                _ => None,
            })
            .collect()
    }

    /// Waits until a Goal write on `branch` names `needle`; returns it.
    pub async fn wait_for(&self, branch: &str, needle: &str) -> Block {
        let mut last = Vec::new();
        let found = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                last = self.writes(branch).await;
                if let Some(block) = last.iter().find(|block| block.text.contains(needle)) {
                    return block.clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        match found {
            Ok(block) => block,
            Err(_) => {
                let texts: Vec<&str> = last.iter().map(|block| block.marker.as_str()).collect();
                panic!("no Goal write with {needle} on {branch}; writes: {texts:?}")
            }
        }
    }

    /// The inputs on `branch` the Loop has neither consumed nor the operator
    /// deleted: what a turn takes in, Append mode aside.
    pub async fn pending_inputs(&self, branch: &str) -> Vec<String> {
        use std::collections::HashSet;
        let events = self.all_events(branch).await;
        let consumed: HashSet<&str> = events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::InputConsumed(data) => Some(data.input_id.as_str()),
                _ => None,
            })
            .collect();
        let deleted: HashSet<&str> = events
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::InputDeleted(data) => Some(data.input_id.as_str()),
                _ => None,
            })
            .collect();
        events
            .iter()
            .filter(|event| {
                matches!(&event.kind, EventKind::InputMessage(data) if data.effective_mode() != InputMode::Append)
                    && !consumed.contains(event.id.as_str())
                    && !deleted.contains(event.id.as_str())
            })
            .map(|event| event.id.clone())
            .collect()
    }

    /// Every fact on `branch`, oldest first, read from the store so an ended
    /// line stays readable at its fixed head.
    pub async fn all_events(&self, branch: &str) -> Vec<Arc<AgentEvent>> {
        let store = self.probe.store();
        let key = TimelineKey {
            session_id: self.session.id().to_owned(),
            branch_id: branch.to_owned(),
        };
        let head = store.head(&key).await.expect("the branch's head");
        if head == 0 {
            return Vec::new();
        }
        store
            .read(&key, 1..=head)
            .await
            .expect("the branch's facts")
            .into_iter()
            .map(|record| record.event)
            .collect()
    }

    /// The Goal writes after the branch has been quiet for a while: the way a
    /// case asserts that a decision wrote nothing.
    pub async fn settled(&self, branch: &str) -> Vec<Block> {
        let mut last = Vec::new();
        let mut stable = 0;
        for _ in 0..600 {
            let writes = self.writes(branch).await;
            if writes.len() == last.len() {
                stable += 1;
                if stable >= 10 {
                    return writes;
                }
            } else {
                stable = 0;
                last = writes;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        last
    }

    /// `turn/start`, one `input/consumed` per input, `turn/end completed`:
    /// what the Loop writes around a turn.
    pub fn turn(&self, branch: &str, inputs: &[&Block]) -> Arc<AgentEvent> {
        let ids: Vec<&str> = inputs.iter().map(|input| input.id.as_str()).collect();
        self.turn_ids(branch, &ids)
    }

    pub fn turn_ids(&self, branch: &str, inputs: &[&str]) -> Arc<AgentEvent> {
        self.turn_with_ids(branch, inputs, "completed")
    }

    pub fn turn_with(&self, branch: &str, inputs: &[&Block], status: &str) -> Arc<AgentEvent> {
        let ids: Vec<&str> = inputs.iter().map(|input| input.id.as_str()).collect();
        self.turn_with_ids(branch, &ids, status)
    }

    pub fn turn_with_ids(&self, branch: &str, inputs: &[&str], status: &str) -> Arc<AgentEvent> {
        let turn_id = format!("turn-{}", uuid::Uuid::new_v4());
        self.turn_steps(branch, &turn_id, inputs, status, &[])
    }

    /// `turn/start`, one `input/consumed` per input, each request's
    /// `step/model-start` and text deltas, then `turn/end`: what the Loop
    /// writes for the turn `turn_id`.
    pub fn turn_steps(
        &self,
        branch: &str,
        turn_id: &str,
        inputs: &[&str],
        status: &str,
        steps: &[(&str, &[&str])],
    ) -> Arc<AgentEvent> {
        self.emit(
            branch,
            EventKind::TurnStart(dim_agent_types::TurnStartData {
                turn_id: turn_id.to_owned(),
            }),
        );
        for input in inputs {
            self.emit(
                branch,
                EventKind::InputConsumed(dim_agent_types::InputConsumedData {
                    input_id: (*input).to_owned(),
                }),
            );
        }
        for (request, deltas) in steps {
            self.emit(
                branch,
                EventKind::ModelStart(StepData {
                    turn_id: turn_id.to_owned(),
                    request_id: (*request).to_owned(),
                }),
            );
            for delta in *deltas {
                self.emit(
                    branch,
                    EventKind::Text(DeltaData {
                        turn_id: turn_id.to_owned(),
                        request_id: (*request).to_owned(),
                        delta: (*delta).to_owned(),
                    }),
                );
            }
        }
        self.emit(branch, turn_end_with(turn_id, status))
    }

    /// A turn that has not ended yet: `turn/start`.
    pub fn turn_start(&self, branch: &str, turn_id: &str) -> Arc<AgentEvent> {
        self.emit(
            branch,
            EventKind::TurnStart(dim_agent_types::TurnStartData {
                turn_id: turn_id.to_owned(),
            }),
        )
    }

    /// `input/consumed`: the Loop took `input` into the open turn.
    pub fn consume(&self, branch: &str, input: &str) -> Arc<AgentEvent> {
        self.emit(
            branch,
            EventKind::InputConsumed(dim_agent_types::InputConsumedData {
                input_id: input.to_owned(),
            }),
        )
    }

    /// `turn/end` for the open `turn_id`.
    pub fn end_turn(&self, branch: &str, turn_id: &str, status: &str) -> Arc<AgentEvent> {
        self.emit(branch, turn_end_with(turn_id, status))
    }

    /// An input message of literal blocks, committed on `branch`.
    pub fn write_message(
        &self,
        branch: &str,
        blocks: Vec<InputBlock>,
        mode: InputMode,
    ) -> Arc<AgentEvent> {
        self.emit(branch, message(blocks, mode))
    }

    pub fn interrupt(&self, branch: &str) -> Arc<AgentEvent> {
        self.emit(
            branch,
            EventKind::StopInterrupt(dim_agent_types::StopInterruptData {
                reason: "user".into(),
                scope: dim_agent_types::InterruptScope::Turn,
            }),
        )
    }

    pub fn delete(&self, branch: &str, input: &Block) {
        self.delete_id(branch, &input.id);
    }

    /// `input/deleted` for an input given by id.
    pub fn delete_id(&self, branch: &str, input: &str) {
        self.emit(
            branch,
            EventKind::InputDeleted(dim_agent_types::InputDeletedData {
                input_id: input.to_owned(),
            }),
        );
    }

    /// A `set` through the Session method, waited for in the log.
    pub async fn set(&self, objective: &str) -> Block {
        self.goal("set", serde_json::json!({"objective": objective}))
            .await
            .expect("set writes");
        self.wait_for("main", r#"op="set""#).await
    }

    pub async fn close(self) {
        self.host.close().await.unwrap();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

/// The `turn/end` fact a case writes.
pub fn turn_end(status: &str) -> EventKind {
    turn_end_with(&format!("turn-{}", uuid::Uuid::new_v4()), status)
}

/// The `turn/end` fact of `turn_id` with `status`.
pub fn turn_end_with(turn_id: &str, status: &str) -> EventKind {
    let outcome = match status {
        "completed" => dim_agent_types::TurnOutcome::Completed {
            reason: "stop".into(),
        },
        "failed" => dim_agent_types::TurnOutcome::Failed {
            error: "failed".into(),
        },
        "cancelled" => dim_agent_types::TurnOutcome::Cancelled {
            reason: "cancelled".into(),
        },
        other => panic!("unknown turn status {other}"),
    };
    EventKind::TurnEnd(dim_agent_types::TurnEndData {
        turn_id: turn_id.to_owned(),
        outcome,
    })
}

/// One input message of literal blocks, the way a restored or foreign writer
/// leaves one.
pub fn message(blocks: Vec<InputBlock>, mode: InputMode) -> EventKind {
    EventKind::InputMessage(InputMessageData {
        submission_id: Some(format!("test:{}", uuid::Uuid::new_v4())),
        user: None,
        blocks,
        mode: Some(mode),
    })
}

/// A raw block with literal text.
pub fn raw(text: &str, key: Option<&str>, retire: bool) -> InputBlock {
    InputBlock::Raw {
        text: text.into(),
        key: key.map(str::to_owned),
        retire,
    }
}

/// A user input, the way the operator writes one.
pub fn user(text: &str, mode: InputMode) -> EventKind {
    EventKind::InputMessage(InputMessageData {
        user: Some(dim_agent_types::UserInput {
            content: vec![dim_agent_types::ContentBlock::Text { text: text.into() }],
        }),
        mode: Some(mode),
        ..Default::default()
    })
}

/// A user input carrying extra blocks, the way another plugin writes one.
pub fn user_with_blocks(text: &str, mode: InputMode, blocks: Vec<InputBlock>) -> EventKind {
    EventKind::InputMessage(InputMessageData {
        user: Some(dim_agent_types::UserInput {
            content: vec![dim_agent_types::ContentBlock::Text { text: text.into() }],
        }),
        blocks,
        mode: Some(mode),
        ..Default::default()
    })
}

/// One Goal input the branch holds.
#[derive(Debug, Clone)]
pub struct Block {
    pub id: String,
    pub sequence: u64,
    /// The `<dim-goal ...>` marker line.
    pub marker: String,
    pub text: String,
    /// Every block of the input, in order.
    pub blocks: Vec<String>,
    pub mode: InputMode,
}

impl Block {
    /// The decoded value after `label`, up to the block's last blank line.
    pub fn value(&self, label: &str) -> String {
        let start = self.text.find(&format!("\n{label}\n")).unwrap() + label.len() + 2;
        let end = self.text.rfind("\n\n").unwrap();
        unescape(&self.text[start..end])
    }
    /// The decoded body after `label`, up to the closing marker.
    pub fn body(&self, label: &str) -> String {
        let start = self.text.find(&format!("\n{label}\n")).unwrap() + label.len() + 2;
        let end = self.text.rfind("\n</dim-goal>").unwrap();
        unescape(&self.text[start..end])
    }
}

fn raw_text(block: &InputBlock) -> String {
    let InputBlock::Raw { text, .. } = block;
    text.clone()
}

pub fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn home() -> PathBuf {
    let home = std::env::temp_dir().join(format!("dim-goal-parts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    // FileStorage opens every path component with O_NOFOLLOW, and on macOS the
    // temp directory sits below `/var`, a symbolic link.
    let home = home.canonicalize().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    home
}
