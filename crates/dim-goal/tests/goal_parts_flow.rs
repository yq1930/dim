//! Goal's cases over the no-Loop part harness: the real Dispatcher,
//! Timeline, TimelineTree, SessionMethods, ToolRegistry, PromptGuide,
//! storage and Settings, with the facts the Loop would write supplied by the
//! test. Every expectation is a literal text or a written input.
mod support;

use dim_agent_types::{InputMode, ToolContent, ToolResult};
use serde_json::json;
use support::parts::*;
use support::texts;

fn tool_text(result: &ToolResult) -> String {
    match &result.content {
        ToolContent::Text(text) => text.clone(),
        ToolContent::Blocks(_) => panic!("a text result"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn goal_reads_current_settings_and_stops_decisions_on_read_failure() {
    let env = Env::start().await;
    env.set_limit(1);
    let set = env.set("ship it").await;
    let end = env.turn("main", &[&set]);
    let continuation = env.wait_for("main", r#"kind="continuation""#).await;
    assert_eq!(
        continuation.text,
        texts::continuation("main", 1, 1, end.sequence)
    );
    assert_eq!(
        env.writes("main").await.len(),
        2,
        "set plus one continuation"
    );

    // An unreadable document produces no automatic input.
    std::fs::write(env.home().join("goal.json"), r#"{"maxContinuations":0}"#).unwrap();
    env.turn("main", &[&continuation]);
    assert_eq!(
        env.settled("main").await.len(),
        2,
        "an unreadable limit must not produce an automatic input"
    );

    let interrupt = env.interrupt("main");
    let pause = env.wait_for("main", r#"reason="interrupted""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "interrupted",
            "the user interrupted the turn",
            interrupt.sequence
        )
    );
    env.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_line_without_a_goal_reads_no_settings_at_its_turn_ends() {
    let env = Env::start().await;
    env.set_limit(1);
    for step in 0..3 {
        let input = env.emit("main", user(&format!("step {step}"), InputMode::Queue));
        env.turn_ids("main", &[&input.id]);
    }
    assert!(
        env.settled("main").await.is_empty(),
        "no goal, so nothing is written"
    );
    env.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_goal_update_tool_writes_its_report_and_refuses_bad_arguments() {
    let env = Env::start().await;
    let set = env.set("ship it").await;
    assert_eq!(set.blocks.len(), 2);

    let refused = env
        .tool("main", json!({"status": "elsewhere", "summary": "x"}))
        .await;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(tool_text(&refused), texts::TOOL_INVALID);
    let refused = env.tool("main", json!({"status": "completed"})).await;
    assert_eq!(refused.is_error, Some(true));
    assert_eq!(tool_text(&refused), texts::TOOL_INVALID);
    assert_eq!(env.settled("main").await.len(), 1, "refusals write nothing");

    let accepted = env
        .tool(
            "main",
            json!({"status": "completed", "summary": "all done"}),
        )
        .await;
    assert_eq!(accepted.is_error, None);
    assert_eq!(tool_text(&accepted), "goal_update recorded: completed.");
    let report = env.wait_for("main", r#"op="completed""#).await;
    assert_eq!(report.text, texts::report("main", "completed", "all done"));
    assert_eq!(report.mode, InputMode::Append);

    let accepted = env
        .tool("main", json!({"status": "blocked", "summary": "need keys"}))
        .await;
    assert_eq!(tool_text(&accepted), "goal_update recorded: blocked.");
    let report = env.wait_for("main", r#"op="blocked""#).await;
    assert_eq!(report.text, texts::report("main", "blocked", "need keys"));
    env.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_guide_entry_and_the_tool_declaration_are_the_shipped_texts() {
    let env = Env::start().await;
    let guide = env.guide("main");
    assert!(guide.contains(texts::ENTRY), "{guide:?}");
    let definitions = env.definitions();
    let tool = definitions
        .iter()
        .find(|definition| definition.name == "goal_update")
        .unwrap_or_else(|| panic!("no goal_update: {definitions:#?}"));
    assert_eq!(tool.description, texts::TOOL_DESCRIPTION);
    assert_eq!(
        serde_json::to_value(&tool.input_schema).unwrap(),
        json!({
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["completed", "blocked"]},
                "summary": {"type": "string", "minLength": 1, "maxLength": 2000},
            },
            "required": ["status", "summary"],
            "additionalProperties": false,
        })
    );
    env.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_completed_turn_drives_the_continuation_and_the_limit_pauses_it() {
    let env = Env::start().await;
    env.set_limit(1);
    let set = env.set("ship it").await;
    let end = env.turn("main", &[&set]);
    let continuation = env.wait_for("main", r#"kind="continuation""#).await;
    assert_eq!(
        continuation.text,
        texts::continuation("main", 1, 1, end.sequence)
    );
    let end = env.turn("main", &[&continuation]);
    let pause = env.wait_for("main", r#"reason="limit""#).await;
    assert_eq!(
        pause.text,
        texts::automatic_pause(
            "main",
            "limit",
            "the automatic continuation limit was reached",
            end.sequence
        )
    );
    assert_eq!(env.settled("main").await.len(), 3);
    env.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_reopens_the_goal_and_the_count_restarts() {
    let env = Env::start().await;
    env.set_limit(2);
    let set = env.set("ship it").await;
    let end = env.turn("main", &[&set]);
    let first = env.wait_for("main", r#"kind="continuation""#).await;
    assert_eq!(first.text, texts::continuation("main", 1, 2, end.sequence));

    env.tool("main", json!({"status": "completed", "summary": "done"}))
        .await;
    let report = env.wait_for("main", r#"op="completed""#).await;
    assert_eq!(report.text, texts::report("main", "completed", "done"));
    env.turn("main", &[&first]);
    assert_eq!(
        env.settled("main").await.len(),
        3,
        "a completed goal writes no continuation"
    );

    env.goal("resume", json!({})).await.expect("resume writes");
    let resume = env.wait_for("main", r#"op="resume""#).await;
    assert_eq!(resume.text, texts::resume("main"));
    let end = env.turn("main", &[&report, &resume]);
    let again = env
        .wait_for(
            "main",
            &format!(
                r#"kind="continuation" count="1" limit="2" after="{}""#,
                end.sequence
            ),
        )
        .await;
    assert_eq!(
        again.text,
        texts::continuation("main", 1, 2, end.sequence),
        "resume restarts the count"
    );
    env.close().await;
}
