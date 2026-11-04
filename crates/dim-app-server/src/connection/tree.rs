use super::*;
use crate::tree_protocol as wire;
use dim_agent_capabilities::input_queue::{
    CheckInterrupted, SubmitCheckReply, SubmitCheckRequest, SubmitChecks, SubmitEntry,
    append_blocks,
};
use dim_agent_capabilities::timeline_tree::TreeCommands;
use dim_agent_runtime::{BranchOptions, TimelineQuery};
use dim_agent_types::{BranchSource, EventKind, InputMode};

impl Connection {
    /// Runs the Session's submit checks before writing operator input (Hooks
    /// A.6.2); `Err` is the error the call answers with.
    async fn submit_check(
        &self,
        rpc: &RpcService,
        session: &Session,
        request: SubmitCheckRequest,
    ) -> Result<SubmitCheckReply, RpcError> {
        let checks = session
            .session_service::<SubmitChecks>()
            .map_err(RpcError::from)?;
        checks
            .check(&request, &rpc.unloading.child_token())
            .await
            .map_err(|CheckInterrupted| RpcError::reloading())
    }

    pub(super) async fn submit(&self, rpc: &RpcService, params: Submit) -> Result<Reply, RpcError> {
        if params.text.is_some() && params.user.is_some() {
            return Err(RpcError::invalid("text and user are mutually exclusive"));
        }
        let session = self.session(rpc, &params.session_id)?;
        let branch = params.branch_id.as_deref().unwrap_or("main");
        let publication = session.publication(branch).map_err(RpcError::from)?;
        let user = params.user.or_else(|| {
            params.text.map(|text| UserInput {
                content: vec![ContentBlock::Text { text }],
            })
        });
        let mut data = InputMessageData {
            submission_id: params.submission_id,
            user,
            mode: params.mode,
            ..InputMessageData::default()
        };
        match self
            .submit_check(
                rpc,
                &session,
                SubmitCheckRequest {
                    entry: SubmitEntry::Submit,
                    branch_id: branch.to_owned(),
                    user: data.user.clone(),
                    mode: data.effective_mode(),
                },
            )
            .await?
        {
            SubmitCheckReply::Block { message } => {
                return Err(RpcError::app(-32000, "input_blocked", message));
            }
            SubmitCheckReply::Proceed { blocks } => append_blocks(&mut data.user, blocks),
        }
        let event = session.input(data, branch).await.map_err(RpcError::from)?;
        if self.protocol_version() == 3 {
            let mut published = publication.watch();
            tokio::select! {
                biased;
                _ = self.closed.cancelled() => {},
                _ = rpc.unloading.cancelled() => {},
                result = published.wait_for(|&(sequence, closed)| sequence >= event.sequence || closed) => {
                    let _ = result;
                }
            }
            let failure = publication.failure();
            if publication.sequence() < event.sequence {
                if let Some(error) = failure {
                    return Err(RpcError::from(error));
                }
                return Err(RpcError::app(
                    -32000,
                    "submission_unknown",
                    "Input was queued but publication could not be confirmed",
                ));
            }
        }
        Ok(Reply::Submit(SubmitResult { event }))
    }

    pub(super) async fn input_lookup(
        &self,
        rpc: &RpcService,
        params: InputLookup,
    ) -> Result<Reply, RpcError> {
        let session = self.session(rpc, &params.session_id)?;
        let branch = params.branch_id.as_deref().expect("validated branchId");
        Ok(Reply::InputLookup(
            match session
                .lookup(branch, &params.submission_id)
                .await
                .map_err(RpcError::from)?
            {
                Some(event) => InputLookupResult::Accepted { event },
                None => InputLookupResult::Unknown,
            },
        ))
    }

    pub(super) async fn history(
        &self,
        rpc: &RpcService,
        params: History,
    ) -> Result<Reply, RpcError> {
        if params.latest_turn && params.after.is_some() {
            return Err(RpcError::invalid(
                "latestTurn and after are mutually exclusive",
            ));
        }
        if params.before.is_some() && params.after.is_some() {
            return Err(RpcError::invalid("before and after are mutually exclusive"));
        }
        if params.before == Some(0) {
            return Err(RpcError::invalid("before must be at least 1"));
        }
        if params.latest_turn && params.limit.is_some_and(|limit| limit > HISTORY_TURN_LIMIT) {
            return Err(RpcError::invalid(format!(
                "latestTurn limit must be between 1 and {HISTORY_TURN_LIMIT}"
            )));
        }
        let limit = if params.latest_turn {
            params.limit.unwrap_or(HISTORY_TURN_LIMIT)
        } else {
            params.limit.unwrap_or(HISTORY_PAGE_LIMIT)
        };
        let window = params.latest_turn || params.before.is_some();
        if window && !rpc.index.has_session_store() {
            return Err(RpcError::invalid(
                "history windows require the native Session store",
            ));
        }
        let session = self.session(rpc, &params.session_id)?;
        let branch = params.branch_id.as_deref().expect("validated branchId");
        let mut dispatched = match session.dispatch_watch(branch) {
            Ok(dispatched) => Some(dispatched),
            Err(AgentError::BranchEnded(_)) => None,
            Err(error) => return Err(RpcError::from(error)),
        };
        let head = session.sequence(branch).map_err(RpcError::from)?;
        let position = dispatched
            .as_ref()
            .map_or(head, |dispatched| dispatched.borrow().sequence);
        let through = match params.through {
            Some(through) => {
                if through > position {
                    return Err(RpcError::invalid("Future dispatched history position"));
                }
                through
            }
            None => head,
        };
        let after = params.after.unwrap_or(0);
        if after > through {
            return Err(RpcError::invalid("History cursor exceeds fixed position"));
        }
        if params.before.is_some_and(|before| before > through) {
            return Err(RpcError::invalid("History cursor exceeds fixed position"));
        }
        if let Some(dispatched) = &mut dispatched {
            loop {
                let state = dispatched.borrow().clone();
                if let Some(error) = state.failure {
                    return Err(RpcError::from(error));
                }
                if state.sequence >= through {
                    break;
                }
                tokio::select! {
                    biased;
                    _ = self.closed.cancelled() => return Err(RpcError::wire(-32000, "Connection closed")),
                    _ = rpc.unloading.cancelled() => return Err(RpcError::reloading()),
                    changed = dispatched.changed() => {
                        if changed.is_err() {
                            // The line ended while the call waited: its stored
                            // head is complete and a read of it serves that.
                            if session.ended_head(branch).is_none() {
                                return Err(RpcError::closed());
                            }
                            break;
                        }
                    }
                }
            }
        }
        if window {
            let tree = session
                .session_service::<TreeCommands>()
                .map_err(history_error)?;
            let turn = if params.latest_turn {
                turn_window(&tree, branch, through).await?
            } else {
                crate::storage::TurnWindow {
                    turn_start: None,
                    window_start: 1,
                }
            };
            let before = params.before.unwrap_or_else(|| through.saturating_add(1));
            let read = if session.ended_head(branch).is_some() {
                WindowRead::Ended(&tree)
            } else {
                WindowRead::Open(&session)
            };
            let reply = window_at(
                &read,
                branch,
                before,
                limit,
                through,
                turn,
                self.protocol_version(),
            )
            .await;
            // The owner is chosen before the read: a line that ends in between
            // answers `BranchEnded`, and the tree serves the same request at its
            // fixed head.
            let reply = match reply {
                Err(error)
                    if error
                        .data
                        .as_ref()
                        .is_some_and(|data| data.app_error == "branch_ended") =>
                {
                    window_at(
                        &WindowRead::Ended(&tree),
                        branch,
                        before,
                        limit,
                        through,
                        turn,
                        self.protocol_version(),
                    )
                    .await
                }
                other => other,
            };
            return reply.map(Reply::History);
        }
        let events = rpc
            .index
            .history_page(&params.session_id, branch, after, limit, through)
            .await
            .map_err(history_error)?;
        let next_after = events.last().map_or(after, |event| event.sequence);
        Ok(Reply::History(wire::HistoryResult {
            events: events
                .into_iter()
                .filter_map(|event| wire::ProductEvent::new(event, self.protocol_version()))
                .collect(),
            through,
            next_after,
            done: next_after == through,
            next_before: None,
            turn_start: None,
        }))
    }

    pub(super) async fn end_branch(
        &self,
        rpc: &RpcService,
        params: BranchEnd,
    ) -> Result<Reply, RpcError> {
        let session = self.session(rpc, &params.session_id)?;
        session
            .end_branch(
                &params.branch_id,
                BranchSource::Host {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                },
            )
            .await
            .map_err(RpcError::from)?;
        Ok(Reply::empty())
    }

    pub(super) async fn create_branch(
        &self,
        rpc: &RpcService,
        params: BranchCreate,
    ) -> Result<Reply, RpcError> {
        let session = self.session(rpc, &params.session_id)?;
        session
            .state(&params.parent_branch_id)
            .map_err(RpcError::from)?;
        if params.prompt.trim().is_empty() {
            return Err(RpcError::invalid("Branch prompt must not be empty"));
        }
        if !session
            .config()
            .tools
            .as_ref()
            .is_none_or(|tools| tools.iter().any(|name| name == "branch"))
        {
            return Err(RpcError::invalid("Branch tool is not enabled"));
        }
        let boundary = dim_agent_capabilities::history::validate_context_boundary_in(
            &session,
            &params.parent_branch_id,
            params.at,
        )
        .await
        .map_err(RpcError::from)?;
        boundary.map_err(RpcError::invalid)?;
        // A blocked prompt creates no branch (Hooks A.6.2). A branch's first
        // input is built from the prompt alone, so the checks' contexts are
        // dropped.
        match self
            .submit_check(
                rpc,
                &session,
                SubmitCheckRequest {
                    entry: SubmitEntry::Branch,
                    branch_id: params.parent_branch_id.clone(),
                    user: Some(UserInput {
                        content: vec![ContentBlock::Text {
                            text: params.prompt.clone(),
                        }],
                    }),
                    mode: InputMode::Queue,
                },
            )
            .await?
        {
            SubmitCheckReply::Block { message } => {
                return Err(RpcError::app(-32000, "input_blocked", message));
            }
            SubmitCheckReply::Proceed { blocks } if !blocks.is_empty() => {
                tracing::warn!(
                    contexts = blocks.len(),
                    "submit check contexts dropped: a branch's first input is its prompt"
                );
            }
            SubmitCheckReply::Proceed { .. } => {}
        }
        let id = session
            .branch(
                params.parent_branch_id,
                params.at,
                BranchOptions {
                    prompt: params.prompt,
                    source: BranchSource::Host {
                        operation_id: uuid::Uuid::new_v4().to_string(),
                    },
                },
            )
            .await
            .map_err(RpcError::from)?;
        let branch = session
            .branches()
            .into_iter()
            .find(|branch| branch.id == id)
            .expect("successful creation is published");
        Ok(Reply::BranchCreate(wire::CreateResult {
            branch: branch.into(),
        }))
    }
}

/// The default page of a `session.history` read without `latestTurn`.
const HISTORY_PAGE_LIMIT: usize = 256;
/// The default and largest page of a `latestTurn` read.
const HISTORY_TURN_LIMIT: usize = 4096;
/// Encoded `ProductEvent` bytes a `latestTurn` reply aims for. A Timeline
/// batch is the smallest unit the window can trim, so a batch above this is
/// returned whole rather than split.
const HISTORY_TURN_BYTES: usize = 4 * 1024 * 1024;
/// The largest reply a `latestTurn` page may produce: below the transport's
/// 16 MiB frame, with room for the envelope. A batch above it cannot be sent
/// and the read fails `history_too_large`.
const HISTORY_FRAME_BYTES: usize = crate::outbound::MAX_FRAME_BYTES - 64 * 1024;
/// Facts one page of the window's anchor read keeps.
const TURN_WINDOW_PAGE: u64 = 256;

fn history_error(error: AgentError) -> RpcError {
    match error {
        AgentError::InvalidInput(message) => RpcError::invalid(message),
        error => RpcError::from(error),
    }
}

/// The newest turn at `through` and the first sequence a window over it
/// includes (session-history-window §4.2): the turn's start, and the messages
/// its first request consumed or, when it has yet to take one, the newest
/// message at or below the start.
async fn turn_window(
    tree: &TreeCommands,
    branch: &str,
    through: u64,
) -> Result<crate::storage::TurnWindow, RpcError> {
    if through == 0 {
        return Ok(crate::storage::TurnWindow {
            turn_start: None,
            window_start: 1,
        });
    }
    let page = |kinds: Vec<String>,
                direction: TimelineQueryDirection,
                from: Option<u64>,
                event_id: Option<String>| TimelineQuery {
        scope: TimelineQueryScope::Branch,
        include_publishing: false,
        from_event: None,
        from_sequence: from,
        order_by: TimelineQueryOrderBy::Sequence,
        direction,
        kinds,
        flags: Vec::new(),
        event_id,
        predicate: None,
        limit: std::num::NonZeroU64::new(TURN_WINDOW_PAGE).expect("a page keeps one event"),
        offset: 0,
    };
    let turned = tree
        .query(
            branch.to_owned(),
            page(
                vec!["turn/start".into()],
                TimelineQueryDirection::Descending,
                Some(through),
                None,
            ),
        )
        .await
        .map_err(history_error)?;
    let Some(turn_start) = turned.events.first().map(|event| event.sequence) else {
        return Ok(crate::storage::TurnWindow {
            turn_start: None,
            window_start: 1,
        });
    };
    let first_step = tree
        .query(
            branch.to_owned(),
            page(
                vec!["step/model-start".into()],
                TimelineQueryDirection::Ascending,
                Some(turn_start.saturating_add(1)),
                None,
            ),
        )
        .await
        .map_err(history_error)?
        .events
        .first()
        .map(|event| event.sequence)
        .filter(|sequence| *sequence <= through);
    let mut window_start = turn_start;
    let mut consumed = false;
    if let Some(first_step) = first_step {
        let mut from = turn_start.saturating_add(1);
        loop {
            let reply = tree
                .query(
                    branch.to_owned(),
                    page(
                        vec!["input/consumed".into()],
                        TimelineQueryDirection::Ascending,
                        Some(from),
                        None,
                    ),
                )
                .await
                .map_err(history_error)?;
            let short = (reply.events.len() as u64) < TURN_WINDOW_PAGE;
            let mut next = None;
            for event in &reply.events {
                if event.sequence >= first_step {
                    break;
                }
                next = Some(event.sequence.saturating_add(1));
                let EventKind::InputConsumed(data) = &event.kind else {
                    unreachable!("the read selects consumed inputs")
                };
                let input = tree
                    .query(
                        branch.to_owned(),
                        page(
                            Vec::new(),
                            TimelineQueryDirection::Ascending,
                            None,
                            Some(data.input_id.clone()),
                        ),
                    )
                    .await
                    .map_err(history_error)?;
                if let Some(sequence) = input.events.first().map(|event| event.sequence) {
                    window_start = window_start.min(sequence);
                    consumed = true;
                }
            }
            match next {
                Some(next) if !short => from = next,
                _ => break,
            }
        }
    }
    if !consumed {
        let newest = tree
            .query(
                branch.to_owned(),
                page(
                    vec!["input/message".into()],
                    TimelineQueryDirection::Descending,
                    Some(turn_start),
                    None,
                ),
            )
            .await
            .map_err(history_error)?;
        if let Some(sequence) = newest.events.first().map(|event| event.sequence) {
            window_start = window_start.min(sequence);
        }
    }
    Ok(crate::storage::TurnWindow {
        turn_start: Some(turn_start),
        window_start,
    })
}

/// Where a window read's facts come from: a live line's own Timeline, through
/// its Session, or an ended line's stored history, through the tree that owns
/// its fixed head (ADR 021 D2).
enum WindowRead<'a> {
    Open(&'a Session),
    Ended(&'a TreeCommands),
}

impl WindowRead<'_> {
    async fn query(
        &self,
        branch: &str,
        query: TimelineQuery,
    ) -> Result<dim_agent_runtime::TimelineQueryReply, AgentError> {
        match self {
            Self::Open(session) => session.query(branch, query).await,
            Self::Ended(tree) => tree.query(branch.to_owned(), query).await,
        }
    }

    async fn batch_end(&self, branch: &str, sequence: u64) -> Result<(), AgentError> {
        match self {
            Self::Open(session) => session.batch_end(branch, sequence).await,
            Self::Ended(tree) => tree.batch_end(branch.to_owned(), sequence).await,
        }
    }
}

async fn window_at(
    read: &WindowRead<'_>,
    branch: &str,
    before: u64,
    limit: usize,
    through: u64,
    turn: crate::storage::TurnWindow,
    version: u32,
) -> Result<wire::HistoryResult, RpcError> {
    let events = window_page(read, branch, before, limit, through, turn.window_start).await?;
    history_window(read, branch, events, through, turn, version).await
}

/// The newest events below `before` inside `window_start..=through`
/// (session-history-window §4.2), read through the loaded line's query or the
/// tree's stored read. The page ends at the newest batch end at or below
/// `before - 1` and starts at a batch start; `batch_end` settles both ends
/// because a query page may begin and end inside an atomic batch (ADR 020 D4).
async fn window_page(
    read: &WindowRead<'_>,
    branch: &str,
    before: u64,
    limit: usize,
    through: u64,
    window_start: u64,
) -> Result<Vec<Arc<dim_agent_types::AgentEvent>>, RpcError> {
    if through > 0 {
        read.batch_end(branch, through)
            .await
            .map_err(history_error)?;
    }
    let top = before.saturating_sub(1).min(through);
    if top == 0 || top < window_start || limit == 0 {
        return Ok(Vec::new());
    }
    let Some(end) = batch_end_at_or_below(read, branch, top, window_start).await? else {
        return Ok(Vec::new());
    };
    let mut low = end
        .saturating_sub(limit.saturating_sub(1) as u64)
        .max(1)
        .max(window_start);
    loop {
        let events = window_events(read, branch, low, end).await?;
        let Some(first) = events.first().map(|event| event.sequence) else {
            return Ok(Vec::new());
        };
        if low <= window_start || low == 1 {
            return Ok(events);
        }
        let start = batch_start(read, branch, first, window_start).await?;
        if start >= low {
            return Ok(events);
        }
        low = start;
    }
}

/// The greatest atomic-batch end at or below `top`, searching no lower than
/// `floor`; `None` when the window holds none.
async fn batch_end_at_or_below(
    read: &WindowRead<'_>,
    branch: &str,
    top: u64,
    floor: u64,
) -> Result<Option<u64>, RpcError> {
    let mut probe = top;
    while probe >= floor && probe > 0 {
        match read.batch_end(branch, probe).await {
            Ok(()) => return Ok(Some(probe)),
            Err(AgentError::InvalidInput(_)) => probe -= 1,
            Err(error) => return Err(history_error(error)),
        }
    }
    Ok(None)
}

/// The first sequence of the batch holding `sequence`, searching no lower
/// than `floor`, which is where a window that begins inside the batch starts.
async fn batch_start(
    read: &WindowRead<'_>,
    branch: &str,
    sequence: u64,
    floor: u64,
) -> Result<u64, RpcError> {
    let mut probe = sequence.saturating_sub(1);
    while probe >= floor && probe > 0 {
        match read.batch_end(branch, probe).await {
            Ok(()) => return Ok(probe + 1),
            Err(AgentError::InvalidInput(_)) => probe -= 1,
            Err(error) => return Err(history_error(error)),
        }
    }
    Ok(floor)
}

/// `low..=end` through the source query.
async fn window_events(
    read: &WindowRead<'_>,
    branch: &str,
    low: u64,
    end: u64,
) -> Result<Vec<Arc<dim_agent_types::AgentEvent>>, RpcError> {
    let limit = end.saturating_sub(low).saturating_add(1).max(1);
    Ok(read
        .query(
            branch,
            TimelineQuery {
                scope: TimelineQueryScope::Branch,
                include_publishing: false,
                from_event: None,
                from_sequence: Some(low),
                order_by: TimelineQueryOrderBy::Sequence,
                direction: TimelineQueryDirection::Ascending,
                kinds: Vec::new(),
                flags: Vec::new(),
                event_id: None,
                predicate: None,
                limit: std::num::NonZeroU64::new(limit).expect("a page keeps one event"),
                offset: 0,
            },
        )
        .await
        .map_err(history_error)?
        .events)
}

/// One event of a window page with its encoded visible form and size.
struct WindowItem {
    sequence: u64,
    bytes: usize,
    visible: Option<wire::ProductEvent>,
}

/// Builds a newest-first window reply: the newest whole batches within the
/// event and byte limits. The newest batch is always kept; one above the byte
/// limit fails `history_too_large` rather than produce a frame the transport
/// could drop. A page within the byte target needs no batch edges beyond its
/// own ends; a larger one walks batches down from the newest, so the store's
/// `batch_end` is asked only for the events the trim inspects.
async fn history_window(
    read: &WindowRead<'_>,
    branch: &str,
    events: Vec<Arc<dim_agent_types::AgentEvent>>,
    through: u64,
    turn: crate::storage::TurnWindow,
    version: u32,
) -> Result<wire::HistoryResult, RpcError> {
    let mut items = Vec::with_capacity(events.len());
    let mut total = 0;
    for event in events {
        let sequence = event.sequence;
        let visible = wire::ProductEvent::new(event, version);
        let bytes = match &visible {
            Some(product) => serde_json::to_vec(product)
                .map_err(|error| RpcError::wire(-32603, error.to_string()))?
                .len(),
            None => 0,
        };
        total += bytes;
        items.push(WindowItem {
            sequence,
            bytes,
            visible,
        });
    }
    let keep_from = if total <= HISTORY_TURN_BYTES {
        0
    } else {
        let mut end = items.len();
        let mut total = 0;
        let mut keep_from = items.len();
        let mut newest = true;
        while end > 0 {
            let begin = batch_begin(read, branch, &items, end).await?;
            let bytes: usize = items[begin..end].iter().map(|item| item.bytes).sum();
            if newest {
                if bytes > HISTORY_FRAME_BYTES {
                    return Err(RpcError::app(
                        -32000,
                        "history_too_large",
                        format!(
                            "The newest history batch is {bytes} bytes; a reply frame allows {HISTORY_FRAME_BYTES}"
                        ),
                    ));
                }
                total = bytes;
                keep_from = begin;
                newest = false;
            } else if total + bytes <= HISTORY_TURN_BYTES {
                total += bytes;
                keep_from = begin;
            } else {
                break;
            }
            end = begin;
        }
        keep_from
    };
    let kept = items.split_off(keep_from);
    let next_before = kept.first().map(|item| item.sequence);
    let next_after = kept.last().map_or(through, |item| item.sequence);
    let done = next_before.is_none_or(|sequence| sequence <= turn.window_start);
    Ok(wire::HistoryResult {
        events: kept.into_iter().filter_map(|item| item.visible).collect(),
        through,
        next_after,
        done,
        next_before,
        turn_start: turn.turn_start,
    })
}

/// Where the batch ending at `items[end - 1]` begins. `end - 1` ends a batch;
/// walking down one event at a time, the batch-end check says whether the
/// event below starts a batch of its own, and every event above it shares this
/// one.
async fn batch_begin(
    read: &WindowRead<'_>,
    branch: &str,
    items: &[WindowItem],
    end: usize,
) -> Result<usize, RpcError> {
    let mut begin = end - 1;
    while begin > 0 {
        match read.batch_end(branch, items[begin - 1].sequence).await {
            Ok(()) => break,
            Err(AgentError::InvalidInput(_)) => begin -= 1,
            Err(error) => return Err(history_error(error)),
        }
    }
    Ok(begin)
}
