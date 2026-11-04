//! The `goal.test.mjs` Pursuit cases: the evaluator judge, its line, its
//! conclusions and the ends it records. The part harness supplies every fact
//! the Loop and TimelineTree would write; expectations are Goal's inputs.
mod support;

use std::time::Duration;

use dim_agent_types::{
    BranchCreateAt, BranchCreateData, BranchCreateFailedData, BranchEndedData, BranchSource,
    InputBlock, InputMode,
};
use serde_json::json;
use support::parts::*;
use support::texts;

/// Waits for the next `branch/create` on `branch`.
async fn next_create(env: &Env, branch: &str, seen: usize) -> BranchCreateData {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let creates = env.creates(branch).await;
            if let Some(create) = creates.into_iter().nth(seen) {
                return create;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("a branch/create")
}

/// A worker turn that ends with the goal's completion request, and the
/// evaluator Goal creates for it. The evaluator clears the inherited goal on
/// its own line, as every new line does; its parts are mounted on return.
async fn completion_requested(env: &Env) -> (u64, BranchCreateData) {
    completion_requested_with(env, 20).await
}

/// [`completion_requested`] under the Settings limit `limit`.
async fn completion_requested_with(env: &Env, limit: u64) -> (u64, BranchCreateData) {
    env.goal("set", json!({"objective": "ship it", "judge": "evaluator"}))
        .await
        .expect("set writes");
    let set = env.wait_for("main", r#"op="set""#).await;
    let end = env.turn("main", &[&set]);
    let continuation = env
        .wait_for(
            "main",
            &format!(
                r#"kind="continuation" count="1" limit="{limit}" after="{}""#,
                end.sequence
            ),
        )
        .await;
    env.turn_start("main", "work");
    env.consume("main", &continuation.id);
    env.tool(
        "main",
        json!({"status": "completed", "summary": "all tests pass"}),
    )
    .await;
    let end = env.end_turn("main", "work", "completed");
    let create = next_create(env, "main", 0).await;
    assert_eq!(
        create.source,
        BranchSource::Host {
            operation_id: format!("goal:evaluate:{}", end.sequence)
        }
    );
    let evaluator = create.branch.clone();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !env.has_branch(&evaluator) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the evaluator's parts activate");
    env.wait_for(&evaluator, r#"reason="new_branch""#).await;
    (end.sequence, create)
}

/// Writes one conclusion about `evaluation` on `branch`, as the evaluator's
/// line reports it.
fn write_conclusion(env: &Env, evaluator: &str, evaluation: u64, result: &str, text: &str) {
    let conclusion = texts::conclusion("main", evaluator, evaluation, result, text);
    env.write_message(
        "main",
        vec![raw(&conclusion, None, false)],
        InputMode::Append,
    );
}

/// The Goal writes on `branch`, newest first, that contain `needle`.
async fn writes_with(env: &Env, branch: &str, needle: &str) -> Vec<Block> {
    env.writes(branch)
        .await
        .into_iter()
        .filter(|block| block.text.contains(needle))
        .collect()
}

/// The pending inputs of `branch`, as string slices for a turn.
async fn pending_refs(env: &Env, branch: &str) -> Vec<String> {
    env.pending_inputs(branch).await
}

/// Drives one completed turn on `branch` that takes in what is pending.
async fn line_turn(env: &Env, branch: &str, turn_id: &str, steps: &[(&str, &[&str])]) {
    let pending = pending_refs(env, branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    env.turn_steps(branch, turn_id, &pending, "completed", steps);
}

#[tokio::test]
async fn pursuit_1_the_judge_is_recorded_kept_by_edit_and_a_completion_is_a_request() {
    let env = Env::start().await;
    env.goal("set", json!({"objective": "ship it", "judge": "evaluator"}))
        .await
        .expect("set writes");
    let set = env.wait_for("main", r#"op="set""#).await;
    assert_eq!(set.text, texts::evaluator_set_blocks("main", "ship it").0);

    env.goal("edit", json!({"objective": "ship it twice"}))
        .await
        .expect("edit writes");
    let edit = env.wait_for("main", r#"op="edit""#).await;
    assert_eq!(edit.text, texts::edit("main", "ship it twice"));

    let end = env.turn("main", &[&set, &edit]);
    let continuation = env.wait_for("main", r#"kind="continuation""#).await;
    assert_eq!(
        continuation.text,
        texts::continuation("main", 1, 20, end.sequence)
    );

    // The completion is a request: the goal stays active and an evaluator
    // takes the turn end.
    env.turn_start("main", "work");
    env.consume("main", &continuation.id);
    env.tool(
        "main",
        json!({"status": "completed", "summary": "all tests pass"}),
    )
    .await;
    let end = env.end_turn("main", "work", "completed");
    let create = next_create(&env, "main", 0).await;
    assert!(
        writes_with(&env, "main", r#"by="evaluator""#)
            .await
            .is_empty(),
        "a request does not complete the goal"
    );

    // A later turn end without a request decides the limit, not an
    // evaluation.
    env.turn_start("main", "next");
    let end2 = env.end_turn("main", "next", "completed");
    let next = env
        .wait_for(
            "main",
            &format!(
                r#"kind="continuation" count="2" limit="20" after="{}""#,
                end2.sequence
            ),
        )
        .await;
    assert_eq!(next.text, texts::continuation("main", 2, 20, end2.sequence));
    assert_eq!(env.creates("main").await.len(), 1);

    // clear returns to the model judge.
    env.goal("clear", json!({})).await.expect("clear writes");
    env.wait_for("main", r#"op="clear""#).await;
    env.goal("set", json!({"objective": "again"}))
        .await
        .expect("set writes");
    let again = env
        .writes("main")
        .await
        .into_iter()
        .rev()
        .find(|block| block.text.contains(r#"op="set""#))
        .expect("the second set");
    assert_eq!(again.text, texts::set_blocks("main", "again").0);
    env.turn_start("main", "model");
    env.tool("main", json!({"status": "completed", "summary": "done"}))
        .await;
    env.end_turn("main", "model", "completed");
    let completed = env.wait_for("main", "Result:\ndone").await;
    assert_eq!(completed.text, texts::report("main", "completed", "done"));
    assert_eq!(env.creates("main").await.len(), 1);
    let _ = (end.sequence, create);
    env.close().await;
}

#[tokio::test]
async fn pursuit_2_7_10_a_conclusion_decides_once_from_its_own_evaluator() {
    // done: the evaluator completes the goal, once.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    let evaluator = create.branch.clone();
    write_conclusion(
        &env,
        &evaluator,
        evaluation,
        "done",
        "DONE: all 3 tests pass",
    );
    let completed = env.wait_for("main", r#"by="evaluator""#).await;
    assert_eq!(
        completed.text,
        texts::evaluated("main", "completed", evaluation, "all 3 tests pass")
    );
    env.wait_for("main", r#"kind="report""#).await;
    let conclusion = env.written_input("main", r#"kind="report""#).await;
    assert_eq!(
        match &conclusion.blocks[0] {
            InputBlock::Raw { text, .. } => text.clone(),
        },
        texts::conclusion(
            "main",
            &evaluator,
            evaluation,
            "done",
            "DONE: all 3 tests pass"
        )
    );

    // The same conclusion again, and a foreign evaluator's, decide nothing.
    let before = env.settled("main").await.len();
    write_conclusion(&env, &evaluator, evaluation, "done", "DONE: again");
    write_conclusion(&env, "F", evaluation, "done", "DONE: forged");
    assert_eq!(env.settled("main").await.len(), before + 2);
    env.close().await;

    // blocked.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "blocked",
        "BLOCKED: a deploy key",
    );
    let blocked = env.wait_for("main", r#"op="blocked""#).await;
    assert_eq!(
        blocked.text,
        texts::evaluated("main", "blocked", evaluation, "a deploy key")
    );
    env.close().await;

    // continue: one Pursuit continuation, with the Goal count.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "continue",
        "CONTINUE:\n- two tests fail: <output>\n- the changelog",
    );
    let needle = format!(
        r#"kind="continuation" count="2" limit="20" after="{evaluation}" evaluation="{evaluation}""#
    );
    let continuation = env.wait_for("main", &needle).await;
    assert_eq!(
        continuation.text,
        texts::evaluated_continuation("main", 2, 20, evaluation, evaluation)
    );
    env.close().await;

    // failed: the evaluation fails.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "failed",
        "the evaluator's turn did not complete",
    );
    let pause = env.wait_for("main", r#"reason="evaluation_failed""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "evaluation_failed",
            "the evaluator could not check the result",
            evaluation
        )
    );
    env.close().await;

    // A conclusion after the user paused the goal decides nothing.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    env.goal("pause", json!({})).await.expect("pause writes");
    env.wait_for("main", r#"op="pause""#).await;
    let before = env.settled("main").await.len();
    write_conclusion(&env, &create.branch, evaluation, "done", "DONE: late");
    assert_eq!(env.settled("main").await.len(), before + 1);
    env.close().await;
}

#[tokio::test]
async fn pursuit_4_8_the_report_is_the_last_conclusion_line_cut_to_4000() {
    // The reply's last conclusion line is the report.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    line_turn(
        &env,
        &create.branch,
        "eval-1",
        &[("r1", &["Checked the tree.\nDONE: all 3 tests pass"])],
    )
    .await;
    env.wait_for("main", r#"kind="report""#).await;
    let conclusion = env.written_input("main", r#"kind="report""#).await;
    assert_eq!(
        match &conclusion.blocks[0] {
            InputBlock::Raw { text, .. } => text.clone(),
        },
        texts::conclusion(
            "main",
            &create.branch,
            evaluation,
            "done",
            "DONE: all 3 tests pass"
        )
    );
    env.close().await;

    // An earlier conclusion line loses to the last one.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    line_turn(
        &env,
        &create.branch,
        "eval-1",
        &[(
            "r1",
            &["DONE: early\nnot yet\nBLOCKED: the build host is offline"],
        )],
    )
    .await;
    let blocked = env.wait_for("main", r#"op="blocked""#).await;
    assert_eq!(
        blocked.text,
        texts::evaluated("main", "blocked", evaluation, "the build host is offline")
    );
    env.close().await;

    // A multiline CONTINUE is reported whole.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    line_turn(
        &env,
        &create.branch,
        "eval-1",
        &[("r1", &["x\nCONTINUE:\n- a: <out>\n- b"])],
    )
    .await;
    env.wait_for("main", r#"kind="report""#).await;
    let conclusion = env.written_input("main", r#"kind="report""#).await;
    assert_eq!(
        match &conclusion.blocks[0] {
            InputBlock::Raw { text, .. } => text.clone(),
        },
        texts::conclusion(
            "main",
            &create.branch,
            evaluation,
            "continue",
            "CONTINUE:\n- a: <out>\n- b"
        )
    );
    env.close().await;

    // A lowercase prefix is no conclusion, and the whole reply is cut to
    // 4000 characters.
    let env = Env::start().await;
    let (_, create) = completion_requested(&env).await;
    let first: &[&str] = &["not done yet"];
    let pending = pending_refs(&env, &create.branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    env.turn_steps(
        &create.branch,
        "eval-1",
        &pending,
        "completed",
        &[("r1", first)],
    );
    env.wait_for(&create.branch, r#"kind="evaluate-again""#)
        .await;
    let whole = format!(
        "Done: a lowercase prefix is not a conclusion {}",
        "z".repeat(5000)
    );
    let second: &[&str] = &[&whole[..100], &whole[100..]];
    let pending = pending_refs(&env, &create.branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    env.turn_steps(
        &create.branch,
        "eval-2",
        &pending,
        "completed",
        &[("r1", second)],
    );
    env.wait_for("main", r#"kind="report""#).await;
    let conclusion = env.written_input("main", r#"kind="report""#).await;
    let InputBlock::Raw { text, .. } = &conclusion.blocks[0];
    assert!(text.contains(r#"result="continue""#), "{text}");
    let body = text
        .split_once("Evaluator report:\n")
        .and_then(|(_, rest)| rest.strip_suffix("\n</dim-goal>"))
        .expect("a report body");
    assert_eq!(body.chars().count(), 4000);
    assert!(
        body.starts_with("Done: a lowercase prefix"),
        "{}",
        &body[..40]
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_1_a_completion_request_creates_one_evaluator_and_continues_nothing() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    assert_eq!(env.creates("main").await.len(), 1);
    assert!(
        matches!(create.at, BranchCreateAt::Create {}),
        "the evaluator opens at the head the request ended at"
    );
    assert_eq!(
        create.prompt,
        texts::evaluator_prompt("main", evaluation, "ship it", "all tests pass")
    );
    assert_eq!(
        env.settled("main").await.len(),
        3,
        "the request's turn writes no continuation"
    );
    assert!(
        writes_with(&env, "main", r#"by="evaluator""#)
            .await
            .is_empty(),
        "the goal stays active until the evaluator decides"
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_1_the_evaluator_done_completes_the_goal_by_the_evaluator() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "done",
        "DONE: all 3 tests pass",
    );
    let completed = env.wait_for("main", r#"by="evaluator""#).await;
    assert_eq!(
        completed.text,
        texts::evaluated("main", "completed", evaluation, "all 3 tests pass")
    );
    // The completed goal writes no continuation at the next turn end.
    let before = env.settled("main").await.len();
    env.turn_start("main", "next");
    env.end_turn("main", "next", "completed");
    assert_eq!(env.settled("main").await.len(), before);
    env.close().await;
}

#[tokio::test]
async fn pursuit_13_e1_the_evaluator_role_comes_from_the_line_source_not_its_prompt() {
    // A host line that is not an evaluation reports nothing, whatever its
    // prompt says.
    let env = Env::start().await;
    let foreign = env
        .open_branch(
            "main",
            &texts::evaluator_prompt("main", 7, "ship it", "all tests pass"),
            BranchSource::Host {
                operation_id: "goal:other:7".into(),
            },
        )
        .await;
    line_turn(&env, &foreign, "t1", &[("r1", &["DONE: everything"])]).await;
    assert!(
        env.settled("main").await.is_empty(),
        "a host line that is not an evaluation reports nothing"
    );
    assert!(
        env.writes(&foreign).await.is_empty(),
        "the foreign line writes nothing"
    );
    env.close().await;

    // The evaluator line's report reaches the branch it evaluates.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    line_turn(&env, &create.branch, "t1", &[("r1", &["DONE: all good"])]).await;
    env.wait_for("main", r#"kind="report""#).await;
    let report = env.written_input("main", r#"kind="report""#).await;
    assert_eq!(
        match &report.blocks[0] {
            InputBlock::Raw { text, .. } => text.clone(),
        },
        texts::conclusion("main", &create.branch, evaluation, "done", "DONE: all good")
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_13_e2_a_decided_evaluation_ends_its_evaluator_once() {
    for result in ["done", "blocked", "continue", "failed"] {
        let env = Env::start().await;
        let (evaluation, create) = completion_requested(&env).await;
        let evaluator = create.branch.clone();
        write_conclusion(
            &env,
            &evaluator,
            evaluation,
            result,
            &format!("{result} report"),
        );
        tokio::time::timeout(Duration::from_secs(15), async {
            while env.ended("main").await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the evaluator ends");
        let ended = env.ended("main").await;
        assert_eq!(ended.len(), 1, "{result}");
        assert_eq!(ended[0].branch, evaluator, "{result}");
        assert_eq!(
            ended[0].source,
            BranchSource::Host {
                operation_id: format!("goal:evaluate:{evaluation}")
            },
            "{result}"
        );
        env.goal("pause", json!({})).await.expect("pause writes");
        env.wait_for("main", r#"op="pause""#).await;
        assert_eq!(env.ended("main").await.len(), 1, "{result}: ended once");
        env.close().await;
    }
}

#[tokio::test]
async fn pursuit_13_e2_a_superseded_evaluation_ends_its_evaluator() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    env.goal("pause", json!({})).await.expect("pause writes");
    tokio::time::timeout(Duration::from_secs(15), async {
        while env.ended("main").await.is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the evaluator ends");
    let ended = env.ended("main").await;
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].branch, create.branch);
    assert_eq!(
        ended[0].source,
        BranchSource::Host {
            operation_id: format!("goal:evaluate:{evaluation}")
        }
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_13_e3_an_outside_end_before_the_conclusion_fails_the_evaluation() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    env.end_branch(
        &create.branch,
        BranchSource::Host {
            operation_id: "user".into(),
        },
    )
    .await;
    let pause = env.wait_for("main", r#"reason="evaluation_failed""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "evaluation_failed",
            "the evaluator could not check the result",
            evaluation
        )
    );
    let ended = env.ended("main").await;
    assert!(
        !ended.iter().any(|end| end.source
            == BranchSource::Host {
                operation_id: format!("goal:evaluate:{evaluation}")
            }),
        "an ended line is not ended twice"
    );

    // A late conclusion decides nothing.
    let before = env.settled("main").await.len();
    write_conclusion(&env, &create.branch, evaluation, "done", "DONE: late");
    assert_eq!(env.settled("main").await.len(), before + 1);
    env.close().await;
}

#[tokio::test]
async fn pursuit_13_e3_an_outside_end_after_the_conclusion_decides_nothing() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(&env, &create.branch, evaluation, "done", "DONE: verified");
    env.wait_for("main", r#"by="evaluator""#).await;
    let ended = env.ended("main").await;
    assert_eq!(ended.len(), 1, "the conclusion ends the evaluator");
    let before = env.settled("main").await.len();
    env.write_message(
        "main",
        vec![raw(
            "<dim-goal kind=\"report\" result=\"done\" evaluation=\"1\" evaluator=\"E\" branch=\"main\">\nEvaluator report:\nDONE: ignored\n</dim-goal>",
            None,
            false,
        )],
        InputMode::Append,
    );
    env.emit(
        "main",
        dim_agent_types::EventKind::BranchEnded(BranchEndedData {
            branch: create.branch.clone(),
            end_event: 0,
            source: BranchSource::Host {
                operation_id: "user".into(),
            },
            ancestor: None,
        }),
    );
    assert_eq!(env.settled("main").await.len(), before + 1);
    env.close().await;
}

#[tokio::test]
async fn pursuit_4_5_a_report_without_a_valid_evaluation_or_evaluator_decides_nothing() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    let evaluator = create.branch.clone();
    let writes = [
        format!(r#"result="done" evaluator="{evaluator}""#),
        format!(r#"result="done" evaluation="abc" evaluator="{evaluator}""#),
        format!(r#"result="done" evaluation="{evaluation}""#),
        r#"result="done""#.to_owned(),
    ];
    let before = env.settled("main").await.len();
    for attrs in &writes {
        let text = format!(
            "<dim-goal kind=\"report\" {attrs} branch=\"main\">\nEvaluator report:\nDONE: x\n</dim-goal>"
        );
        env.write_message("main", vec![raw(&text, None, false)], InputMode::Append);
    }
    assert_eq!(
        env.settled("main").await.len(),
        before + writes.len(),
        "no malformed report decides anything"
    );
    assert!(
        writes_with(&env, "main", r#"by="evaluator""#)
            .await
            .is_empty(),
        "no malformed report completes the goal"
    );

    write_conclusion(
        &env,
        &evaluator,
        evaluation,
        "done",
        "DONE: all 3 tests pass",
    );
    let completed = env.wait_for("main", r#"by="evaluator""#).await;
    assert_eq!(
        completed.text,
        texts::evaluated("main", "completed", evaluation, "all 3 tests pass")
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_4_a_report_outside_the_first_block_decides_nothing() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    let report = texts::conclusion("main", &create.branch, evaluation, "done", "DONE: later");
    env.write_message(
        "main",
        vec![
            raw("a note before the report", None, false),
            raw(&report, None, false),
        ],
        InputMode::Append,
    );
    assert!(
        env.settled("main")
            .await
            .iter()
            .all(|block| !block.text.contains(r#"by="evaluator""#)),
        "no conclusion after the first block"
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_2_a_continue_conclusion_writes_one_continuation_or_pauses_at_the_limit() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "continue",
        "CONTINUE:\n- two tests fail: <output>",
    );
    let needle = format!(
        r#"kind="continuation" count="2" limit="20" after="{evaluation}" evaluation="{evaluation}""#
    );
    let continuation = env.wait_for("main", &needle).await;
    assert_eq!(
        continuation.text,
        texts::evaluated_continuation("main", 2, 20, evaluation, evaluation)
    );
    env.close().await;

    let env = Env::start().await;
    env.set_limit(1);
    let (evaluation, create) = completion_requested_with(&env, 1).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "continue",
        "CONTINUE:\n- still missing",
    );
    let pause = env.wait_for("main", r#"reason="limit""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "limit",
            "the automatic continuation limit was reached",
            evaluation
        )
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_5_a_failed_conclusion_or_a_failed_creation_gives_one_pause() {
    // A failed conclusion.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    write_conclusion(
        &env,
        &create.branch,
        evaluation,
        "failed",
        "the evaluator's turn did not complete",
    );
    let pause = env.wait_for("main", r#"reason="evaluation_failed""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "evaluation_failed",
            "the evaluator could not check the result",
            evaluation
        )
    );
    env.close().await;

    // A creation that failed after the fact.
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    let create_event = env
        .all_events("main")
        .await
        .into_iter()
        .find(|event| matches!(event.kind, dim_agent_types::EventKind::BranchCreate(_)))
        .expect("the create fact")
        .sequence;
    env.emit(
        "main",
        dim_agent_types::EventKind::BranchCreateFailed(BranchCreateFailedData::Failed {
            branch: create.branch.clone(),
            create_event,
            error: "branch creation failed".into(),
        }),
    );
    let pause = env.wait_for("main", r#"reason="evaluation_failed""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "evaluation_failed",
            "the evaluator could not check the result",
            evaluation
        )
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_8_the_evaluator_reports_the_streamed_conclusion_of_its_final_request() {
    let env = Env::start().await;
    let (evaluation, create) = completion_requested(&env).await;
    let first: &[&str] = &["DONE: stale"];
    let second: &[&str] = &["checked the tree\nDONE", ": all 3 tests pass"];
    let pending = pending_refs(&env, &create.branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    env.turn_steps(
        &create.branch,
        "eval-1",
        &pending,
        "completed",
        &[("r1", first), ("r2", second)],
    );
    env.wait_for("main", r#"kind="report""#).await;
    let report = env.written_input("main", r#"kind="report""#).await;
    assert_eq!(
        match &report.blocks[0] {
            InputBlock::Raw { text, .. } => text.clone(),
        },
        texts::conclusion(
            "main",
            &create.branch,
            evaluation,
            "done",
            "DONE: all 3 tests pass"
        )
    );
    assert!(
        env.writes(&create.branch).await.len() == 1,
        "the evaluator line only carries its inherited clear"
    );
    env.close().await;
}

#[tokio::test]
async fn pursuit_4_the_evaluator_is_asked_once_then_its_whole_reply_is_the_report() {
    let env = Env::start().await;
    let (_, create) = completion_requested(&env).await;
    let pending = pending_refs(&env, &create.branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    let first: &[&str] = &["not done yet"];
    let end = env.turn_steps(
        &create.branch,
        "eval-1",
        &pending,
        "completed",
        &[("r1", first)],
    );
    let again = env
        .wait_for(&create.branch, r#"kind="evaluate-again""#)
        .await;
    assert_eq!(
        again.text,
        texts::evaluate_again(&create.branch, end.sequence)
    );
    assert_eq!(again.mode, InputMode::Queue);

    let whole = format!("still working {}", "y".repeat(5000));
    let second: &[&str] = &[&whole[..100], &whole[100..]];
    let pending = pending_refs(&env, &create.branch).await;
    let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
    env.turn_steps(
        &create.branch,
        "eval-2",
        &pending,
        "completed",
        &[("r1", second)],
    );
    env.wait_for("main", r#"kind="report""#).await;
    let report = env.written_input("main", r#"kind="report""#).await;
    let InputBlock::Raw { text, .. } = &report.blocks[0];
    assert!(text.contains(r#"result="continue""#), "{text}");
    let body = text
        .split_once("Evaluator report:\n")
        .and_then(|(_, rest)| rest.strip_suffix("\n</dim-goal>"))
        .expect("a report body");
    assert_eq!(body.chars().count(), 4000);
    assert!(body.starts_with("still working y"), "{}", &body[..40]);
    env.close().await;
}

#[tokio::test]
async fn pursuit_5_a_failed_or_cancelled_evaluator_turn_reports_failed() {
    for status in ["failed", "cancelled"] {
        let env = Env::start().await;
        let (evaluation, create) = completion_requested(&env).await;
        let pending = pending_refs(&env, &create.branch).await;
        let pending: Vec<&str> = pending.iter().map(String::as_str).collect();
        env.turn_steps(&create.branch, "eval-1", &pending, status, &[]);
        env.wait_for("main", r#"kind="report""#).await;
        let report = env.written_input("main", r#"kind="report""#).await;
        assert_eq!(
            match &report.blocks[0] {
                InputBlock::Raw { text, .. } => text.clone(),
            },
            texts::conclusion(
                "main",
                &create.branch,
                evaluation,
                "failed",
                "the evaluator's turn did not complete"
            ),
            "{status}"
        );
        let pause = env.wait_for("main", r#"reason="evaluation_failed""#).await;
        assert_eq!(
            pause.text,
            texts::automatic_pause(
                "main",
                "evaluation_failed",
                "the evaluator could not check the result",
                evaluation
            ),
            "{status}"
        );
        env.close().await;
    }
}
