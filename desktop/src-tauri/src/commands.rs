//! The native commands the workspace page may call. Each answers only the
//! main window while it shows the local service's origin.

use crate::{Desktop, window};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// Kinds macOS runs, installs or follows when opened; these are only shown in Finder.
const RUNNABLE: [&str; 17] = [
    "app",
    "action",
    "applescript",
    "command",
    "dmg",
    "fileloc",
    "inetloc",
    "jar",
    "mpkg",
    "pkg",
    "prefpane",
    "saver",
    "scpt",
    "terminal",
    "tool",
    "webloc",
    "workflow",
];

pub(crate) fn in_local_workspace(app: &AppHandle, window: &WebviewWindow) -> Result<bool, String> {
    let url = window.url().map_err(|error| error.to_string())?;
    Ok(window.label() == window::MAIN && app.state::<Arc<Desktop>>().service.in_workspace(&url))
}

/// Notifies the user when the window is not in front, and counts the
/// request on the Dock badge until the window is in front again.
#[tauri::command]
pub fn notify_attention(
    app: AppHandle,
    window: WebviewWindow,
    title: String,
    body: String,
) -> Result<(), String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Notifications are only available in the local workspace".into());
    }
    if window.is_focused().unwrap_or(false) {
        return Ok(());
    }
    let _ = window.request_user_attention(Some(tauri::UserAttentionType::Informational));
    let unread = app
        .state::<Arc<Desktop>>()
        .unread
        .fetch_add(1, Ordering::AcqRel)
        + 1;
    let _ = window.set_badge_count(Some(unread));
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| error.to_string())
}

/// Clears the Dock badge once the window is in front.
pub fn clear_attention(app: &AppHandle) {
    if app.state::<Arc<Desktop>>().unread.swap(0, Ordering::AcqRel) > 0
        && let Some(window) = app.get_webview_window(window::MAIN)
    {
        let _ = window.set_badge_count(None);
    }
}

/// The macOS folder picker, attached to the main window.
#[tauri::command]
pub async fn choose_workspace(
    app: AppHandle,
    window: WebviewWindow,
    initial_directory: Option<String>,
) -> Result<Option<String>, String> {
    if !in_local_workspace(&app, &window)? {
        return Err("The folder picker is only available in the local workspace".into());
    }
    let mut dialog = app.dialog().file().set_parent(&window);
    if let Some(path) = initial_directory.filter(|path| Path::new(path).is_absolute()) {
        dialog = dialog.set_directory(path);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    dialog.pick_folder(move |folder| {
        let _ = sender.send(folder);
    });
    receiver
        .await
        .map_err(|error| error.to_string())?
        .map(|folder| {
            let path = folder.into_path().map_err(|error| error.to_string())?;
            path.into_os_string()
                .into_string()
                .map_err(|_| "The folder path is not valid UTF-8".into())
        })
        .transpose()
}

/// Opens a path the workspace page links to with its default application,
/// or shows it in Finder: when `reveal` is set, for a folder or package, for
/// anything that could run, and when no application opens it. A `:line` or
/// `:line:column` suffix that is not part of the name is dropped.
#[tauri::command]
pub fn open_workspace_path(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
    reveal: bool,
) -> Result<(), String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Opening files is only available in the local workspace".into());
    }
    let target = existing(&path).ok_or_else(|| format!("{path} does not exist"))?;
    let opener = app.opener();
    if reveal || !openable(&target) {
        return opener
            .reveal_item_in_dir(&target)
            .map_err(|error| error.to_string());
    }
    opener
        .open_path(target.to_string_lossy(), None::<&str>)
        .or_else(|_| opener.reveal_item_in_dir(&target))
        .map_err(|error| error.to_string())
}

/// `path` when it exists, else `path` without a trailing `:line[:column]`.
fn existing(path: &str) -> Option<PathBuf> {
    let mut candidate = path;
    for _ in 0..3 {
        if Path::new(candidate).is_absolute() && Path::new(candidate).exists() {
            return Some(PathBuf::from(candidate));
        }
        let (head, number) = candidate.rsplit_once(':')?;
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        candidate = head;
    }
    None
}

/// A regular file that is not executable and not of a kind that runs.
fn openable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let runnable = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| RUNNABLE.contains(&extension.to_ascii_lowercase().as_str()));
    meta.is_file() && meta.permissions().mode() & 0o111 == 0 && !runnable
}
