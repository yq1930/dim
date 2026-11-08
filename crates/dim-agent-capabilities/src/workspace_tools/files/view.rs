//! `session.files.read`: a file of the Session's workspace, or of its command
//! output, as the web workspace's file pane shows it. The read is for display
//! only and registers nothing with FileReads. A whole UTF-8 file of the
//! workspace is `editable`, with the `revision` `session.files.save` expects.

use super::super::file_reads::TextEncoding;
use super::{
    NotRegular, codec, hex, open_regular, read::BINARY_EXTENSIONS, resolve_path, revision,
};
use crate::images::{self, Sniffed};
use crate::methods::RpcError;
use base64::{Engine, engine::general_purpose::STANDARD};
use encoding_rs::CoderResult;
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Text past this many bytes is cut at its last whole line.
const TEXT_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Larger images are not sent.
const IMAGE_MAX_BYTES: u64 = 10 * 1024 * 1024;
const DETECT_BYTES: u64 = 4096;
const PDF_MAGIC: &[u8] = b"%PDF-";

/// The lines a link names after its path: `path:line`, `path:line:column` or
/// `path:line-end`.
struct Location {
    line: u64,
    end_line: Option<u64>,
}

/// Reads `input` (absolute, `~/…` or relative to `cwd`) for display. Only a
/// file under `cwd` or `output` is served; any other path is
/// `file_outside_workspace`.
pub(crate) fn view(cwd: &Path, home: &Path, output: &Path, input: &str) -> Result<Value, RpcError> {
    let (path, location) = locate(cwd, home, input)?;
    let roots =
        [cwd, output].map(|root| std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned()));
    let Some(root) = roots.iter().position(|root| path.starts_with(root)) else {
        return Err(RpcError::app(
            -32000,
            "file_outside_workspace",
            format!("{} is outside the Session's workspace", path.display()),
        ));
    };
    let mut reply = json!({ "path": path.to_string_lossy() });
    if root == 0
        && let Ok(relative) = path.strip_prefix(&roots[0])
        && !relative.as_os_str().is_empty()
    {
        reply["relativePath"] = json!(relative.to_string_lossy());
    }
    if let Some(location) = location {
        reply["line"] = json!(location.line);
        if let Some(end) = location.end_line {
            reply["endLine"] = json!(end);
        }
    }
    let metadata = std::fs::metadata(&path).map_err(|error| io_error(&error, input))?;
    if metadata.is_dir() {
        reply["kind"] = json!("directory");
        return Ok(reply);
    }
    let mut file = match open_regular(&path) {
        Ok(Ok(file)) => file,
        Ok(Err(NotRegular)) => return Err(failed(format!("Not a regular file: {input}"))),
        Err(error) => return Err(io_error(&error, input)),
    };
    let size = metadata.len();
    reply["size"] = json!(size);
    if let Some(modified) = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    {
        reply["modified"] = json!(modified.as_millis() as u64);
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(DETECT_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(&error, input))?;
    match images::sniff(&bytes) {
        Some(Sniffed::Image(kind)) if size <= IMAGE_MAX_BYTES => {
            file.read_to_end(&mut bytes)
                .map_err(|error| io_error(&error, input))?;
            reply["kind"] = json!("image");
            reply["mime"] = json!(kind.media_type());
            reply["data"] = json!(STANDARD.encode(&bytes));
            return Ok(reply);
        }
        Some(_) => return Ok(binary(reply)),
        None => {}
    }
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());
    if bytes.starts_with(PDF_MAGIC)
        || extension.is_some_and(|extension| BINARY_EXTENSIONS.contains(&extension.as_str()))
    {
        return Ok(binary(reply));
    }
    let Ok(resolved) = codec::resolve_encoding(&bytes, None) else {
        return Ok(binary(reply));
    };
    let utf16 = matches!(
        resolved.encoding,
        TextEncoding::Utf16Le | TextEncoding::Utf16Be
    );
    if !utf16 && bytes.contains(&0) {
        return Ok(binary(reply));
    }
    (&mut file)
        .take(TEXT_MAX_BYTES + 1 - bytes.len() as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| io_error(&error, input))?;
    let truncated = bytes.len() as u64 > TEXT_MAX_BYTES;
    bytes.truncate(TEXT_MAX_BYTES as usize);
    let mut body = bytes
        .get(codec::bom_bytes(resolved.bom).len()..)
        .unwrap_or_default();
    let mut decoder = codec::encoding_rs_of(&resolved.encoding).new_decoder_without_bom_handling();
    let mut text = String::new();
    let mut lossy = false;
    loop {
        text.reserve(
            decoder
                .max_utf8_buffer_length(body.len())
                .unwrap_or(body.len() * 3 + 16),
        );
        let (result, read, replaced) = decoder.decode_to_string(body, &mut text, !truncated);
        lossy |= replaced;
        body = &body[read..];
        if result == CoderResult::InputEmpty {
            break;
        }
    }
    if truncated && let Some(end) = text.rfind('\n') {
        text.truncate(end + 1);
    }
    let editable = root == 0 && !truncated && !lossy && !utf16;
    if editable {
        reply["revision"] = json!(hex(&revision(&bytes)));
    }
    reply["kind"] = json!("text");
    reply["text"] = json!(text);
    reply["truncated"] = json!(truncated);
    reply["lossy"] = json!(lossy);
    reply["editable"] = json!(editable);
    Ok(reply)
}

/// The existing path `input` names, trying it whole first and then without a
/// trailing location.
fn locate(cwd: &Path, home: &Path, input: &str) -> Result<(PathBuf, Option<Location>), RpcError> {
    let resolve = |text: &str| resolve_path(cwd, home, text).map_err(failed);
    let exists = |path: &Path| std::fs::symlink_metadata(path).is_ok();
    let path = resolve(input)?;
    if exists(&path) {
        return Ok((path, None));
    }
    if let Some((stem, location)) = split_location(input) {
        let path = resolve(stem)?;
        if exists(&path) {
            return Ok((path, Some(location)));
        }
    }
    Err(not_found(input))
}

fn split_location(input: &str) -> Option<(&str, Location)> {
    let number = |text: &str| {
        (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| text.parse::<u64>().ok())
            .flatten()
            .filter(|value| *value > 0)
    };
    let (rest, last) = input.rsplit_once(':')?;
    if rest.is_empty() {
        return None;
    }
    if let Some((line, end)) = last.split_once('-') {
        let (line, end) = (number(line)?, number(end)?);
        let end_line = Some(end.max(line));
        return Some((rest, Location { line, end_line }));
    }
    let last = number(last)?;
    if let Some((stem, line)) = rest.rsplit_once(':')
        && !stem.is_empty()
        && let Some(line) = number(line)
    {
        return Some((
            stem,
            Location {
                line,
                end_line: None,
            },
        ));
    }
    Some((
        rest,
        Location {
            line: last,
            end_line: None,
        },
    ))
}

fn binary(mut reply: Value) -> Value {
    reply["kind"] = json!("binary");
    reply
}

fn not_found(input: &str) -> RpcError {
    RpcError::app(-32000, "file_not_found", format!("File not found: {input}"))
}

fn failed(message: impl Into<String>) -> RpcError {
    RpcError::app(-32000, "file_read_failed", message)
}

fn io_error(error: &std::io::Error, input: &str) -> RpcError {
    match error.kind() {
        ErrorKind::NotFound => not_found(input),
        ErrorKind::PermissionDenied => failed(format!("Permission denied: {input}")),
        _ => failed(format!("Failed to read {input}: {error}")),
    }
}
