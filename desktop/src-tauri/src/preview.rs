//! The dev servers on this computer the workspace page previews in a frame.
//! WebKit hands the main window's navigation rule its frames' navigations as
//! well, so a previewed origin is let through instead of opening in the
//! default browser.

use crate::{Desktop, commands::in_local_workspace};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, Url, WebviewWindow};

/// The origins the preview pane has shown.
#[derive(Default)]
pub struct Previews(Mutex<HashSet<String>>);

impl Previews {
    pub fn allows(&self, url: &Url) -> bool {
        local(url)
            && self
                .0
                .lock()
                .unwrap()
                .contains(&url.origin().ascii_serialization())
    }
}

/// An http or https address on this computer.
fn local(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

/// The page is about to show `url` in its preview frame: let its origin through.
#[tauri::command]
pub fn show_preview(app: AppHandle, window: WebviewWindow, url: String) -> Result<(), String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Previews are only available in the local workspace".into());
    }
    let url = Url::parse(&url).map_err(|error| error.to_string())?;
    if !local(&url) {
        return Err(format!("{url} is not on this computer"));
    }
    app.state::<Arc<Desktop>>()
        .previews
        .0
        .lock()
        .unwrap()
        .insert(url.origin().ascii_serialization());
    Ok(())
}
