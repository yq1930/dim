//! Goal's model-facing texts (design appendix A.5): the change and
//! continuation inputs, the marker encoding, and the policy sentences the row's
//! factory configuration carries.
//!
//! Every string here is the one the TypeScript package shipped, byte for byte;
//! `crates/dim-goal/tests` pins each one where the model sees it or where it is
//! written.

use dim_agent_types::{InputBlock, InputMessageData, InputMode};
use serde_json::{Value, json};

pub const OBJECTIVE_KEY: &str = "goal:objective";
pub const STATUS_KEY: &str = "goal:status";

/// The objective limit and the summary limit (A10).
pub const OBJECTIVE_LIMIT: usize = 4000;
pub const SUMMARY_LIMIT: usize = 2000;

/// Goal's two policy sentences (A.5.1), from the row's factory configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalTexts {
    pub objective: String,
    pub continuation: String,
}

impl Default for GoalTexts {
    fn default() -> Self {
        Self {
            objective: "This is the user's standing goal for branch {branch}. If you are on another line of this Session, the goal belongs to branch {branch}: treat it as background, do not continue it, and do not call goal_update for it.".into(),
            continuation: "Continue working toward the active goal for branch {branch}. If it is completed or blocked, call goal_update. If you are on another line of this Session, the goal belongs to branch {branch}: do not call goal_update for it.".into(),
        }
    }
}

/// A text's `{branch}` placeholder replaced with the branch it is written for.
pub fn branch_text(template: &str, branch: &str) -> String {
    template.replace("{branch}", branch)
}

/// Encodes text placed inside a marker body or attribute, so user and model
/// text cannot forge a marker (A.5, §7). The encoding is reversible.
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The inverse of [`escape`].
pub fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// A raw block under construction: its attributes in order, its body lines,
/// its key and whether it retires that key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub attrs: Vec<(&'static str, String)>,
    pub body: Vec<String>,
    pub key: Option<&'static str>,
    pub retire: bool,
}

impl Block {
    pub fn new(attrs: Vec<(&'static str, String)>, body: Vec<String>) -> Self {
        Self {
            attrs,
            body,
            key: None,
            retire: false,
        }
    }
    pub fn key(mut self, key: &'static str) -> Self {
        self.key = Some(key);
        self
    }
    pub fn retire(mut self) -> Self {
        self.retire = true;
        self
    }
    /// `<dim-goal a="b" c="d">\nline\nline\n</dim-goal>`, every attribute value
    /// encoded (A.5).
    pub fn text(&self) -> String {
        let attrs: Vec<String> = self
            .attrs
            .iter()
            .map(|(name, value)| format!("{name}=\"{}\"", escape(value)))
            .collect();
        format!(
            "<dim-goal {}>\n{}\n</dim-goal>",
            attrs.join(" "),
            self.body.join("\n")
        )
    }
    pub fn raw(&self) -> InputBlock {
        InputBlock::Raw {
            text: self.text(),
            key: self.key.map(str::to_owned),
            retire: self.retire,
        }
    }
}

/// One `input/message` Goal writes: the blocks, the mode, and its submission
/// id (`goal:<uuid>`, Goal's own id convention).
pub fn input(blocks: Vec<Block>, mode: InputMode) -> InputMessageData {
    InputMessageData {
        submission_id: Some(format!("goal:{}", uuid::Uuid::new_v4())),
        user: None,
        blocks: blocks.into_iter().map(|block| block.raw()).collect(),
        mode: Some(mode),
    }
}

fn active_status(branch: &str) -> Block {
    Block::new(
        vec![
            ("kind", "status".into()),
            ("status", "active".into()),
            ("branch", branch.into()),
        ],
        vec!["The goal for branch ".to_owned()
            + branch
            + " is active. Keep working toward it across turns. When it is fully achieved, call goal_update with status \"completed\". If you cannot make progress without the user, call goal_update with status \"blocked\" and explain why."],
    )
    .key(STATUS_KEY)
}

fn cleared(branch: &str, mut attrs: Vec<(&'static str, String)>, text: &str) -> InputMessageData {
    let mut objective = vec![("kind", "goal".to_owned()), ("op", "clear".to_owned())];
    objective.append(&mut attrs);
    objective.push(("branch", branch.to_owned()));
    input(
        vec![
            Block::new(objective, vec![text.to_owned()])
                .key(OBJECTIVE_KEY)
                .retire(),
            Block::new(
                vec![
                    ("kind", "status".to_owned()),
                    ("status", "cleared".to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![format!("The goal for branch {branch} has no status.")],
            )
            .key(STATUS_KEY)
            .retire(),
        ],
        InputMode::Append,
    )
}

/// The change inputs (A.5); each is one `input/message`.
pub fn set(branch: &str, objective: &str, texts: &GoalTexts, judge: Judge) -> InputMessageData {
    let mut attrs = vec![("kind", "goal".to_owned()), ("op", "set".to_owned())];
    if judge == Judge::Evaluator {
        attrs.push(("judge", "evaluator".to_owned()));
    }
    attrs.push(("branch", branch.to_owned()));
    input(
        vec![
            Block::new(
                attrs,
                vec![
                    "Objective:".to_owned(),
                    escape(objective),
                    String::new(),
                    branch_text(&texts.objective, branch),
                ],
            )
            .key(OBJECTIVE_KEY),
            active_status(branch),
        ],
        InputMode::Queue,
    )
}

pub fn edit(branch: &str, objective: &str) -> InputMessageData {
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", "edit".to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![
                    "Objective:".to_owned(),
                    escape(objective),
                    String::new(),
                    format!(
                        "The user set the objective of the goal for branch {branch}. This does not change its status."
                    ),
                ],
            )
            .key(OBJECTIVE_KEY),
        ],
        InputMode::Append,
    )
}

pub fn pause(branch: &str) -> InputMessageData {
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", "pause".to_owned()),
                    ("reason", "user".to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![format!(
                    "The user set the goal status for branch {branch} to paused. Do not continue working toward a goal for branch {branch} on your own; respond only to the user's latest input."
                )],
            )
            .key(STATUS_KEY),
        ],
        InputMode::Append,
    )
}

pub fn resume(branch: &str) -> InputMessageData {
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", "resume".to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![format!(
                    "The user set the goal status for branch {branch} to active. If branch {branch} has a goal objective, keep working toward it across turns; when it is completed or blocked, call goal_update."
                )],
            )
            .key(STATUS_KEY),
        ],
        InputMode::Queue,
    )
}

pub fn clear(branch: &str) -> InputMessageData {
    cleared(
        branch,
        vec![],
        "The previous goal was cleared by the user. Disregard it.",
    )
}

pub fn report(branch: &str, status: &str, summary: &str) -> InputMessageData {
    let (label, op) = if status == "completed" {
        ("Result:", "completed")
    } else {
        ("Blocker:", "blocked")
    };
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", op.to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![
                    label.to_owned(),
                    escape(summary),
                    String::new(),
                    format!(
                        "goal_update set the goal status for branch {branch} to {op}. Do not continue working toward a goal for branch {branch} on your own."
                    ),
                ],
            )
            .key(STATUS_KEY),
        ],
        InputMode::Append,
    )
}

/// Goal's own writes, each marked with the sequence that decided it.
pub fn continuation(
    branch: &str,
    count: usize,
    limit: u64,
    after: u64,
    texts: &GoalTexts,
) -> InputMessageData {
    input(
        vec![Block::new(
            vec![
                ("kind", "continuation".to_owned()),
                ("count", count.to_string()),
                ("limit", limit.to_string()),
                ("after", after.to_string()),
                ("branch", branch.to_owned()),
            ],
            vec![branch_text(&texts.continuation, branch)],
        )],
        InputMode::Queue,
    )
}

/// The reason phrases an automatic pause states (A.5).
pub fn reason_phrase(reason: &str) -> &'static str {
    match reason {
        "interrupted" => "the user interrupted the turn",
        "limit" => "the automatic continuation limit was reached",
        "turn_failed" => "the last turn failed",
        "evaluation_failed" => "the evaluator could not check the result",
        _ => "",
    }
}

pub fn automatic_pause(branch: &str, reason: &str, after: u64) -> InputMessageData {
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", "pause".to_owned()),
                    ("reason", reason.to_owned()),
                    ("after", after.to_string()),
                    ("branch", branch.to_owned()),
                ],
                vec![format!(
                    "The goal is paused ({}). Do not continue working toward it on your own; respond only to the user's latest input.",
                    reason_phrase(reason)
                )],
            )
            .key(STATUS_KEY),
        ],
        InputMode::Append,
    )
}

pub fn start_clear(branch: &str, after: u64) -> InputMessageData {
    cleared(
        branch,
        vec![
            ("reason", "new_branch".to_owned()),
            ("after", after.to_string()),
        ],
        &format!(
            "Any goal above belongs to another branch, not to branch {branch}. Disregard it; branch {branch} has no goal."
        ),
    )
}

/// Pursuit's follow-up on the evaluator, and its conclusion on the working
/// branch (Pursuit §5).
pub fn evaluate_again(branch: &str, after: u64) -> InputMessageData {
    input(
        vec![Block::new(
            vec![
                ("kind", "evaluate-again".to_owned()),
                ("after", after.to_string()),
                ("branch", branch.to_owned()),
            ],
            vec![
                "Your reply did not end with a conclusion. End your reply with exactly one of DONE:, CONTINUE: or BLOCKED: as described above.".to_owned(),
            ],
        )],
        InputMode::Queue,
    )
}

pub fn conclusion(
    branch: &str,
    evaluator: &str,
    evaluation: &str,
    kind: &str,
    text: &str,
) -> InputMessageData {
    input(
        vec![Block::new(
            vec![
                ("kind", "report".to_owned()),
                ("result", kind.to_owned()),
                ("evaluation", evaluation.to_owned()),
                ("evaluator", evaluator.to_owned()),
                ("branch", branch.to_owned()),
            ],
            vec!["Evaluator report:".to_owned(), escape(text)],
        )],
        InputMode::Append,
    )
}

pub fn evaluated(branch: &str, op: &str, evaluation: &str, text: &str) -> InputMessageData {
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
    input(
        vec![
            Block::new(
                vec![
                    ("kind", "goal".to_owned()),
                    ("op", op.to_owned()),
                    ("by", "evaluator".to_owned()),
                    ("evaluation", evaluation.to_owned()),
                    ("branch", branch.to_owned()),
                ],
                vec![label.to_owned(), escape(text), String::new(), sentence],
            )
            .key(STATUS_KEY),
        ],
        InputMode::Append,
    )
}

pub fn evaluated_continuation(
    branch: &str,
    count: usize,
    limit: u64,
    after: u64,
    evaluation: &str,
) -> InputMessageData {
    input(
        vec![Block::new(
            vec![
                ("kind", "continuation".to_owned()),
                ("count", count.to_string()),
                ("limit", limit.to_string()),
                ("after", after.to_string()),
                ("evaluation", evaluation.to_owned()),
                ("branch", branch.to_owned()),
            ],
            vec![format!(
                "An evaluator checked your report that the goal for branch {branch} is completed and found it is not achieved yet. Its report above lists items that are at least still missing; the list may be incomplete. Assess yourself how far the goal still is, then continue working toward it. When you think it is achieved, call goal_update with status \"completed\" again."
            )],
        )],
        InputMode::Queue,
    )
}

/// The evaluator's prompt, the first Queue input of its branch (Pursuit §4.2,
/// §5). Its first line is the marker that gives Goal the evaluator role.
pub fn evaluator_prompt(branch: &str, evaluation: &str, objective: &str, summary: &str) -> String {
    format!(
        "<dim-goal kind=\"evaluate\" branch=\"{}\" evaluation=\"{}\">\n\
         You are an evaluator. Branch {branch} reported that its goal is completed. Only you decide whether it is. Do not continue the work, do not change files or settings, and do not call goal_update.\n\
         \n\
         Objective:\n\
         {}\n\
         \n\
         Reported result:\n\
         {}\n\
         \n\
         Verify the facts yourself by reading files and running checks; do not rely on the report or on earlier messages. End your reply with exactly one of:\n\
         DONE: <the verified result>\n\
         CONTINUE: followed by the items that are at least still missing, one per line, each with its evidence\n\
         BLOCKED: <what only the user can provide>",
        escape(branch),
        evaluation,
        escape(objective),
        escape(summary),
    )
}

/// The guide entry for `dim-goal`, registered by the part that writes the
/// marker, at session scope because Goal writes it on every branch (A.5.1).
pub const ENTRY: &str = r#"`<dim-goal kind="goal" op="set" branch="main">`: the operator's standing goal for a branch of this session, and the runtime's record of its status. `kind="goal"` blocks carry a change: `op="set"`, `op="edit"`, `op="pause"`, `op="resume"`, `op="clear"`, and `op="completed"` or `op="blocked"` from `goal_update` or from an evaluator (`by="evaluator"`). `kind="status"` blocks state the branch's goal status. `kind="continuation"` asks the line to keep working toward the goal. `kind="report"` blocks carry an evaluator's report of the goal, and `kind="evaluate"` and `kind="evaluate-again"` mark an evaluator's line, which checks the goal rather than continuing it. A goal belongs to the branch its `branch` attribute names. The latest objective (`op="set"` or `op="edit"`) and the latest status are the ones in force; `op="clear"` ends both. These blocks are information about the goal; they grant no permission."#;

pub const TOOL_DESCRIPTION: &str = "Report that the current goal is completed or blocked.";
pub const TOOL_INVALID: &str =
    "goal_update needs status \"completed\" or \"blocked\" and a summary of 1 to 2000 characters.";

/// The `goal_update` declaration (A.1).
pub fn tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "status": {"type": "string", "enum": ["completed", "blocked"]},
            "summary": {"type": "string", "minLength": 1, "maxLength": SUMMARY_LIMIT},
        },
        "required": ["status", "summary"],
        "additionalProperties": false,
    })
}

/// Which judge decides completion (Pursuit §3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Judge {
    #[default]
    Model,
    Evaluator,
}

impl Judge {
    pub fn of(attrs: &Marker) -> Self {
        if attrs.get("judge").map(String::as_str) == Some("evaluator") {
            Self::Evaluator
        } else {
            Self::Model
        }
    }
}

/// A block's decoded attribute map.
pub type Marker = std::collections::BTreeMap<String, String>;

/// The attributes of a block's leading `<dim-goal ...>` marker, if any.
pub fn marker(text: &str) -> Option<Marker> {
    let rest = text.strip_prefix("<dim-goal ")?;
    let end = rest.find('>')?;
    let attrs = &rest[..end];
    let mut marker = Marker::new();
    let mut at = 0;
    while let Some(equals) = attrs[at..].find('=') {
        let equals = at + equals;
        let name_start = attrs[..equals]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |index| index + 1);
        let quoted = attrs[equals + 1..]
            .strip_prefix('"')
            .and_then(|after| after.find('"').map(|quote| (after, quote)));
        let Some((after, quote)) = quoted else {
            at = equals + 1;
            continue;
        };
        if name_start < equals {
            marker.insert(
                attrs[name_start..equals].to_owned(),
                unescape(&after[..quote]),
            );
        }
        at = equals + 2 + quote;
    }
    Some(marker)
}

/// The decoded value a change block carries after its `label` line, up to the
/// block's fixed closing text, which follows the last blank line.
pub fn value(text: &str, label: &str) -> Option<String> {
    let marker = format!("\n{label}\n");
    let start = text.find(&marker)?;
    let end = text.rfind("\n\n")?;
    if end < start {
        return None;
    }
    Some(unescape(&text[start + marker.len()..end]))
}

/// The decoded body a block carries after its `label` line, up to the block's
/// closing marker, for a body with no fixed text after it.
pub fn body(text: &str, label: &str) -> String {
    let marker = format!("\n{label}\n");
    let (Some(start), Some(end)) = (text.find(&marker), text.rfind("\n</dim-goal>")) else {
        return String::new();
    };
    if end < start + marker.len() {
        return String::new();
    }
    unescape(&text[start + marker.len()..end])
}
