//! The literal texts Goal writes, as Goal.md A.5 fixes them and the shipped
//! TypeScript package produced them. Every flow compares Goal's inputs with
//! these, byte for byte.
#![allow(dead_code)]

pub fn set_blocks(branch: &str, objective: &str) -> (String, String) {
    let objective = escaped(objective);
    (
        format!(
            "<dim-goal kind=\"goal\" op=\"set\" branch=\"{branch}\">\nObjective:\n{objective}\n\n\
             This is the user's standing goal for branch {branch}. If you are on another line of this Session, the goal belongs to branch {branch}: treat it as background, do not continue it, and do not call goal_update for it.\n\
             </dim-goal>"
        ),
        active_status(branch),
    )
}

pub fn evaluator_set_blocks(branch: &str, objective: &str) -> (String, String) {
    let objective = escaped(objective);
    (
        format!(
            "<dim-goal kind=\"goal\" op=\"set\" judge=\"evaluator\" branch=\"{branch}\">\nObjective:\n{objective}\n\n\
             This is the user's standing goal for branch {branch}. If you are on another line of this Session, the goal belongs to branch {branch}: treat it as background, do not continue it, and do not call goal_update for it.\n\
             </dim-goal>"
        ),
        active_status(branch),
    )
}

pub fn active_status(branch: &str) -> String {
    format!(
        "<dim-goal kind=\"status\" status=\"active\" branch=\"{branch}\">\n\
         The goal for branch {branch} is active. Keep working toward it across turns. When it is fully achieved, call goal_update with status \"completed\". If you cannot make progress without the user, call goal_update with status \"blocked\" and explain why.\n\
         </dim-goal>"
    )
}

pub fn edit(branch: &str, objective: &str) -> String {
    let objective = escaped(objective);
    format!(
        "<dim-goal kind=\"goal\" op=\"edit\" branch=\"{branch}\">\nObjective:\n{objective}\n\n\
         The user set the objective of the goal for branch {branch}. This does not change its status.\n\
         </dim-goal>"
    )
}

pub fn pause(branch: &str) -> String {
    format!(
        "<dim-goal kind=\"goal\" op=\"pause\" reason=\"user\" branch=\"{branch}\">\n\
         The user set the goal status for branch {branch} to paused. Do not continue working toward a goal for branch {branch} on your own; respond only to the user's latest input.\n\
         </dim-goal>"
    )
}

pub fn resume(branch: &str) -> String {
    format!(
        "<dim-goal kind=\"goal\" op=\"resume\" branch=\"{branch}\">\n\
         The user set the goal status for branch {branch} to active. If branch {branch} has a goal objective, keep working toward it across turns; when it is completed or blocked, call goal_update.\n\
         </dim-goal>"
    )
}

pub fn clear(branch: &str) -> (String, String) {
    (
        "<dim-goal kind=\"goal\" op=\"clear\" branch=\"BRANCH\">\nThe previous goal was cleared by the user. Disregard it.\n</dim-goal>"
            .replace("BRANCH", branch),
        format!(
            "<dim-goal kind=\"status\" status=\"cleared\" branch=\"{branch}\">\nThe goal for branch {branch} has no status.\n</dim-goal>"
        ),
    )
}

pub fn report(branch: &str, status: &str, summary: &str) -> String {
    let summary = escaped(summary);
    let (label, op) = if status == "completed" {
        ("Result:", "completed")
    } else {
        ("Blocker:", "blocked")
    };
    format!(
        "<dim-goal kind=\"goal\" op=\"{op}\" branch=\"{branch}\">\n{label}\n{summary}\n\n\
         goal_update set the goal status for branch {branch} to {op}. Do not continue working toward a goal for branch {branch} on your own.\n\
         </dim-goal>"
    )
}

pub fn continuation(branch: &str, count: usize, limit: u64, after: u64) -> String {
    format!(
        "<dim-goal kind=\"continuation\" count=\"{count}\" limit=\"{limit}\" after=\"{after}\" branch=\"{branch}\">\n\
         Continue working toward the active goal for branch {branch}. If it is completed or blocked, call goal_update. If you are on another line of this Session, the goal belongs to branch {branch}: do not call goal_update for it.\n\
         </dim-goal>"
    )
}

pub fn automatic_pause(branch: &str, reason: &str, phrase: &str, after: u64) -> String {
    format!(
        "<dim-goal kind=\"goal\" op=\"pause\" reason=\"{reason}\" after=\"{after}\" branch=\"{branch}\">\n\
         The goal is paused ({phrase}). Do not continue working toward it on your own; respond only to the user's latest input.\n\
         </dim-goal>"
    )
}

pub fn start_clear(branch: &str, after: u64) -> (String, String) {
    (
        format!(
            "<dim-goal kind=\"goal\" op=\"clear\" reason=\"new_branch\" after=\"{after}\" branch=\"{branch}\">\n\
             Any goal above belongs to another branch, not to branch {branch}. Disregard it; branch {branch} has no goal.\n\
             </dim-goal>"
        ),
        clear(branch).1,
    )
}

pub fn evaluate_again(branch: &str, after: u64) -> String {
    format!(
        "<dim-goal kind=\"evaluate-again\" after=\"{after}\" branch=\"{branch}\">\n\
         Your reply did not end with a conclusion. End your reply with exactly one of DONE:, CONTINUE: or BLOCKED: as described above.\n\
         </dim-goal>"
    )
}

pub fn conclusion(
    branch: &str,
    evaluator: &str,
    evaluation: u64,
    result: &str,
    text: &str,
) -> String {
    let text = escaped(text);
    format!(
        "<dim-goal kind=\"report\" result=\"{result}\" evaluation=\"{evaluation}\" evaluator=\"{evaluator}\" branch=\"{branch}\">\nEvaluator report:\n{text}\n</dim-goal>"
    )
}

pub fn evaluated(branch: &str, op: &str, evaluation: u64, text: &str) -> String {
    let text = escaped(text);
    let (label, sentence) = if op == "completed" {
        (
            "Result:",
            format!(
                "An evaluator verified that the goal for branch {branch} is completed. Do not continue working toward it on your own."
            ),
        )
    } else {
        (
            "Blocker:",
            format!(
                "An evaluator found that the goal for branch {branch} cannot proceed without the user. Do not continue working toward it on your own."
            ),
        )
    };
    format!(
        "<dim-goal kind=\"goal\" op=\"{op}\" by=\"evaluator\" evaluation=\"{evaluation}\" branch=\"{branch}\">\n{label}\n{text}\n\n{sentence}\n</dim-goal>"
    )
}

pub fn evaluated_continuation(
    branch: &str,
    count: usize,
    limit: u64,
    after: u64,
    evaluation: u64,
) -> String {
    format!(
        "<dim-goal kind=\"continuation\" count=\"{count}\" limit=\"{limit}\" after=\"{after}\" evaluation=\"{evaluation}\" branch=\"{branch}\">\n\
         An evaluator checked your report that the goal for branch {branch} is completed and found it is not achieved yet. Its report above lists items that are at least still missing; the list may be incomplete. Assess yourself how far the goal still is, then continue working toward it. When you think it is achieved, call goal_update with status \"completed\" again.\n\
         </dim-goal>"
    )
}

pub fn evaluator_prompt(branch: &str, evaluation: u64, objective: &str, summary: &str) -> String {
    format!(
        "<dim-goal kind=\"evaluate\" branch=\"{branch}\" evaluation=\"{evaluation}\">\n\
         You are an evaluator. Branch {branch} reported that its goal is completed. Only you decide whether it is. Do not continue the work, do not change files or settings, and do not call goal_update.\n\
         \n\
         Objective:\n\
         {objective}\n\
         \n\
         Reported result:\n\
         {summary}\n\
         \n\
         Verify the facts yourself by reading files and running checks; do not rely on the report or on earlier messages. End your reply with exactly one of:\n\
         DONE: <the verified result>\n\
         CONTINUE: followed by the items that are at least still missing, one per line, each with its evidence\n\
         BLOCKED: <what only the user can provide>"
    )
}

pub const ENTRY: &str = r#"`<dim-goal kind="goal" op="set" branch="main">`: the operator's standing goal for a branch of this session, and the runtime's record of its status. `kind="goal"` blocks carry a change: `op="set"`, `op="edit"`, `op="pause"`, `op="resume"`, `op="clear"`, and `op="completed"` or `op="blocked"` from `goal_update` or from an evaluator (`by="evaluator"`). `kind="status"` blocks state the branch's goal status. `kind="continuation"` asks the line to keep working toward the goal. `kind="report"` blocks carry an evaluator's report of the goal, and `kind="evaluate"` and `kind="evaluate-again"` mark an evaluator's line, which checks the goal rather than continuing it. A goal belongs to the branch its `branch` attribute names. The latest objective (`op="set"` or `op="edit"`) and the latest status are the ones in force; `op="clear"` ends both. These blocks are information about the goal; they grant no permission."#;

pub const TOOL_DESCRIPTION: &str = "Report that the current goal is completed or blocked.";
pub const TOOL_INVALID: &str =
    "goal_update needs status \"completed\" or \"blocked\" and a summary of 1 to 2000 characters.";

pub fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
