//! `session.files.list`: one folder of the Session's workspace, as the web
//! workspace's file tree shows it: the entries ignore files leave, dotfiles
//! included, folders first.

use super::resolve_path;
use crate::methods::RpcError;
use ignore::WalkBuilder;
use serde_json::{Value, json};
use std::path::Path;

/// A folder lists at most this many entries.
const MAX_ENTRIES: usize = 2000;
/// Never listed.
const UNLISTED: [&str; 2] = [".git", ".DS_Store"];

/// Lists the folder `input` names, relative to `cwd` or absolute; the empty
/// path is the workspace root. Only folders under `cwd` are listed.
pub(crate) fn list(cwd: &Path, home: &Path, input: &str) -> Result<Value, RpcError> {
    let root = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    let folder = if input.is_empty() {
        root.clone()
    } else {
        resolve_path(cwd, home, input).map_err(failed)?
    };
    let Ok(relative) = folder.strip_prefix(&root) else {
        return Err(RpcError::app(
            -32000,
            "file_outside_workspace",
            format!("{} is outside the Session's workspace", folder.display()),
        ));
    };
    match std::fs::metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Err(failed(format!("Not a folder: {input}"))),
        Err(_) => {
            return Err(RpcError::app(
                -32000,
                "file_not_found",
                format!("Folder not found: {input}"),
            ));
        }
    }
    let walker = WalkBuilder::new(&folder)
        .max_depth(Some(1))
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .require_git(false)
        .parents(true)
        .follow_links(false)
        .build();
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if UNLISTED.contains(&name) {
            continue;
        }
        if entries.len() == MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push((name.to_owned(), entry.path().is_dir()));
    }
    entries.sort_by(|(left, left_dir), (right, right_dir)| {
        right_dir
            .cmp(left_dir)
            .then_with(|| left.to_lowercase().cmp(&right.to_lowercase()))
    });
    Ok(json!({
        "path": folder.to_string_lossy(),
        "relativePath": relative.to_string_lossy(),
        "entries": entries.into_iter().map(|(name, dir)| json!({ "name": name, "dir": dir })).collect::<Vec<_>>(),
        "truncated": truncated,
    }))
}

fn failed(message: impl Into<String>) -> RpcError {
    RpcError::app(-32000, "file_list_failed", message)
}
