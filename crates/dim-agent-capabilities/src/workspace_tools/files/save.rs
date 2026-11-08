//! `session.files.save`: the operator's edit of a workspace file, written from
//! the web workspace's file pane. It replaces the file as `write` does, under
//! the same per-path lock, but only over the revision the editor started from,
//! and records nothing for FileReads or file rewind: a later `write` or `edit`
//! of the model finds the file changed since it read it.

use super::super::file_reads::TextEncoding;
use super::{
    Cancelled, FileTools, NotRegular, codec, hex, open_regular, resolve_path, revision,
    write::write_target,
};
use crate::methods::RpcError;
use dim_agent_types::{ToolContent, ToolResult};
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tokio_util::sync::CancellationToken;

/// The editor works on files the file view shows whole.
const SAVE_MAX_BYTES: u64 = 2 * 1024 * 1024;

impl FileTools {
    /// Replaces the UTF-8 text file `input` names under the workspace with
    /// `text` when its bytes still have the `expected` revision (any, when
    /// `None`). The file keeps its UTF-8 BOM, and its CRLF line endings when
    /// `text` has none of its own.
    pub(crate) async fn save(
        &self,
        input: String,
        text: String,
        expected: Option<String>,
        cancel: CancellationToken,
    ) -> Result<Value, RpcError> {
        let (cwd, home) = (self.cwd.clone(), self.home.clone());
        let path = tokio::task::spawn_blocking(move || target(&cwd, &home, &input))
            .await
            .map_err(|error| failed(error.to_string()))??;
        let _lock = self
            .locks
            .lock(&path, &cancel)
            .await
            .map_err(|_| failed("The save was cancelled"))?;
        tokio::task::spawn_blocking(move || replace(&path, &text, expected.as_deref(), &cancel))
            .await
            .map_err(|error| failed(error.to_string()))?
    }
}

/// The existing file under the workspace that `input` names.
fn target(cwd: &Path, home: &Path, input: &str) -> Result<PathBuf, RpcError> {
    let path = resolve_path(cwd, home, input).map_err(failed)?;
    let root = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    if !path.starts_with(&root) {
        return Err(RpcError::app(
            -32000,
            "file_outside_workspace",
            format!("{} is outside the Session's workspace", path.display()),
        ));
    }
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => Ok(path),
        Ok(_) => Err(not_editable(format!("Not a regular file: {input}"))),
        Err(error) if error.kind() == ErrorKind::NotFound => Err(RpcError::app(
            -32000,
            "file_not_found",
            format!("File not found: {input}"),
        )),
        Err(error) => Err(failed(error.to_string())),
    }
}

fn replace(
    path: &Path,
    text: &str,
    expected: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Value, RpcError> {
    let display = path.to_string_lossy().into_owned();
    let mut original = match open_regular(path) {
        Ok(Ok(file)) => file,
        Ok(Err(NotRegular)) => return Err(not_editable(format!("Not a regular file: {display}"))),
        Err(error) => return Err(failed(error.to_string())),
    };
    let mut current = Vec::new();
    (&mut original)
        .take(SAVE_MAX_BYTES + 1)
        .read_to_end(&mut current)
        .map_err(|error| failed(error.to_string()))?;
    if current.len() as u64 > SAVE_MAX_BYTES {
        return Err(not_editable(format!("{display} is too large to edit here")));
    }
    let current_revision = revision(&current);
    if expected.is_some_and(|expected| expected != hex(&current_revision)) {
        return Err(changed(&display));
    }
    let resolved = codec::resolve_encoding(&current, None)
        .map_err(|_| not_editable(format!("{display} is not UTF-8 text")))?;
    let bom = codec::bom_bytes(resolved.bom);
    if !matches!(resolved.encoding, TextEncoding::Utf8)
        || current.contains(&0)
        || std::str::from_utf8(&current[bom.len()..]).is_err()
    {
        return Err(not_editable(format!("{display} is not UTF-8 text")));
    }
    let crlf = !text.contains('\r') && current.windows(2).any(|pair| pair == b"\r\n");
    let body = if crlf {
        text.replace('\n', "\r\n")
    } else {
        text.to_owned()
    };
    let bytes = [bom, body.as_bytes()].concat();
    match write_target(
        path,
        &bytes,
        &display,
        Some(&original),
        &current_revision,
        cancel,
        #[cfg(test)]
        None,
    ) {
        Ok(Ok(())) => {}
        Ok(Err(result)) => return Err(write_failed(result, &display)),
        Err(Cancelled) => return Err(failed("The save was cancelled")),
    }
    let metadata = std::fs::metadata(path).map_err(|error| failed(error.to_string()))?;
    let mut reply = json!({ "size": metadata.len(), "revision": hex(&revision(&bytes)) });
    if let Some(modified) = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
    {
        reply["modified"] = json!(modified.as_millis() as u64);
    }
    Ok(reply)
}

/// A refused `write_target`: `file_changed` when the file moved under the
/// save, else the write's own message.
fn write_failed(result: ToolResult, display: &str) -> RpcError {
    let code = result
        .structured_content
        .as_ref()
        .and_then(|fields| fields.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if code == "file_modified_since_read" {
        return changed(display);
    }
    match &result.content {
        ToolContent::Text(text) => failed(text.clone()),
        _ => failed("The file could not be saved"),
    }
}

fn changed(display: &str) -> RpcError {
    RpcError::app(
        -32000,
        "file_changed",
        format!("{display} changed on disk since it was opened"),
    )
}

fn not_editable(message: impl Into<String>) -> RpcError {
    RpcError::app(-32000, "file_not_editable", message)
}

fn failed(message: impl Into<String>) -> RpcError {
    RpcError::app(-32000, "file_save_failed", message)
}
