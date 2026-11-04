//! The pinned Node Goal flows (transport_flow/goal.rs) against the native
//! Goal in the product profile: `serve` with the defaults and
//! `dim_goal::install`, no TypeScript row. Each flow keeps its scenario and
//! compares Goal's writes with the literal texts of support/texts.rs byte for
//! byte.
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use dim_agent_capabilities::adapters::EchoModel;
use dim_agent_capabilities::host::{
    HostError, HostFuture, ModelChunk, ModelConnection, ModelDriver, ModelStopReason, ModelStream,
};
use dim_agent_capabilities::timeline_tree::TreeCommands;
use dim_agent_runtime::BranchOptions;
use dim_agent_types::{BranchSource, EventKind, MessageContent, ModelRequest, Role, ToolCall};
use serde_json::{Value, json};
use support::texts;
use support::*;
use tokio_util::sync::CancellationToken;

/// Text with `<`, `&`, quotes, an entity-like run and a blank line.
const SPECIAL: &str = "ship <it> & \"now\"\n\nthen &amp; done";

/// Whether the request is the evaluator's: its prompt carries Goal's
/// evaluate marker (Pursuit §5).
fn is_evaluator(request: &ModelRequest) -> bool {
    request.messages.iter().any(|message| {
        matches!(&message.content, MessageContent::Text(text)
            if text.contains("<dim-goal kind=\"evaluate\""))
    })
}

/// The `count` attribute of a Goal write's marker.
fn count_of(marker: &str) -> Option<usize> {
    marker
        .split("count=\"")
        .nth(1)?
        .split('"')
        .next()?
        .parse()
        .ok()
}

/// The `after` attribute of a Goal write's marker.
fn after_of(marker: &str) -> Option<u64> {
    marker
        .split("after=\"")
        .nth(1)?
        .split('"')
        .next()?
        .parse()
        .ok()
}

/// Answers the evaluator's prompt with a DONE conclusion; echoes otherwise.
struct EvaluatingModel;
impl ModelDriver for EvaluatingModel {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        if !is_evaluator(&request) {
            return EchoModel.invoke(request, connection, cancel);
        }
        Box::pin(async move {
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(ModelChunk::MessageStart {
                    message_id: "evaluation".into(),
                }),
                Ok(ModelChunk::Text(
                    "Read the tests and ran them.\nDONE: all tests pass".into(),
                )),
                Ok(ModelChunk::TextEnd),
                Ok(ModelChunk::MessageStop {
                    message_id: "evaluation".into(),
                    reason: ModelStopReason::EndTurn,
                }),
            ])) as ModelStream)
        })
    }
}

/// Holds the evaluator's turn in a long tool wait; echoes otherwise.
struct WaitingModel;
impl ModelDriver for WaitingModel {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        if !is_evaluator(&request) {
            return EchoModel.invoke(request, connection, cancel);
        }
        Box::pin(async move {
            let call = ToolCall {
                id: "evaluation-wait".into(),
                name: "wait".into(),
                input: json!({"millis": 60000}).as_object().unwrap().clone(),
            };
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(ModelChunk::MessageStart {
                    message_id: "evaluation".into(),
                }),
                Ok(ModelChunk::ToolStart {
                    id: call.id.clone(),
                    name: call.name.clone(),
                }),
                Ok(ModelChunk::ToolUse(call)),
                Ok(ModelChunk::MessageStop {
                    message_id: "evaluation".into(),
                    reason: ModelStopReason::ToolUse,
                }),
            ])) as ModelStream)
        })
    }
}

/// Answers the evaluator's first turn without a conclusion and its second
/// with CONTINUE; echoes otherwise.
#[derive(Default)]
struct RevisingModel {
    evaluator_turns: AtomicUsize,
}
impl ModelDriver for RevisingModel {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        if !is_evaluator(&request) {
            return EchoModel.invoke(request, connection, cancel);
        }
        let turn = self.evaluator_turns.fetch_add(1, Ordering::SeqCst);
        let text = if turn == 0 {
            "I read the tests.".to_owned()
        } else {
            "Still missing.\nCONTINUE:\n- tests still fail".to_owned()
        };
        Box::pin(async move {
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(ModelChunk::MessageStart {
                    message_id: "evaluation".into(),
                }),
                Ok(ModelChunk::Text(text)),
                Ok(ModelChunk::TextEnd),
                Ok(ModelChunk::MessageStop {
                    message_id: "evaluation".into(),
                    reason: ModelStopReason::EndTurn,
                }),
            ])) as ModelStream)
        })
    }
}

/// Answers the evaluator's turn with BLOCKED; echoes otherwise.
struct BlockedModel;
impl ModelDriver for BlockedModel {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        if !is_evaluator(&request) {
            return EchoModel.invoke(request, connection, cancel);
        }
        Box::pin(async move {
            Ok(Box::pin(futures_util::stream::iter(vec![
                Ok(ModelChunk::MessageStart {
                    message_id: "evaluation".into(),
                }),
                Ok(ModelChunk::Text(
                    "Blocked on credentials.\nBLOCKED: need credentials".into(),
                )),
                Ok(ModelChunk::TextEnd),
                Ok(ModelChunk::MessageStop {
                    message_id: "evaluation".into(),
                    reason: ModelStopReason::EndTurn,
                }),
            ])) as ModelStream)
        })
    }
}

/// Fails the evaluator's turn; echoes every other request.
struct FailingEvaluator;
impl ModelDriver for FailingEvaluator {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        if !is_evaluator(&request) {
            return EchoModel.invoke(request, connection, cancel);
        }
        Box::pin(async move { Err(HostError::failed("fixture evaluator failure")) })
    }
}

/// Fails a turn whose last operator text is `fail`; echoes otherwise.
struct FailingModel;
impl ModelDriver for FailingModel {
    fn invoke(
        &self,
        request: Arc<ModelRequest>,
        connection: ModelConnection,
        cancel: CancellationToken,
    ) -> HostFuture<'_, ModelStream> {
        let fails = request.messages.iter().rev().find_map(|message| {
            (message.role == Role::User).then(|| match &message.content {
                MessageContent::Text(text) => text.contains("fail"),
                _ => false,
            })
        });
        if fails != Some(true) {
            return EchoModel.invoke(request, connection, cancel);
        }
        Box::pin(async move { Err(HostError::failed("fixture failure")) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_continues_to_the_limit_and_stops_on_completion() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(2).await;

    // Limit (Appendix D 1, 2): set drives a turn; each normal end writes a
    // continuation until the third decision pauses with `limit`.
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    let sequence = fixture
        .goal_on(&id, "set", json!({"objective": "ship <it>"}))
        .await
        .unwrap();
    assert!(sequence > 0);
    wait_goal(&session, "main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;
    let writes = goal_writes(&session, "main").await;
    assert_eq!(writes.len(), 4, "{writes:#?}");
    assert!(writes[0].marker.contains(r#"op="set""#), "{writes:#?}");
    assert!(
        writes[1]
            .marker
            .contains(r#"kind="continuation" count="1" limit="2""#),
        "{writes:#?}"
    );
    assert!(
        writes[2]
            .marker
            .contains(r#"kind="continuation" count="2" limit="2""#),
        "{writes:#?}"
    );
    assert!(
        writes[3].marker.contains(r#"op="pause" reason="limit""#),
        "{writes:#?}"
    );
    // Every model-facing text, byte for byte. The three turns are the set's,
    // and the two continuations'.
    let ends = turn_ends(&session, "main").await;
    assert_eq!(ends.len(), 3, "three turns: {ends:?}");
    let (set_objective, set_status) = texts::set_blocks("main", "ship <it>");
    assert_eq!(
        goal_blocks(&session, "main", r#"op="set""#).await,
        vec![set_objective, set_status],
        "the set input's two blocks"
    );
    assert_eq!(
        goal_block(&session, "main", r#"count="1""#).await,
        texts::continuation("main", 1, 2, ends[0])
    );
    assert_eq!(
        goal_block(&session, "main", r#"count="2""#).await,
        texts::continuation("main", 2, 2, ends[1])
    );
    assert_eq!(
        goal_block(&session, "main", r#"reason="limit""#).await,
        texts::automatic_pause(
            "main",
            "limit",
            "the automatic continuation limit was reached",
            ends[2]
        )
    );
    assert_goal_tool(&session, "main").await;

    // A child branch cut after the goal starts with none: Goal retires the
    // inherited keys once (§3.4, Appendix D 7).
    let child = session
        .branch(
            "main".into(),
            session.sequence("main").unwrap(),
            BranchOptions {
                prompt: "child task".into(),
                source: BranchSource::Host {
                    operation_id: "goal-child".into(),
                },
            },
        )
        .await
        .unwrap();
    wait_goal(&session, &child, r#"reason="new_branch""#).await;
    fixture.idle_on(&id, &child).await;
    let child_writes = goal_writes(&session, &child).await;
    assert_eq!(child_writes.len(), 1, "{child_writes:#?}");
    assert_eq!(goal_writes(&session, "main").await.len(), 4);
    let opening = branch_events(&session, &child, ..)
        .await
        .unwrap()
        .iter()
        .find(|event| matches!(event.kind, EventKind::BranchStart(_)))
        .expect("the child's branch/start")
        .sequence;
    let (clear_objective, clear_status) = texts::start_clear(&child, opening);
    assert_eq!(
        goal_blocks(&session, &child, r#"reason="new_branch""#).await,
        vec![clear_objective, clear_status],
        "the new-branch clear's two blocks"
    );

    // Completion (Appendix D 1): the model's goal_update records `completed`
    // and no continuation follows it.
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(&id, "set", json!({"objective": "finish"}))
        .await
        .unwrap();
    fixture
        .submit_on(
            &id,
            "main",
            &format!(
                "tool:goal_update {}",
                json!({"status": "completed", "summary": SPECIAL})
            ),
        )
        .await;
    wait_goal(&session, "main", r#"op="completed""#).await;
    fixture.idle_on(&id, "main").await;
    let (finish_set, finish_status) = texts::set_blocks("main", "finish");
    assert_eq!(
        goal_blocks(&session, "main", r#"op="set""#).await,
        vec![finish_set, finish_status],
        "the second session's set"
    );
    assert_eq!(
        goal_block(&session, "main", r#"op="completed""#).await,
        texts::report("main", "completed", SPECIAL),
        "the completed report"
    );
    wait_tool_text(&session, "main", "goal_update recorded: completed.").await;
    let completed_block = goal_block(&session, "main", r#"op="completed""#).await;
    assert!(!completed_block.contains("<it>"), "{completed_block}");
    assert_eq!(block_value(&completed_block, "Result:"), SPECIAL);
    let writes = goal_writes(&session, "main").await;
    let completed = writes
        .iter()
        .find(|block| block.marker.contains(r#"op="completed""#))
        .expect("the completed report")
        .sequence;
    assert!(
        writes
            .iter()
            .all(|block| block.sequence <= completed || !block.text.contains("continuation")),
        "{writes:#?}"
    );
    let results: Vec<String> = branch_events(&session, "main", ..)
        .await
        .unwrap()
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ToolResult(data) => Some(serde_json::to_string(data).unwrap()),
            _ => None,
        })
        .collect();
    assert!(
        results
            .iter()
            .any(|result| result.contains("goal_update recorded: completed.")),
        "{results:#?}"
    );

    let error = fixture
        .goal_on(&id, "set", json!({"objective": ""}))
        .await
        .unwrap_err();
    assert_eq!(dim_client::app_error(&error), Some("invalid_params"));
    let error = fixture
        .goal_on(&id, "set", json!({"branchId": "missing", "objective": "x"}))
        .await
        .unwrap_err();
    assert_eq!(dim_client::app_error(&error), Some("session_invalid_input"));
    for method in ["pause", "resume", "clear"] {
        fixture.goal_on(&id, method, json!({})).await.unwrap();
    }
    wait_goal(&session, "main", r#"op="clear""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"op="pause" reason="user""#).await,
        texts::pause("main")
    );
    assert_eq!(
        goal_block(&session, "main", r#"op="resume""#).await,
        texts::resume("main")
    );
    let (clear_objective, clear_status) = texts::clear("main");
    assert_eq!(
        goal_blocks(&session, "main", r#"op="clear""#).await,
        vec![clear_objective, clear_status],
        "the user's clear"
    );

    // The tool's rejection text, and a `blocked` report.
    fixture
        .submit_on(
            &id,
            "main",
            r#"tool:goal_update {"status":"nope","summary":"x"}"#,
        )
        .await;
    wait_tool_text(&session, "main", texts::TOOL_INVALID).await;
    fixture
        .submit_report_on(&id, "blocked", "needs credentials")
        .await;
    wait_goal(&session, "main", r#"op="blocked""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"op="blocked""#).await,
        texts::report("main", "blocked", "needs credentials")
    );
    wait_tool_text(&session, "main", "goal_update recorded: blocked.").await;
    let written = goal_writes(&session, "main").await;
    let after: Vec<&Block> = written
        .iter()
        .filter(|block| block.sequence > completed)
        .collect();
    let at = |op: &str| {
        after
            .iter()
            .position(|block| block.marker.contains(&format!(r#"op="{op}""#)))
            .unwrap_or_else(|| panic!("no {op}: {after:#?}"))
    };
    assert!(
        at("pause") < at("resume") && at("resume") < at("clear"),
        "{after:#?}"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_restore_refolds_without_repeating_decisions() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(&id, "set", json!({"objective": SPECIAL}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        block_value(
            &goal_block(&session, "main", r#"op="set""#).await,
            "Objective:"
        ),
        SPECIAL
    );
    let edited = format!("{SPECIAL} (edited)");
    fixture
        .goal_on(&id, "edit", json!({"objective": edited}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"op="edit""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"op="edit""#).await,
        texts::edit("main", &edited)
    );
    let before = goal_writes(&session, "main").await;
    assert_eq!(before.len(), 4, "{before:#?}");
    drop(session);
    let mut fixture = fixture.restart(Arc::new(EchoModel)).await;
    fixture.submit_on(&id, "main", "hello again").await;
    fixture.idle_on(&id, "main").await;
    let session = fixture.agent.get_session(&id).unwrap();
    assert_eq!(goal_writes(&session, "main").await, before);
    assert_eq!(
        block_value(
            &goal_block(&session, "main", r#"op="edit""#).await,
            "Objective:"
        ),
        edited
    );
    fixture.goal_on(&id, "resume", json!({})).await.unwrap();
    wait_goal(&session, "main", r#"kind="continuation" count="1""#).await;
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_pending_input_and_interrupts() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1).await;

    // Appendix D 4: the goal is set while a turn runs, and the operator
    // queues more input. The turn's end finds Queue input pending, so it
    // decides nothing; the turn that consumes them decides.
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .submit_on(&id, "main", r#"tool:wait {"millis":1500}"#)
        .await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::ToolCall(_))
    })
    .await;
    fixture
        .goal_on(&id, "set", json!({"objective": "ship"}))
        .await
        .unwrap();
    fixture.submit_on(&id, "main", "and the changelog").await;
    wait_goal(&session, "main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;
    let ends = turn_ends(&session, "main").await;
    let writes = goal_writes(&session, "main").await;
    let decided: Vec<&String> = writes
        .iter()
        .map(|block| &block.marker)
        .filter(|line| line.contains("after="))
        .collect();
    assert!(
        decided
            .iter()
            .all(|line| !line.contains(&format!(r#"after="{}""#, ends[0]))),
        "{ends:?} {writes:#?}"
    );
    assert!(
        decided
            .iter()
            .any(|line| line.contains(r#"kind="continuation" count="1""#)),
        "{writes:#?}"
    );
    let continuation = writes
        .iter()
        .find(|block| block.marker.contains(r#"count="1""#))
        .expect("the first continuation")
        .sequence;
    let deciding = ends
        .iter()
        .rev()
        .find(|end| **end < continuation)
        .copied()
        .expect("the deciding turn");
    assert_eq!(
        goal_block(&session, "main", r#"count="1""#).await,
        texts::continuation("main", 1, 1, deciding)
    );

    // Appendix D 3: an interrupt after the goal starts pauses it, and no
    // continuation follows.
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .submit_on(&id, "main", r#"tool:wait {"millis":1500}"#)
        .await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::ToolCall(_))
    })
    .await;
    fixture
        .goal_on(&id, "set", json!({"objective": "ship"}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"op="set""#).await;
    fixture
        .client
        .rpc("session.stop", json!({"sessionId": id, "branchId": "main"}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"reason="interrupted""#).await;
    fixture.idle_on(&id, "main").await;
    let interrupt = branch_events(&session, "main", ..)
        .await
        .unwrap()
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::StopInterrupt(_)))
        .expect("the interrupt")
        .sequence;
    assert_eq!(
        goal_block(&session, "main", r#"reason="interrupted""#).await,
        texts::automatic_pause(
            "main",
            "interrupted",
            "the user interrupted the turn",
            interrupt
        )
    );
    let writes = goal_writes(&session, "main").await;
    assert!(
        writes
            .iter()
            .all(|block| !block.text.contains("continuation")),
        "{writes:#?}"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_settings_resume_unload_and_fork() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(&id, "set", json!({"objective": "ship"}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;

    // Appendix D 11: completed, then resume reopens the goal with the count
    // restarted. Appendix D 8: read the committed limit at the next decision,
    // without reloading Goal or waiting for a settings.changed callback.
    fixture.submit_report_on(&id, "completed", "done").await;
    wait_goal(&session, "main", r#"op="completed""#).await;
    fixture.idle_on(&id, "main").await;
    fixture.set_limit(3).await;
    fixture.goal_on(&id, "resume", json!({})).await.unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let writes = goal_writes(&session, "main").await;
            let limits = writes
                .iter()
                .filter(|block| block.marker.contains(r#"reason="limit""#))
                .count();
            if limits == 2 {
                assert!(
                    writes
                        .iter()
                        .any(|block| block.marker.contains(r#"count="3" limit="3""#)),
                    "{writes:#?}"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the resumed goal pauses at the new limit");
    fixture.idle_on(&id, "main").await;

    // Appendix D 8: an unloaded Session is restored by a goal method, which
    // writes.
    assert!(session.unload_if_idle().await.unwrap());
    drop(session);
    fixture.goal_on(&id, "pause", json!({})).await.unwrap();
    let session = fixture.agent.get_session(&id).unwrap();
    wait_goal(&session, "main", r#"op="pause" reason="user""#).await;
    fixture.idle_on(&id, "main").await;

    // Appendix D 7: a fork writes `session/forked` after the copied facts.
    // The forked `main`'s goal starts empty: one start `clear` after that
    // fact, and no continuation.
    let at = session.sequence("main").unwrap();
    let fork_id = fixture.fork_on(&id, at).await;
    let forked = fixture.agent.get_session(&fork_id).unwrap();
    wait_goal(&forked, "main", r#"reason="new_branch""#).await;
    fixture.idle_on(&fork_id, "main").await;
    let events = branch_events(&forked, "main", ..).await.unwrap();
    let opening = events
        .iter()
        .find(|event| matches!(&event.kind, EventKind::SessionForked(_)))
        .expect("the fork's fact")
        .sequence;
    let writes = goal_writes(&forked, "main").await;
    let own: Vec<&Block> = writes
        .iter()
        .filter(|block| block.sequence > opening)
        .collect();
    assert_eq!(own.len(), 1, "{writes:#?}");
    assert!(
        own[0]
            .marker
            .contains(&format!(r#"reason="new_branch" after="{opening}""#)),
        "{own:#?}"
    );
    let (fork_clear, fork_status) = texts::start_clear("main", opening);
    assert_eq!(
        goal_blocks(&forked, "main", r#"reason="new_branch""#).await,
        vec![fork_clear, fork_status],
        "the fork's new-branch clear"
    );
    assert!(
        goal_writes(&session, "main")
            .await
            .iter()
            .all(|block| !block.marker.contains("new_branch"))
    );
    fixture.close().await;

    // Appendix D 8: an assembly without Goal has no goal methods.
    let mut fixture = Fixture::start_without_goal().await;
    let id = fixture.new_session(&[]).await;
    let error = fixture
        .goal_on(&id, "set", json!({"objective": "x"}))
        .await
        .unwrap_err();
    assert_eq!(dim_client::app_error(&error), Some("unknown_method"));
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_on_a_line_behaves_as_on_main() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1000).await;
    let id = fixture
        .new_session(&["goal_update", "wait", "branch"])
        .await;
    let session = fixture.agent.get_session(&id).unwrap();
    let at = session.sequence("main").unwrap();
    let created = fixture
        .client
        .rpc(
            "session.branch_create",
            json!({"sessionId":id,"parentBranchId":"main","at":at,"prompt":"line work"}),
        )
        .await
        .unwrap();
    let child = created["branch"]["branchId"].as_str().unwrap().to_owned();
    fixture.idle_on(&id, &child).await;
    let opening = branch_events(&session, &child, ..)
        .await
        .unwrap()
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::InputMessage(data) => data.blocks.first().map(|block| {
                let dim_agent_types::InputBlock::Raw { text, .. } = block;
                (text.clone(), data.user.clone())
            }),
            _ => None,
        })
        .expect("the line's opening input");
    let marker = format!(r#"<dim-branch-line branch="{child}" parent="main" source="host">"#);
    assert!(opening.0.starts_with(&marker), "{}", opening.0);
    assert_eq!(
        first_user_text(&session, &child).await,
        "line work",
        "the creator's prompt stays the opening input's words"
    );
    fixture
        .goal_on(
            &id,
            "set",
            json!({"branchId": &child, "objective": "line objective"}),
        )
        .await
        .unwrap();
    wait_goal(&session, &child, r#"op="set""#).await;
    let (line_set, line_status) = texts::set_blocks(&child, "line objective");
    assert_eq!(
        goal_blocks(&session, &child, r#"op="set""#).await,
        vec![line_set, line_status],
        "the line's set"
    );
    wait_goal(&session, &child, r#"count="1""#).await;
    fixture.idle_on(&id, &child).await;
    let operator = session
        .submit(
            "keep going".into(),
            dim_agent_types::InputMode::Queue,
            &child,
        )
        .await
        .unwrap()
        .sequence;
    let (reset, expected) = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let writes = goal_writes(&session, &child).await;
            let reset = writes.iter().find(|block| {
                block.marker.contains(r#"kind="continuation""#)
                    && after_of(&block.marker).is_some_and(|after| after > operator)
            });
            if let Some(block) = reset {
                let raced = writes
                    .iter()
                    .filter(|other| {
                        other.marker.contains(r#"kind="continuation""#)
                            && other.sequence > operator
                            && other.sequence < block.sequence
                    })
                    .count();
                return (block.marker.clone(), 1 + raced);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a continuation after the operator input");
    assert!(
        reset.contains(&format!(r#"count="{expected}""#)),
        "the continuation after the operator input counts {expected}: {reset}"
    );
    let after = after_of(&reset).expect("the continuation's after");
    assert_eq!(
        goal_block(&session, &child, &format!(r#"count="{expected}""#)).await,
        texts::continuation(&child, expected, 1000, after),
        "the line's continuation"
    );
    assert!(goal_writes(&session, "main").await.is_empty());
    fixture
        .goal_on(&id, "pause", json!({"branchId": &child}))
        .await
        .unwrap();
    wait_goal(&session, &child, r#"op="pause""#).await;
    fixture.idle_on(&id, &child).await;
    assert_eq!(
        goal_block(&session, &child, r#"op="pause""#).await,
        texts::pause(&child)
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_evaluator_judge_creates_an_evaluator_and_completes_on_done() {
    let mut fixture = Fixture::start_with(Arc::new(EvaluatingModel)).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(
            &id,
            "set",
            json!({"objective": "make the tests pass", "judge": "evaluator"}),
        )
        .await
        .unwrap();
    wait_goal(&session, "main", r#"op="set""#).await;
    let (judged_set, judged_status) = texts::evaluator_set_blocks("main", "make the tests pass");
    assert_eq!(
        goal_blocks(&session, "main", r#"op="set""#).await,
        vec![judged_set, judged_status],
        "the judged set"
    );
    fixture
        .submit_report_on(&id, "completed", "all tests pass")
        .await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::BranchCreate(_))
    })
    .await;
    fixture.idle_on(&id, "main").await;
    let creates = branch_creates(&session, "main").await;
    assert_eq!(creates.len(), 1, "{creates:#?}");
    let (created, creation) = &creates[0];
    let evaluation = turn_ends(&session, "main")
        .await
        .last()
        .copied()
        .expect("a turn end");
    assert_eq!(
        creation.source,
        BranchSource::Host {
            operation_id: format!("goal:evaluate:{evaluation}")
        },
        "{creation:#?}"
    );
    let writes = goal_writes(&session, "main").await;
    let request = writes
        .iter()
        .find(|block| block.marker.contains(r#"op="completed""#) && !block.marker.contains("by="))
        .unwrap_or_else(|| panic!("the worker's request: {writes:#?}"));
    assert!(
        *created > request.sequence,
        "the request precedes the creation: {writes:#?}"
    );
    assert!(
        writes
            .iter()
            .all(|block| !block.marker.contains("continuation")),
        "{writes:#?}"
    );
    let child = &creation.branch;
    let prompt = first_user_text(&session, child).await;
    assert_eq!(
        prompt,
        texts::evaluator_prompt("main", evaluation, "make the tests pass", "all tests pass"),
        "the evaluator's prompt"
    );
    wait_goal(&session, "main", r#"by="evaluator""#).await;
    fixture.idle_on(&id, "main").await;
    let done = goal_block(&session, "main", r#"by="evaluator""#).await;
    assert_eq!(
        done,
        texts::evaluated("main", "completed", evaluation, "all tests pass"),
        "the evaluator's completion"
    );
    // The conclusion it decides on is one input of one block, and that
    // block's marker is the report of this evaluation and evaluator
    // (Pursuit §5).
    let conclusions: Vec<(usize, String)> = branch_events(&session, "main", ..)
        .await
        .unwrap()
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::InputMessage(data) => data.blocks.first().and_then(|block| {
                let dim_agent_types::InputBlock::Raw { text, .. } = block;
                text.starts_with(r#"<dim-goal kind="report""#)
                    .then(|| (data.blocks.len(), text.clone()))
            }),
            _ => None,
        })
        .collect();
    assert_eq!(conclusions.len(), 1, "{conclusions:#?}");
    assert_eq!(
        conclusions[0].0, 1,
        "the conclusion input carries the report alone"
    );
    assert_eq!(
        conclusions[0].1,
        texts::conclusion("main", child, evaluation, "done", "DONE: all tests pass"),
        "the conclusion input"
    );
    let writes = goal_writes(&session, "main").await;
    assert_eq!(
        writes
            .iter()
            .filter(|block| block.marker.contains(r#"by="evaluator""#))
            .count(),
        1,
        "{writes:#?}"
    );
    wait_event(
        &session,
        "main",
        |kind| matches!(kind, EventKind::BranchEnded(data) if data.branch == *child),
    )
    .await;
    fixture.idle_on(&id, "main").await;
    let head = session.sequence(child).unwrap();
    assert_eq!(session.ended_head(child), Some(head));
    let line_events = session
        .session_service::<TreeCommands>()
        .unwrap()
        .page(child.to_owned(), 0, 256, head)
        .await
        .unwrap();
    assert_eq!(
        line_events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::TurnEnd(_)))
            .count(),
        1,
        "one evaluator turn"
    );
    let endings: Vec<Value> = branch_events(&session, "main", ..)
        .await
        .unwrap()
        .iter()
        .filter(
            |event| matches!(&event.kind, EventKind::BranchEnded(data) if data.branch == *child),
        )
        .map(|event| serde_json::to_value(&event.kind).unwrap())
        .collect();
    assert_eq!(endings.len(), 1, "{endings:#?}");
    assert_eq!(
        endings[0]["data"]["source"],
        json!({"kind": "host", "operation_id": format!("goal:evaluate:{evaluation}")}),
        "{endings:#?}"
    );
    assert_eq!(endings[0]["data"]["branch"], *child, "{endings:#?}");
    let line = fixture
        .client
        .rpc(
            "session.history",
            json!({"sessionId":id,"branchId":child,"after":0,"limit":256}),
        )
        .await
        .unwrap();
    assert_eq!(line["done"], true, "{line:#?}");
    assert!(
        line["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "branch"
                && event["name"] == "end"
                && event["data"]["source"]["operation_id"]
                    == format!("goal:evaluate:{evaluation}")),
        "{line:#?}"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_evaluator_restart_writes_no_second_result() {
    let mut fixture = Fixture::start_with(Arc::new(WaitingModel)).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(
            &id,
            "set",
            json!({"objective": "ship it", "judge": "evaluator"}),
        )
        .await
        .unwrap();
    fixture.submit_report_on(&id, "completed", "done").await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::BranchCreate(_))
    })
    .await;
    let creates = branch_creates(&session, "main").await;
    let child = creates[0].1.branch.clone();
    wait_event(&session, &child, |kind| {
        matches!(kind, EventKind::ToolCall(_))
    })
    .await;
    drop(session);
    let mut fixture = fixture.restart(Arc::new(EvaluatingModel)).await;
    fixture.submit_on(&id, "main", "back again").await;
    fixture.idle_on(&id, "main").await;
    let session = fixture.agent.get_session(&id).unwrap();
    assert_eq!(branch_creates(&session, "main").await.len(), 1);
    let writes = goal_writes(&session, "main").await;
    assert!(
        writes
            .iter()
            .all(|block| !block.marker.contains(r#"by="evaluator""#)),
        "a replayed turn end reports nothing: {writes:#?}"
    );
    assert!(
        writes
            .iter()
            .all(|block| !block.marker.contains(r#"reason="evaluation_failed""#)),
        "{writes:#?}"
    );
    fixture.submit_on(&id, &child, "carry on").await;
    wait_goal(&session, "main", r#"by="evaluator""#).await;
    fixture.idle_on(&id, "main").await;
    let writes = goal_writes(&session, "main").await;
    assert_eq!(
        writes
            .iter()
            .filter(|block| block.marker.contains(r#"by="evaluator""#))
            .count(),
        1,
        "{writes:#?}"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_evaluator_asks_again_and_continues() {
    let mut fixture = Fixture::start_with(Arc::new(RevisingModel::default())).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(
            &id,
            "set",
            json!({"objective": "make the tests pass", "judge": "evaluator"}),
        )
        .await
        .unwrap();
    fixture
        .submit_report_on(&id, "completed", "all tests pass")
        .await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::BranchCreate(_))
    })
    .await;
    let evaluation: u64 = evaluation_of(&session, "main").await.parse().unwrap();
    let child = branch_creates(&session, "main").await[0].1.branch.clone();

    // The first evaluator turn ends without a conclusion: Goal asks again on
    // the evaluator's own line.
    wait_goal(&session, &child, r#"kind="evaluate-again""#).await;
    let first_end = turn_ends(&session, &child).await[0];
    assert_eq!(
        goal_block(&session, &child, r#"kind="evaluate-again""#).await,
        texts::evaluate_again(&child, first_end),
        "the evaluator's follow-up"
    );

    // The second ends with CONTINUE: its report decides the evaluation, and
    // the working branch continues.
    let needle = format!(r#"evaluation="{evaluation}""#);
    wait_goal(&session, "main", &needle).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"kind="report""#).await,
        texts::conclusion(
            "main",
            &child,
            evaluation,
            "continue",
            "CONTINUE:\n- tests still fail"
        ),
        "the CONTINUE report"
    );
    let continuation = goal_block(&session, "main", &needle).await;
    let count = count_of(&continuation).expect("the continuation's count");
    let after = after_of(&continuation).expect("the continuation's after");
    assert_eq!(
        continuation,
        texts::evaluated_continuation("main", count, 5, after, evaluation),
        "the evaluator's continuation"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_evaluator_blocks_or_fails_the_evaluation() {
    // An evaluator that reports BLOCKED completes the goal as blocked.
    let mut fixture = Fixture::start_with(Arc::new(BlockedModel)).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(
            &id,
            "set",
            json!({"objective": "ship it", "judge": "evaluator"}),
        )
        .await
        .unwrap();
    fixture.submit_report_on(&id, "completed", "done").await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::BranchCreate(_))
    })
    .await;
    let evaluation: u64 = evaluation_of(&session, "main").await.parse().unwrap();
    let child = branch_creates(&session, "main").await[0].1.branch.clone();
    wait_goal(&session, "main", r#"by="evaluator""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"by="evaluator""#).await,
        texts::evaluated("main", "blocked", evaluation, "need credentials"),
        "the evaluator's block"
    );
    assert_eq!(
        goal_block(&session, "main", r#"kind="report""#).await,
        texts::conclusion(
            "main",
            &child,
            evaluation,
            "blocked",
            "BLOCKED: need credentials"
        ),
        "the BLOCKED report"
    );
    fixture.close().await;

    // An evaluator whose turn fails pauses the goal with `evaluation_failed`.
    let mut fixture = Fixture::start_with(Arc::new(FailingEvaluator)).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(
            &id,
            "set",
            json!({"objective": "ship it", "judge": "evaluator"}),
        )
        .await
        .unwrap();
    fixture.submit_report_on(&id, "completed", "done").await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::BranchCreate(_))
    })
    .await;
    let evaluation: u64 = evaluation_of(&session, "main").await.parse().unwrap();
    let child = branch_creates(&session, "main").await[0].1.branch.clone();
    wait_goal(&session, "main", r#"reason="evaluation_failed""#).await;
    fixture.idle_on(&id, "main").await;
    assert_eq!(
        goal_block(&session, "main", r#"reason="evaluation_failed""#).await,
        texts::automatic_pause(
            "main",
            "evaluation_failed",
            "the evaluator could not check the result",
            evaluation
        ),
        "the failed evaluation"
    );
    assert_eq!(
        goal_block(&session, "main", r#"kind="report""#).await,
        texts::conclusion(
            "main",
            &child,
            evaluation,
            "failed",
            "the evaluator's turn did not complete"
        ),
        "the evaluator's failure report"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_turn_failure_pauses_the_goal() {
    let mut fixture = Fixture::start_with(Arc::new(FailingModel)).await;
    fixture.set_limit(5).await;
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .submit_on(&id, "main", r#"tool:wait {"millis":1500}"#)
        .await;
    wait_event(&session, "main", |kind| {
        matches!(kind, EventKind::ToolCall(_))
    })
    .await;
    fixture
        .goal_on(&id, "set", json!({"objective": "ship"}))
        .await
        .unwrap();
    fixture.submit_on(&id, "main", "now fail").await;
    wait_goal(&session, "main", r#"reason="turn_failed""#).await;
    fixture.idle_on(&id, "main").await;
    let failed = turn_ends(&session, "main")
        .await
        .last()
        .copied()
        .expect("a turn end");
    assert_eq!(
        goal_block(&session, "main", r#"reason="turn_failed""#).await,
        texts::automatic_pause("main", "turn_failed", "the last turn failed", failed),
        "the failed turn's pause"
    );
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn goal_registers_the_entry_for_the_marker_it_writes() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1).await;

    // `session.goal.set` writes the `<dim-goal` input and drives a turn.
    let id = fixture.new_session(&["goal_update", "wait"]).await;
    let session = fixture.agent.get_session(&id).unwrap();
    fixture
        .goal_on(&id, "set", json!({"objective": "ship it"}))
        .await
        .unwrap();
    wait_goal(&session, "main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;

    // The request that carries the `<dim-goal` input carries the entry too
    // (A.5.1): the Session part registers it for the Session, so every
    // branch's System holds it while the row is loaded.
    let request = last_request(&session, "main").await;
    assert!(
        serde_json::to_string(&request.messages)
            .unwrap()
            .contains("<dim-goal "),
        "the last request carried no <dim-goal input: {request:#?}"
    );
    let system = last_system(&session, "main").await;
    assert!(system.contains(texts::ENTRY), "{system:?}");

    fixture.close().await;
}
