//! `edit` (TS `edit.ts`): exact replacement with EOL, Unicode and recovery fallbacks.

use super::super::file_reads::{Eol, FileReads, TextFileBinding};
use super::codec;
use super::write::write_target;
use super::{
    Before, Cancelled, FileTools, NotRegular, blocking, cancelled, failed, field, hex,
    invalid_input, js_len, not_regular, open_regular, record_published, resolve_path, revision,
    wrote,
};
use crate::host::HostError;
use dim_agent_types::{JsonObject, ToolResult};
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Read},
    path::Path,
};
use tokio_util::sync::CancellationToken;

/// Max combined UTF-16 length of oldText + newText.
const EDIT_INPUT_MAX_CHARS: usize = 10_000;
/// TS limit: min(1 GiB, V8 `MAX_STRING_LENGTH`).
const EDIT_TARGET_MAX_BYTES: u64 = 536_870_888;
/// Read size per cancellation check.
const READ_CHUNK_BYTES: usize = 64 * 1024;
/// Context lines around each change in the display diff.
const EDIT_DIFF_CONTEXT: usize = 3;
/// A longer file gets no closest-text hint on a refused edit.
const CLOSEST_MAX_FILE_LINES: usize = 100_000;
/// Lines the closest-text hint shows.
const CLOSEST_MAX_LINES: usize = 12;
/// Chars of one line the closest-text hint shows.
const CLOSEST_MAX_LINE_CHARS: usize = 200;

pub(super) fn definition() -> Value {
    json!({
        "name": "edit",
        "description": "Edit an existing file by replacing exact text matches. Read the file first, then derive oldText and newText from the latest read result. Use this tool for ordinary incremental text edits instead of shell commands or Python. Supports single replacement (default) or replacing all matches with replaceAll.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to edit." },
                "oldText": { "type": "string", "description": "Text to find and replace (exact match)." },
                "newText": { "type": "string", "description": "Replacement text." },
                "replaceAll": { "type": "boolean", "description": "Replace all occurrences (default: false)." },
            },
            "required": ["path", "oldText", "newText"],
        },
        "source": "builtin",
        "risk": "write_filesystem",
        "fileChanges": {
            "preExecutePaths": { "selector": "path" },
            "postExecutePaths": "path",
        },
    })
}

struct Input {
    path: String,
    old_text: String,
    new_text: String,
    replace_all: bool,
}

fn parse(input: &JsonObject) -> Result<Input, ToolResult> {
    let Some(Value::String(path)) = field(input, "path").filter(|path| path.as_str() != Some(""))
    else {
        return Err(invalid_input("edit requires a non-empty string path."));
    };
    let Some(Value::String(old_text)) = field(input, "oldText") else {
        return Err(invalid_input("edit requires a string oldText."));
    };
    if old_text.is_empty() {
        return Err(invalid_input("oldText must not be empty."));
    }
    let Some(Value::String(new_text)) = field(input, "newText") else {
        return Err(invalid_input("edit requires a string newText."));
    };
    Ok(Input {
        path: path.clone(),
        old_text: old_text.clone(),
        new_text: new_text.clone(),
        replace_all: field(input, "replaceAll") == Some(&Value::Bool(true)),
    })
}

pub(super) async fn run(
    tools: &FileTools,
    input: &JsonObject,
    reads: &FileReads,
    cancel: CancellationToken,
) -> Result<ToolResult, HostError> {
    let input = match parse(input) {
        Ok(input) => input,
        Err(result) => return Ok(result),
    };
    let established = reads.established();
    let chars = js_len(&input.old_text) + js_len(&input.new_text);
    if chars > EDIT_INPUT_MAX_CHARS {
        return Ok(failed(
            format!(
                "edit input too large: oldText + newText is {chars} chars, limit is {EDIT_INPUT_MAX_CHARS} chars."
            ),
            json!({ "code": "input_too_large", "chars": chars, "limit": EDIT_INPUT_MAX_CHARS }),
        ));
    }
    if input.old_text == input.new_text {
        return Ok(failed(
            "oldText and newText are identical; no change needed.",
            json!({ "code": "no_change" }),
        ));
    }
    let (cwd, home, requested) = (tools.cwd.clone(), tools.home.clone(), input.path.clone());
    let path = match blocking(move || resolve_path(&cwd, &home, &requested)).await? {
        Ok(path) => path,
        Err(message) => return Ok(invalid_input(message)),
    };
    let _lock = tools.locks.lock(&path, &cancel).await?;
    let binding = reads.get(&path);
    let (target, stop) = (path.clone(), cancel.clone());
    let (_lock, prepared) = blocking(move || {
        let prepared = prepare(&target, &input, binding, &stop);
        (_lock, prepared)
    })
    .await?;
    let mut prepared = match prepared.map_err(|Cancelled| cancelled())? {
        Ok(prepared) => prepared,
        Err(result) => return Ok(result),
    };
    let replaced = std::mem::take(&mut prepared.replaced);
    let before = match tools
        .keep(&prepared.display, Some(replaced), &cancel)
        .await?
    {
        Ok(before) => before,
        Err(result) => return Ok(result),
    };
    let target = path.clone();
    #[cfg(test)]
    let hook = tools.before_commit.clone();
    let (_lock, written) = blocking(move || {
        let written = publish(
            &target,
            prepared,
            &before,
            &cancel,
            #[cfg(test)]
            hook,
        );
        (_lock, written)
    })
    .await?;
    let (result, binding) = written.map_err(|Cancelled| cancelled())?;
    if let Some(binding) = binding {
        reads.set(path, binding, established);
    }
    Ok(result)
}

/// An `edit` whose replacement is computed, ready to publish.
struct Prepared {
    display: String,
    original: std::fs::File,
    /// The bytes the publish replaces.
    replaced: Vec<u8>,
    binding: TextFileBinding,
    after: String,
    written: Vec<u8>,
    count: usize,
    recovery: Option<Recovery>,
    diff: String,
}

fn prepare(
    path: &Path,
    input: &Input,
    binding: Option<TextFileBinding>,
    cancel: &CancellationToken,
) -> Result<Result<Prepared, ToolResult>, Cancelled> {
    let done = |result| Ok(Err(result));
    let display = &input.path;
    Cancelled::check(cancel)?;
    let size = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            return done(failed(
                format!("Cannot edit directory: {display}"),
                json!({ "code": "is_directory" }),
            ));
        }
        Ok(metadata) => metadata.len(),
        Err(error) => {
            return done(match error.kind() {
                ErrorKind::NotFound => failed(
                    format!("File not found: {display}"),
                    json!({ "code": "file_not_found" }),
                ),
                ErrorKind::PermissionDenied => failed(
                    format!("Permission denied: {display}"),
                    json!({ "code": "permission_denied" }),
                ),
                _ => failed(
                    format!("Failed to stat file: {error}"),
                    json!({ "code": "stat_error" }),
                ),
            });
        }
    };
    Cancelled::check(cancel)?;
    if size > EDIT_TARGET_MAX_BYTES {
        return done(too_large(size));
    }
    let read_failure = |error: std::io::Error| {
        if error.kind() == ErrorKind::PermissionDenied {
            failed(
                format!("Permission denied: {display}"),
                json!({ "code": "permission_denied" }),
            )
        } else {
            failed(
                format!("Failed to read file: {error}"),
                json!({ "code": "read_error" }),
            )
        }
    };
    let file = match open_regular(path) {
        Ok(Ok(file)) => file,
        Ok(Err(NotRegular)) => return done(not_regular(display)),
        Err(error) => return done(read_failure(error)),
    };
    let Some(binding) = binding else {
        return done(failed(
            "Read the file before editing it so its encoding and line endings can be preserved.",
            json!({ "code": "text_binding_required" }),
        ));
    };
    let mut bytes = Vec::with_capacity(size as usize);
    let mut limited = file.take(EDIT_TARGET_MAX_BYTES + 1);
    let mut chunk = vec![0; READ_CHUNK_BYTES];
    loop {
        Cancelled::check(cancel)?;
        match limited.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return done(read_failure(error)),
        }
    }
    if bytes.len() as u64 > EDIT_TARGET_MAX_BYTES {
        return done(too_large(bytes.len() as u64));
    }
    if revision(&bytes) != binding.raw_revision {
        return done(failed(
            "File was modified after it was last read. Use the read tool again before editing.",
            json!({ "code": "file_modified_since_read" }),
        ));
    }
    let before = match codec::decode_text(&bytes, &binding) {
        Ok(text) => text,
        Err(error) => return done(error.into_result()),
    };
    let eol = match binding.eol {
        Eol::None => Eol::Lf,
        eol => eol,
    };
    let Replaced {
        content: after,
        count,
        recovery,
    } = match replace(&before, input, eol) {
        Ok(replaced) => replaced,
        Err(result) => return done(result),
    };
    Cancelled::check(cancel)?;
    let diff = unified_diff(&to_lf(&before), &to_lf(&after));
    let written = match codec::encode_text(&after, &binding) {
        Ok(bytes) => bytes,
        Err(error) => return done(error.into_result()),
    };
    Ok(Ok(Prepared {
        display: display.clone(),
        original: limited.into_inner(),
        replaced: bytes,
        binding,
        after,
        written,
        count,
        recovery,
        diff,
    }))
}

fn publish(
    path: &Path,
    prepared: Prepared,
    before: &Before,
    cancel: &CancellationToken,
    #[cfg(test)] hook: Option<super::CommitHook>,
) -> Result<(ToolResult, Option<TextFileBinding>), Cancelled> {
    let Prepared {
        display,
        original,
        binding,
        after,
        written,
        count,
        recovery,
        diff,
        ..
    } = prepared;
    Cancelled::check(cancel)?;
    let raw_revision = revision(&written);
    if let Err(mut result) = write_target(
        path,
        &written,
        &display,
        Some(&original),
        &binding.raw_revision,
        cancel,
        #[cfg(test)]
        hook,
    )? {
        record_published(&mut result, before, &raw_revision);
        return Ok((result, None));
    }
    let mut structured = json!({
        "path": display,
        "replacementCount": count,
        "revision": hex(&raw_revision),
        "diff": diff,
    });
    if let Some(recovery) = recovery {
        structured["recovery"] = json!(recovery.code());
    }
    let result = wrote(
        match recovery {
            Some(recovery) => format!("Edited {display}. {}", recovery.note()),
            None => format!("Edited {display}"),
        },
        structured,
        &display,
        before,
    );
    let binding = TextFileBinding {
        raw_revision,
        eol: codec::detect_eol(&after),
        ..binding
    };
    Ok((result, Some(binding)))
}

/// `size` is a lower bound when the file grew while being read.
fn too_large(size: u64) -> ToolResult {
    failed(
        format!(
            "edit target file too large: file is {size} bytes, limit is {EDIT_TARGET_MAX_BYTES} bytes."
        ),
        json!({ "code": "edit_target_file_too_large", "bytes": size, "limit": EDIT_TARGET_MAX_BYTES }),
    )
}

pub(super) fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn to_crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

fn multiple_matches(matches: usize) -> ToolResult {
    failed(
        "oldText matched more than once; set replaceAll to true to replace all matches.",
        json!({ "code": "multiple_matches", "matches": matches }),
    )
}

/// How `oldText` was located after it did not match as given.
#[derive(Clone, Copy)]
enum Recovery {
    LineNumbers,
    Escapes,
    LineWhitespace {
        start: usize,
        end: usize,
        reindented: bool,
    },
}

impl Recovery {
    fn code(self) -> &'static str {
        match self {
            Self::LineNumbers => "line_numbers",
            Self::Escapes => "escapes",
            Self::LineWhitespace { .. } => "line_whitespace",
        }
    }

    fn note(self) -> String {
        match self {
            Self::LineNumbers => {
                "oldText matched after removing the line numbers read adds.".to_owned()
            }
            Self::Escapes => {
                "oldText matched after unescaping it; newText was unescaped the same way."
                    .to_owned()
            }
            Self::LineWhitespace {
                start,
                end,
                reindented,
            } => format!(
                "oldText matched lines {start}-{end} ignoring each line's surrounding whitespace{}.",
                if reindented {
                    "; newText was re-indented to those lines"
                } else {
                    ""
                }
            ),
        }
    }
}

struct Replaced {
    content: String,
    count: usize,
    recovery: Option<Recovery>,
}

/// As given first; then, only when that finds nothing, the mistakes a model
/// makes copying text: `read`'s line numbers, one level of escaping, and each
/// line's surrounding whitespace. A recovered match must be as unique as an
/// exact one, and the result says which recovery located it.
fn replace(file: &str, input: &Input, eol: Eol) -> Result<Replaced, ToolResult> {
    let lf_file = to_lf(file);
    let lf_old = to_lf(&input.old_text);
    let lf_new = to_lf(&input.new_text);
    let replace_all = input.replace_all;
    let replaced = |found: Option<(String, usize)>, recovery| {
        found.map(|(content, count)| Replaced {
            content,
            count,
            recovery,
        })
    };
    if let Some(found) = replaced(
        replace_text(file, &lf_file, &lf_old, &lf_new, replace_all, eol)?,
        None,
    ) {
        return Ok(found);
    }
    if let Some(old) = without_line_numbers(&lf_old) {
        let new = without_line_numbers(&lf_new).unwrap_or_else(|| lf_new.clone());
        let found = replace_text(file, &lf_file, &old, &new, replace_all, eol)?;
        if let Some(found) = replaced(found, Some(Recovery::LineNumbers)) {
            return changed(file, found);
        }
    }
    if let Some(old) = unescaped(&lf_old) {
        let new = unescaped(&lf_new).unwrap_or_else(|| lf_new.clone());
        let found = replace_text(file, &lf_file, &old, &new, replace_all, eol)?;
        if let Some(found) = replaced(found, Some(Recovery::Escapes)) {
            return changed(file, found);
        }
    }
    if !replace_all && let Some(found) = replace_line_block(file, &lf_file, &lf_old, &lf_new, eol)?
    {
        return changed(file, found);
    }
    Err(not_found(&lf_file, &lf_old))
}

/// A recovered `oldText` can already read as `newText`: a model re-indenting a
/// block it copied without its indentation, say.
fn changed(file: &str, replaced: Replaced) -> Result<Replaced, ToolResult> {
    if replaced.content == file {
        return Err(failed(
            "The target text already reads as newText; no change needed.",
            json!({ "code": "no_change" }),
        ));
    }
    Ok(replaced)
}

/// Exact match in LF space first, then the Unicode/trailing-whitespace
/// fallback. `eol` is never `None` here.
///
/// `newText` is always inserted literally. TS single replacement goes through
/// JS `String.replace`, which expands `$&`, `$$`, `` $` `` and `$'` in the
/// replacement, while its `replaceAll` and fuzzy paths do not; that is a JS
/// API accident, not a tool contract, and it corrupts code that contains
/// `$$` or `$&` (shell, PHP, regex docs), so Rust keeps every path literal.
fn replace_text(
    file: &str,
    lf_file: &str,
    lf_old: &str,
    lf_new: &str,
    replace_all: bool,
    eol: Eol,
) -> Result<Option<(String, usize)>, ToolResult> {
    let exact = lf_file.matches(lf_old).count();
    if exact > 0 {
        if !replace_all && exact > 1 {
            return Err(multiple_matches(exact));
        }
        let limit = if replace_all { usize::MAX } else { 1 };
        let content = match eol {
            Eol::Mixed => replace_lf_matches_in_raw(file, lf_old, lf_new, replace_all),
            Eol::Crlf => to_crlf(&lf_file.replacen(lf_old, lf_new, limit)),
            _ => lf_file.replacen(lf_old, lf_new, limit),
        };
        return Ok(Some((content, if replace_all { exact } else { 1 })));
    }

    let normalized_file = normalize(lf_file);
    let normalized_old = normalize(lf_old);
    let matches = if normalized_old.is_empty() {
        0
    } else {
        normalized_file.matches(normalized_old.as_str()).count()
    };
    if matches > 0 {
        if !replace_all && matches > 1 {
            return Err(multiple_matches(matches));
        }
        let content = replace_by_normalized_positions(
            file,
            &normalized_file,
            &normalized_old,
            lf_new,
            replace_all,
            eol,
        );
        return Ok(Some((content, if replace_all { matches } else { 1 })));
    }
    Ok(None)
}

/// `text` without a line-number prefix, when every non-blank line carries one:
/// `read`'s `<n><tab>`, `cat -n`'s padded form, or `<n>: `.
fn without_line_numbers(lf_text: &str) -> Option<String> {
    let lines = lf_text
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return Some(line);
            }
            let digits = line.trim_start_matches(' ');
            let rest = digits.trim_start_matches(|c: char| c.is_ascii_digit());
            (rest.len() < digits.len())
                .then(|| rest.strip_prefix('\t').or_else(|| rest.strip_prefix(": ")))
                .flatten()
        })
        .collect::<Option<Vec<_>>>()?;
    let stripped = lines.join("\n");
    (stripped != lf_text).then_some(stripped)
}

/// `text` with one level of backslash escaping undone, when it has any: a
/// tool-call argument encoded twice carries `\n` and `\"` as two characters.
fn unescaped(lf_text: &str) -> Option<String> {
    let mut result = String::with_capacity(lf_text.len());
    let mut chars = lf_text.chars().peekable();
    while let Some(c) = chars.next() {
        let unescaped = (c == '\\')
            .then(|| chars.peek().copied())
            .flatten()
            .and_then(|next| match next {
                'n' => Some('\n'),
                't' => Some('\t'),
                'r' => Some('\r'),
                '"' | '\'' | '`' | '\\' | '$' => Some(next),
                _ => None,
            });
        match unescaped {
            Some(unescaped) => {
                chars.next();
                result.push(unescaped);
            }
            None => result.push(c),
        }
    }
    (result != lf_text).then(|| to_lf(&result))
}

fn normalized_line(line: &str) -> String {
    line.chars().map(normalize_char).collect::<String>()
}

/// The one block of whole lines that matches `oldText` once each line's
/// surrounding whitespace is ignored. A block that differs from `oldText` by a
/// uniform indentation shift takes `newText` with that shift applied.
fn replace_line_block(
    file: &str,
    lf_file: &str,
    lf_old: &str,
    lf_new: &str,
    eol: Eol,
) -> Result<Option<Replaced>, ToolResult> {
    let old_lines: Vec<&str> = lf_old
        .strip_suffix('\n')
        .unwrap_or(lf_old)
        .split('\n')
        .collect();
    if old_lines.iter().all(|line| line.trim().is_empty()) {
        return Ok(None);
    }
    let wanted: Vec<String> = old_lines
        .iter()
        .map(|line| normalized_line(line).trim().to_owned())
        .collect();
    let lines: Vec<&str> = lf_file.split('\n').collect();
    let have: Vec<String> = lines
        .iter()
        .map(|line| normalized_line(line).trim().to_owned())
        .collect();
    let starts: Vec<usize> = have
        .windows(wanted.len())
        .enumerate()
        .filter(|(_, window)| *window == wanted.as_slice())
        .map(|(start, _)| start)
        .collect();
    let start = match starts.as_slice() {
        [] => return Ok(None),
        [start] => *start,
        _ => return Err(multiple_matches(starts.len())),
    };
    let block = &lines[start..start + wanted.len()];
    let lf_start: usize = lines[..start].iter().map(|line| line.len() + 1).sum();
    let mut lf_end = lf_start + block.join("\n").len();
    if lf_old.ends_with('\n') && lf_file[lf_end..].starts_with('\n') {
        lf_end += 1;
    }
    let reindented = reindent(lf_new, &old_lines, block);
    let new_text = reindented.as_deref().unwrap_or(lf_new);
    let content = match eol {
        Eol::Mixed => {
            let view = LfView::new(file);
            let (raw_start, raw_end) = (view.boundaries[lf_start], view.boundaries[lf_end]);
            let local = apply_local_eol(new_text, file, raw_start, raw_end);
            let mut result = file.to_owned();
            result.replace_range(raw_start..raw_end, &local);
            result
        }
        _ => {
            let mut result = lf_file.to_owned();
            result.replace_range(lf_start..lf_end, new_text);
            if matches!(eol, Eol::Crlf) {
                to_crlf(&result)
            } else {
                result
            }
        }
    };
    Ok(Some(Replaced {
        content,
        count: 1,
        recovery: Some(Recovery::LineWhitespace {
            start: start + 1,
            end: start + wanted.len(),
            reindented: reindented.is_some(),
        }),
    }))
}

/// The leading whitespace every non-blank line shares.
fn common_indent<'a>(lines: &[&'a str]) -> &'a str {
    let mut lines = lines.iter().filter(|line| !line.trim().is_empty());
    let Some(first) = lines.next() else {
        return "";
    };
    let mut indent = &first[..first.len() - first.trim_start().len()];
    for line in lines {
        let shared = indent
            .char_indices()
            .zip(line.chars())
            .take_while(|((_, a), b)| a == b)
            .last()
            .map_or(0, |((index, a), _)| index + a.len_utf8());
        indent = &indent[..shared];
    }
    indent
}

/// `new_text` moved by the indentation shift between `old` and `block`, when
/// every line of the block shows that same shift.
fn reindent(new_text: &str, old: &[&str], block: &[&str]) -> Option<String> {
    let (old_indent, block_indent) = (common_indent(old), common_indent(block));
    let new_lines: Vec<&str> = new_text.split('\n').collect();
    if old_indent == block_indent || common_indent(&new_lines) == block_indent {
        return None;
    }
    let shifted = |line: &str, indent: &str| {
        normalized_line(line.strip_prefix(indent).unwrap_or(line))
            .trim_end()
            .to_owned()
    };
    let uniform = old.iter().zip(block).all(|(old, block)| {
        old.trim().is_empty() || shifted(old, old_indent) == shifted(block, block_indent)
    });
    uniform.then(|| {
        new_lines
            .iter()
            .map(|line| match line.strip_prefix(old_indent) {
                Some(rest) if !line.trim().is_empty() => format!("{block_indent}{rest}"),
                _ => (*line).to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// The not-found refusal, naming the lines that come closest so the model can
/// correct `oldText` from what the file holds now without guessing.
fn not_found(lf_file: &str, lf_old: &str) -> ToolResult {
    let mut message = "oldText was not found in the target file.".to_owned();
    if let Some(closest) = closest_lines(lf_file, lf_old) {
        message.push(' ');
        message.push_str(&closest);
    }
    failed(message, json!({ "code": "oldtext_not_found" }))
}

fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| !word.is_empty())
}

/// The lines around the file line sharing the most words with `oldText`'s
/// wordiest line, when it shares at least half of them.
fn closest_lines(lf_file: &str, lf_old: &str) -> Option<String> {
    let lines: Vec<&str> = lf_file.split('\n').collect();
    if lines.len() > CLOSEST_MAX_FILE_LINES {
        return None;
    }
    let old_lines: Vec<&str> = lf_old.trim_end_matches('\n').split('\n').collect();
    let (anchor_index, anchor) = old_lines
        .iter()
        .enumerate()
        .max_by_key(|(index, line)| (words(line).count(), std::cmp::Reverse(*index)))?;
    let anchor_words: std::collections::HashSet<&str> = words(anchor).collect();
    if anchor_words.is_empty() {
        return None;
    }
    let shared = |line: &str| {
        words(line)
            .collect::<std::collections::HashSet<_>>()
            .intersection(&anchor_words)
            .count()
    };
    let (best, score) = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (index, shared(line)))
        .max_by_key(|(index, score)| (*score, std::cmp::Reverse(*index)))?;
    if score * 2 < anchor_words.len() {
        return None;
    }
    let start = best.saturating_sub(anchor_index);
    let end = (start + old_lines.len().min(CLOSEST_MAX_LINES)).min(lines.len());
    let shown = lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let line: String = line.chars().take(CLOSEST_MAX_LINE_CHARS).collect();
            format!("{}\t{line}", start + index + 1)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "The closest text is at line {}; derive oldText from it:\n{shown}",
        start + 1
    ))
}

/// Smart quotes, Unicode dashes and special spaces map to ASCII one char to
/// one char, so positions within a line carry over by char index.
fn normalize_char(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' => ' ',
        c => c,
    }
}

/// Unicode normalization plus per-line trailing space/tab trim.
fn normalize(lf_text: &str) -> String {
    lf_text
        .split('\n')
        .map(|line| {
            line.chars()
                .map(normalize_char)
                .collect::<String>()
                .trim_end_matches([' ', '\t'])
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn match_positions(haystack: &str, needle: &str, all: bool) -> Vec<usize> {
    let positions = haystack.match_indices(needle).map(|(index, _)| index);
    if all {
        positions.collect()
    } else {
        positions.take(1).collect()
    }
}

fn replace_by_normalized_positions(
    file: &str,
    normalized_file: &str,
    normalized_old: &str,
    new_text: &str,
    replace_all: bool,
    eol: Eol,
) -> String {
    let lf_file = to_lf(file);
    let lf_lines: Vec<&str> = lf_file.split('\n').collect();
    let norm_lines: Vec<&str> = normalized_file.split('\n').collect();
    let starts = |lines: &[&str]| {
        let mut offset = 0;
        lines
            .iter()
            .map(|line| {
                let start = offset;
                offset += line.len() + 1;
                start
            })
            .collect::<Vec<_>>()
    };
    let (lf_starts, norm_starts) = (starts(&lf_lines), starts(&norm_lines));
    let to_lf_offset = |position: usize| {
        let line = norm_starts.partition_point(|start| *start <= position) - 1;
        let within = position - norm_starts[line];
        let (norm_line, lf_line) = (norm_lines[line], lf_lines[line]);
        if within >= norm_line.len() {
            return lf_starts[line] + lf_line.len() + (within - norm_line.len());
        }
        let chars = norm_line[..within].chars().count();
        lf_starts[line]
            + lf_line
                .char_indices()
                .nth(chars)
                .map_or(lf_line.len(), |(index, _)| index)
    };
    let view = matches!(eol, Eol::Mixed).then(|| LfView::new(file));
    let mut result = if view.is_some() {
        file.to_owned()
    } else {
        lf_file.clone()
    };
    for position in match_positions(normalized_file, normalized_old, replace_all)
        .into_iter()
        .rev()
    {
        let lf_start = to_lf_offset(position);
        let lf_end = to_lf_offset(position + normalized_old.len());
        match &view {
            Some(view) => {
                let (raw_start, raw_end) = (view.boundaries[lf_start], view.boundaries[lf_end]);
                let local = apply_local_eol(new_text, file, raw_start, raw_end);
                result.replace_range(raw_start..raw_end, &local);
            }
            None => result.replace_range(lf_start..lf_end, new_text),
        }
    }
    if matches!(eol, Eol::Crlf) {
        to_crlf(&result)
    } else {
        result
    }
}

/// Mixed-EOL files are edited in the raw text so untouched line endings keep
/// their bytes.
fn replace_lf_matches_in_raw(raw: &str, lf_old: &str, lf_new: &str, replace_all: bool) -> String {
    let view = LfView::new(raw);
    let mut result = raw.to_owned();
    for position in match_positions(&view.text, lf_old, replace_all)
        .into_iter()
        .rev()
    {
        let (raw_start, raw_end) = (
            view.boundaries[position],
            view.boundaries[position + lf_old.len()],
        );
        let replacement = apply_local_eol(lf_new, raw, raw_start, raw_end);
        result.replace_range(raw_start..raw_end, &replacement);
    }
    result
}

/// LF view of raw text with each LF byte offset mapped to its raw offset.
struct LfView {
    text: String,
    boundaries: Vec<usize>,
}
impl LfView {
    fn new(raw: &str) -> Self {
        let bytes = raw.as_bytes();
        let mut text = Vec::with_capacity(bytes.len());
        let mut boundaries = vec![0];
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                text.push(b'\n');
                index += 2;
            } else {
                text.push(bytes[index]);
                index += 1;
            }
            boundaries.push(index);
        }
        Self {
            text: String::from_utf8(text).expect("CRLF folding keeps UTF-8 valid"),
            boundaries,
        }
    }
}

/// New line breaks follow the matched region's style, else the nearest one.
fn apply_local_eol(replacement: &str, raw: &str, start: usize, end: usize) -> String {
    match detect_local_eol(&raw[start..end]).or_else(|| nearest_eol(raw, start, end)) {
        Some(Eol::Crlf) => to_crlf(&to_lf(replacement)),
        Some(_) => to_lf(replacement),
        None => replacement.to_owned(),
    }
}

fn eol_at(bytes: &[u8], newline: usize) -> Eol {
    if newline > 0 && bytes[newline - 1] == b'\r' {
        Eol::Crlf
    } else {
        Eol::Lf
    }
}

fn detect_local_eol(text: &str) -> Option<Eol> {
    let newline = text.find('\n')?;
    Some(eol_at(text.as_bytes(), newline))
}

fn nearest_eol(text: &str, start: usize, end: usize) -> Option<Eol> {
    let bytes = text.as_bytes();
    let before_end = (start.saturating_sub(1) + 1).min(bytes.len());
    let previous = bytes[..before_end].iter().rposition(|byte| *byte == b'\n');
    let next = bytes
        .get(end..)
        .and_then(|rest| rest.iter().position(|byte| *byte == b'\n'))
        .map(|index| index + end);
    let chosen = match (previous, next) {
        (None, None) => return None,
        (None, Some(next)) => next,
        (Some(previous), None) => previous,
        (Some(previous), Some(next)) => {
            if start as isize - previous as isize <= (next - end) as isize {
                previous
            } else {
                next
            }
        }
    };
    Some(eol_at(bytes, chosen))
}

/// Display-only unified diff hunks in LF space.
pub(super) fn unified_diff(before: &str, after: &str) -> String {
    let diff = similar::TextDiff::from_lines(before, after);
    let mut lines = Vec::new();
    for hunk in diff
        .unified_diff()
        .context_radius(EDIT_DIFF_CONTEXT)
        .iter_hunks()
    {
        let ops = hunk.ops();
        let (Some(first), Some(last)) = (ops.first(), ops.last()) else {
            continue;
        };
        let range = |start: usize, end: usize| {
            let len = end - start;
            format!("{},{len}", if len == 0 { start } else { start + 1 })
        };
        lines.push(format!(
            "@@ -{} +{} @@",
            range(first.old_range().start, last.old_range().end),
            range(first.new_range().start, last.new_range().end),
        ));
        for change in hunk.iter_changes() {
            let sign = match change.tag() {
                similar::ChangeTag::Delete => '-',
                similar::ChangeTag::Insert => '+',
                similar::ChangeTag::Equal => ' ',
            };
            let value = change.value();
            lines.push(format!(
                "{sign}{}",
                value.strip_suffix('\n').unwrap_or(value)
            ));
        }
    }
    lines.join("\n")
}
