//! Goal's flows through its assembled parts: the Session part writes change
//! inputs by branch id, the branch part folds the branch's committed facts
//! and decides continuation. The product profile's agent loop drives the
//! turns these inputs queue.
mod support;

use dim_agent_types::{EventKind, InputMode};
use serde_json::json;
use support::*;

const OBJECTIVE: &str = "ship it";

/// The two blocks a `set` writes, byte for byte.
fn set_blocks(objective: &str) -> (String, String) {
    (
        format!(
            "<dim-goal kind=\"goal\" op=\"set\" branch=\"main\">\nObjective:\n{objective}\n\n\
             This is the user's standing goal for branch main. If you are on another line of this Session, the goal belongs to branch main: treat it as background, do not continue it, and do not call goal_update for it.\n\
             </dim-goal>"
        ),
        "<dim-goal kind=\"status\" status=\"active\" branch=\"main\">\n\
         The goal for branch main is active. Keep working toward it across turns. When it is fully achieved, call goal_update with status \"completed\". If you cannot make progress without the user, call goal_update with status \"blocked\" and explain why.\n\
         </dim-goal>"
            .to_owned(),
    )
}

fn continuation_text(count: usize, limit: u64, after: u64) -> String {
    format!(
        "<dim-goal kind=\"continuation\" count=\"{count}\" limit=\"{limit}\" after=\"{after}\" branch=\"main\">\n\
         Continue working toward the active goal for branch main. If it is completed or blocked, call goal_update. If you are on another line of this Session, the goal belongs to branch main: do not call goal_update for it.\n\
         </dim-goal>"
    )
}

fn pause_text(reason: &str, phrase: &str, after: u64) -> String {
    format!(
        "<dim-goal kind=\"goal\" op=\"pause\" reason=\"{reason}\" after=\"{after}\" branch=\"main\">\n\
         The goal is paused ({phrase}). Do not continue working toward it on your own; respond only to the user's latest input.\n\
         </dim-goal>"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_set_input_writes_the_objective_and_its_active_status() {
    let mut fixture = Fixture::start().await;
    let sequence = fixture
        .goal("set", json!({"objective": OBJECTIVE}))
        .await
        .unwrap();
    let writes = fixture.settled_writes("main").await;
    assert_eq!(writes.len(), 1, "{writes:#?}");
    assert_eq!(writes[0].sequence, sequence);
    assert_eq!(writes[0].mode, InputMode::Queue);
    assert_eq!(writes[0].blocks, 2, "{}", writes[0].text);
    let (objective, status) = set_blocks(OBJECTIVE);
    assert_eq!(writes[0].text, objective);
    let all = fixture.events("main", 0).await;
    let event = all
        .iter()
        .find(|event| event.sequence == sequence)
        .expect("the set input");
    let EventKind::InputMessage(data) = &event.kind else {
        panic!("the set input is an input message")
    };
    assert_eq!(data.blocks.len(), 2, "{data:?}");
    let dim_agent_types::InputBlock::Raw { text, key, retire } = &data.blocks[1];
    assert_eq!(text, &status);
    assert_eq!(key.as_deref(), Some("goal:status"));
    assert!(!retire);
    fixture.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_completed_turn_drives_the_continuation_and_the_limit_pauses_it() {
    let mut fixture = Fixture::start().await;
    fixture.set_limit(1).await;
    let id = fixture.id.clone();
    fixture
        .goal("set", json!({"objective": OBJECTIVE}))
        .await
        .unwrap();
    let paused = fixture.wait_for("main", r#"reason="limit""#).await;
    fixture.idle_on(&id, "main").await;
    let ends = turn_ends(fixture.default_session(), "main").await;
    assert_eq!(ends.len(), 2, "two turns: the set's, the continuation's");
    let continuation = fixture.wait_for("main", r#"kind="continuation""#).await;
    assert_eq!(
        continuation.text,
        continuation_text(1, 1, ends[0]),
        "the continuation counts the turn that drove it"
    );
    assert_eq!(continuation.mode, InputMode::Queue);
    assert_eq!(
        paused.text,
        pause_text(
            "limit",
            "the automatic continuation limit was reached",
            ends[1]
        )
    );
    let writes = fixture.settled_writes("main").await;
    assert_eq!(writes.len(), 3, "{writes:#?}");
    fixture.close().await;
}
