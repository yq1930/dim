//! A turn's `write` and `edit` changes, and undoing them
//! (`docs/design/file-rewind.md`).
//!
//! Everything is read from the branch's facts and the files as they are now:
//! the turn's tool calls name its `write` and `edit` results, whose `before`
//! and `revision` say what each change started from and what it left.

use super::write::write_target;
use super::{Cancelled, FileTools, NotRegular, blocking, open_regular, revision};
use crate::{
    context_block::{element, raw, text},
    methods::RpcError,
};
use dim_agent_runtime::{
    Branch, TimelineQuery, TimelineQueryDirection, TimelineQueryOrderBy, TimelineQueryPredicate,
    TimelineQueryScope,
};
use dim_agent_types::{
    EventKind, InputMessageData, InputMode, JsonObject, ToolContent, ToolEffect,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    io::{ErrorKind, Read},
    num::NonZeroU64,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

/// The marker a rewind records itself with.
pub(crate) const MARKER: &str = "dim-file-rewind";
/// The guide entry of [`MARKER`].
pub(crate) const ENTRY: &str = r#"<dim-file-rewind>: the operator undid file changes that write and edit made in an earlier turn. Each line names one file: restored means it again holds its content from before that turn, and removed means that turn created it and it was deleted. Files not listed, and changes made by commands, are as they were. Your earlier reads of the listed files no longer match them. This is information, not a request."#;
/// The tools whose results record what they replaced.
const KEEPING_TOOLS: [&str; 2] = ["write", "edit"];
/// How many facts one page of a turn's read takes.
const PAGE: u64 = 256;

/// Whether a Session declaring `tools` (every tool when `None`) can make the
/// changes a rewind undoes, and so needs the guide entry of [`MARKER`].
pub(crate) fn rewindable(tools: Option<&[String]>) -> bool {
    tools.is_none_or(|tools| {
        tools
            .iter()
            .any(|tool| KEEPING_TOOLS.contains(&tool.as_str()))
    })
}

/// A turn's `write` and `edit` changes and the commands it ran.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnChanges {
    pub ended: bool,
    pub files: Vec<FileChange>,
    pub commands: Vec<CommandRun>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub change: Change,
    pub calls: usize,
    pub status: Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    Modified,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// The file holds what the turn left, and what it started from is known.
    Restorable,
    /// The file holds what it held before the turn.
    Restored,
    /// The file changed after the turn.
    Changed,
    /// What the file held before the turn was not kept.
    Unrecorded,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRun {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
}

/// What a rewind did: the files it restored and removed, in path order, the
/// file it stopped at, and whether its record was written.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rewound {
    pub restored: Vec<String>,
    pub removed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<Failure>,
    pub recorded: bool,
}

#[derive(Debug, Serialize)]
pub struct Failure {
    pub path: String,
    pub message: String,
}

/// What a file held before its first change in the turn.
enum Start {
    Absent,
    Kept { path: PathBuf, revision: [u8; 32] },
}

/// One file's changes in a turn: what the first started from, what the last
/// left, and how many there were.
struct Record {
    start: Option<Start>,
    end: Option<[u8; 32]>,
    calls: usize,
}

impl Record {
    fn change(&self) -> Change {
        match self.start {
            Some(Start::Absent) => Change::Created,
            _ => Change::Modified,
        }
    }
}

struct Turn {
    ended: bool,
    files: BTreeMap<String, Record>,
    commands: Vec<CommandRun>,
}

/// What a path holds now.
#[derive(PartialEq, Eq)]
enum Current {
    Absent,
    File([u8; 32]),
    Other,
}

/// How a restorable file gets its content from before the turn back.
enum Restore {
    Remove { end: [u8; 32] },
    Write { bytes: Vec<u8>, end: [u8; 32] },
}

#[derive(Clone, Copy)]
enum Action {
    Restored,
    Removed,
}

/// What the restoring step did, in order, and the file it stopped at.
struct Restoration {
    done: Vec<(Action, String)>,
    failed: Option<Failure>,
}

/// A file a rewind stopped at; `done` when its own change happened anyway.
struct Stop {
    done: bool,
    message: String,
}

impl FileTools {
    /// The `write` and `edit` changes of `turn_id` on `branch`, each with its
    /// status now, and the commands the turn ran.
    pub async fn changes(&self, branch: &Branch, turn_id: &str) -> Result<TurnChanges, RpcError> {
        let Turn {
            ended,
            files,
            commands,
        } = read_turn(branch, turn_id).await?;
        let files = blocking(move || {
            files
                .into_iter()
                .map(|(path, record)| FileChange {
                    status: assess(Path::new(&path), &record)
                        .map_or_else(|status| status, |_| Status::Restorable),
                    change: record.change(),
                    calls: record.calls,
                    path,
                })
                .collect()
        })
        .await
        .map_err(failure)?;
        Ok(TurnChanges {
            ended,
            files,
            commands,
        })
    }

    /// Restores `paths`, files `turn_id`'s `write` and `edit` changed, to
    /// what they held before the turn, and records what it did on `branch`.
    /// Every path must still be restorable, checked under the paths' write
    /// locks, or nothing changes.
    pub async fn rewind(
        &self,
        branch: &Branch,
        turn_id: &str,
        mut paths: Vec<String>,
        cancel: CancellationToken,
    ) -> Result<Rewound, RpcError> {
        let mut turn = read_turn(branch, turn_id).await?;
        if !turn.ended {
            return Err(RpcError::app(
                -32000,
                "turn_running",
                "The turn has not ended; undo its file changes after it ends",
            ));
        }
        paths.sort();
        if paths.is_empty() || paths.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(RpcError::invalid("paths must name distinct files"));
        }
        let mut records = Vec::with_capacity(paths.len());
        for path in paths {
            let Some(record) = turn.files.remove(&path) else {
                return Err(RpcError::invalid(format!(
                    "{path} is not a file this turn's write or edit changed"
                )));
            };
            records.push((path, record));
        }
        let mut locks = Vec::with_capacity(records.len());
        for (path, _) in &records {
            let lock = self.locks.lock(Path::new(path), &cancel).await;
            locks.push(lock.map_err(|_| RpcError::interrupted())?);
        }
        let (_locks, restored) = blocking(move || {
            let restored = restore(records, &cancel);
            (locks, restored)
        })
        .await
        .map_err(failure)?;
        let Restoration { done, failed } = restored?;
        let recorded = !done.is_empty() && branch.input(record(&done)).await.is_ok();
        let paths = |wanted: fn(&Action) -> bool| {
            done.iter()
                .filter(|(action, _)| wanted(action))
                .map(|(_, path)| path.clone())
                .collect()
        };
        Ok(Rewound {
            restored: paths(|action| matches!(action, Action::Restored)),
            removed: paths(|action| matches!(action, Action::Removed)),
            failed,
            recorded,
        })
    }
}

fn failure(error: impl std::fmt::Display) -> RpcError {
    RpcError::app(-32000, "session_files_failed", error.to_string())
}

/// Folds the turn's tool calls and results, and whether it ended. A turn
/// this branch never started is `invalid_params`.
async fn read_turn(branch: &Branch, turn_id: &str) -> Result<Turn, RpcError> {
    let bounds = branch
        .query(turn_query(turn_id, &["turn/start", "turn/end"], None, 2))
        .await?;
    if bounds.events.is_empty() {
        return Err(RpcError::invalid(format!(
            "No turn {turn_id} on this branch"
        )));
    }
    let ended = bounds
        .events
        .iter()
        .any(|event| matches!(event.kind, EventKind::TurnEnd(_)));
    let mut names = HashMap::new();
    let mut files = BTreeMap::new();
    let mut commands = Vec::new();
    let mut from = None;
    loop {
        let reply = branch
            .query(turn_query(
                turn_id,
                &["step/tool-call", "step/tool-result"],
                from,
                PAGE,
            ))
            .await?;
        for event in &reply.events {
            match &event.kind {
                EventKind::ToolCall(data) => names.extend(
                    data.calls
                        .iter()
                        .map(|call| (call.id.clone(), call.name.clone())),
                ),
                EventKind::ToolResult(data) => {
                    for call in &data.results {
                        let keeping = names
                            .get(&call.call_id)
                            .is_some_and(|name: &String| KEEPING_TOOLS.contains(&name.as_str()));
                        let fields = call.result.structured_content.as_ref();
                        for effect in call.result.effects.iter().flatten() {
                            match effect {
                                ToolEffect::FileWritten { path } if keeping => {
                                    let record =
                                        files.entry(path.clone()).or_insert_with(|| Record {
                                            start: fields.and_then(start),
                                            end: None,
                                            calls: 0,
                                        });
                                    record.end = fields
                                        .and_then(|fields| fields.get("revision"))
                                        .and_then(Value::as_str)
                                        .and_then(parse_revision);
                                    record.calls += 1;
                                }
                                ToolEffect::FileWritten { .. } => {}
                                ToolEffect::CommandRun {
                                    command,
                                    exit_code,
                                    signal,
                                } => commands.push(CommandRun {
                                    command: command.clone(),
                                    exit_code: *exit_code,
                                    signal: signal.clone(),
                                }),
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        match reply.positions.last() {
            Some(last) if reply.events.len() as u64 == PAGE => from = Some(last.sequence + 1),
            _ => break,
        }
    }
    Ok(Turn {
        ended,
        files,
        commands,
    })
}

fn turn_query(
    turn_id: &str,
    kinds: &[&str],
    from_sequence: Option<u64>,
    limit: u64,
) -> TimelineQuery {
    TimelineQuery {
        scope: TimelineQueryScope::Branch,
        from_event: None,
        from_sequence,
        order_by: TimelineQueryOrderBy::Sequence,
        direction: TimelineQueryDirection::Ascending,
        include_publishing: false,
        kinds: kinds.iter().map(|kind| (*kind).to_owned()).collect(),
        flags: Vec::new(),
        event_id: None,
        predicate: Some(TimelineQueryPredicate {
            path: "turn_id".into(),
            value: Value::String(turn_id.into()),
        }),
        limit: NonZeroU64::new(limit).expect("a page reads at least one fact"),
        offset: 0,
    }
}

/// The recorded `before` of a `write` or `edit` result.
fn start(fields: &JsonObject) -> Option<Start> {
    let before = fields.get("before")?;
    match before.get("kind")?.as_str()? {
        "absent" => Some(Start::Absent),
        "kept" => Some(Start::Kept {
            path: PathBuf::from(before.get("path")?.as_str()?),
            revision: parse_revision(before.get("revision")?.as_str()?)?,
        }),
        _ => None,
    }
}

fn parse_revision(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut revision = [0; 32];
    for (index, byte) in revision.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(revision)
}

fn read_regular(path: &Path) -> Option<Vec<u8>> {
    let mut file = open_regular(path).ok()?.ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn current(path: &Path) -> Current {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Current::Absent,
        Ok(metadata) if metadata.is_file() => {
            read_regular(path).map_or(Current::Other, |bytes| Current::File(revision(&bytes)))
        }
        _ => Current::Other,
    }
}

/// How `path` gets back what it held before the turn, or why it cannot.
fn assess(path: &Path, record: &Record) -> Result<Restore, Status> {
    let (Some(start), Some(end)) = (&record.start, record.end) else {
        return Err(Status::Unrecorded);
    };
    let now = current(path);
    match start {
        Start::Absent if now == Current::Absent => Err(Status::Restored),
        Start::Kept { revision, .. } if now == Current::File(*revision) => Err(Status::Restored),
        _ if now != Current::File(end) => Err(Status::Changed),
        Start::Absent => Ok(Restore::Remove { end }),
        Start::Kept {
            path: kept,
            revision: kept_revision,
        } => match read_regular(kept) {
            Some(bytes) if revision(&bytes) == *kept_revision => Ok(Restore::Write { bytes, end }),
            _ => Err(Status::Unrecorded),
        },
    }
}

/// Checks every file, then restores them in order, stopping at the first
/// that fails or when cancelled.
fn restore(
    records: Vec<(String, Record)>,
    cancel: &CancellationToken,
) -> Result<Restoration, RpcError> {
    let mut plan = Vec::with_capacity(records.len());
    for (path, record) in records {
        match assess(Path::new(&path), &record) {
            Ok(restore) => plan.push((path, restore)),
            Err(status) => {
                let reason = match status {
                    Status::Restored => "already holds its content from before the turn",
                    Status::Changed => "changed after the turn",
                    Status::Unrecorded | Status::Restorable => {
                        "has no kept content from before the turn"
                    }
                };
                return Err(RpcError::app(
                    -32000,
                    "rewind_conflict",
                    format!("{path} {reason}; nothing was restored"),
                ));
            }
        }
    }
    let mut done = Vec::with_capacity(plan.len());
    for (path, restore) in plan {
        if cancel.is_cancelled() {
            break;
        }
        let (action, outcome) = match restore {
            Restore::Remove { end } => (Action::Removed, remove(Path::new(&path), &end)),
            Restore::Write { bytes, end } => (
                Action::Restored,
                put_back(Path::new(&path), &bytes, &end, cancel),
            ),
        };
        if let Err(stop) = outcome {
            if stop.done {
                done.push((action, path.clone()));
            }
            let failed = Some(Failure {
                path,
                message: stop.message,
            });
            return Ok(Restoration { done, failed });
        }
        done.push((action, path));
    }
    Ok(Restoration { done, failed: None })
}

/// Replaces the file, which must still hold `end`, with `bytes`, keeping its
/// metadata as `write` does.
fn put_back(
    path: &Path,
    bytes: &[u8],
    end: &[u8; 32],
    cancel: &CancellationToken,
) -> Result<(), Stop> {
    let stop = |done, message: String| Stop { done, message };
    let original = match open_regular(path) {
        Ok(Ok(file)) => file,
        Ok(Err(NotRegular)) => return Err(stop(false, "Not a regular file".into())),
        Err(error) => return Err(stop(false, error.to_string())),
    };
    let display = path.display().to_string();
    match write_target(
        path,
        bytes,
        &display,
        Some(&original),
        end,
        cancel,
        #[cfg(test)]
        None,
    ) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(result)) => Err(stop(
            result.effects.is_some(),
            match result.content {
                ToolContent::Text(message) => message,
                ToolContent::Blocks(_) => "Failed to restore the file".into(),
            },
        )),
        Err(Cancelled) => Err(stop(false, "Cancelled before the file was restored".into())),
    }
}

/// Removes the file the turn created, which must still hold `end`.
fn remove(path: &Path, end: &[u8; 32]) -> Result<(), Stop> {
    if current(path) != Current::File(*end) {
        return Err(Stop {
            done: false,
            message: "The file changed while it was being restored".into(),
        });
    }
    if let Err(error) = std::fs::remove_file(path) {
        return Err(Stop {
            done: false,
            message: error.to_string(),
        });
    }
    #[cfg(unix)]
    if let Err(error) = std::fs::File::open(path.parent().expect("a written file has a parent"))
        .and_then(|parent| parent.sync_all())
    {
        return Err(Stop {
            done: true,
            message: format!(
                "The file was removed, but its directory could not be synced: {error}"
            ),
        });
    }
    Ok(())
}

/// The input that records a rewind (design §5.4).
fn record(done: &[(Action, String)]) -> InputMessageData {
    let lines = done
        .iter()
        .map(|(action, path)| {
            let word = match action {
                Action::Restored => "restored",
                Action::Removed => "removed",
            };
            format!("{word} {}", text(path))
        })
        .collect::<Vec<_>>();
    InputMessageData {
        blocks: vec![raw(element(MARKER, &[], &lines.join("\n")), None)],
        mode: Some(InputMode::Append),
        ..InputMessageData::default()
    }
}
