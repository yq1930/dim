//! The native commands the workspace page may call. Each answers only the
//! main window while it shows the local service's origin.

use crate::{Desktop, window};
use std::{
    path::Path,
    sync::{Arc, atomic::Ordering},
};
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

fn in_local_workspace(app: &AppHandle, window: &WebviewWindow) -> Result<bool, String> {
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
