//! The main window: where it may navigate, the size and zoom it reopens with,
//! and how closing hides it.

use crate::{Desktop, Result};
use std::{io::Write, path::Path, sync::Arc};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

pub const MAIN: &str = "main";
/// The page the window shows until the local service answers.
pub const BOOT_URL: &str = "tauri://localhost/index.html";
const STATE_FILE: &str = "desktop-state.json";
const DEFAULT_SIZE: (f64, f64) = (1320.0, 880.0);
const MIN_SIZE: (f64, f64) = (840.0, 600.0);
/// Zoom is a step count; each step scales the page by `ZOOM_STEP`.
const ZOOM_LEVELS: std::ops::RangeInclusive<i32> = -3..=5;
const ZOOM_STEP: f64 = 1.1;

/// What the window reopens with, kept in the Desktop home.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WindowState {
    /// The size of the window when it is not maximized, in points; a
    /// maximized window keeps the size it returns to.
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
    pub zoom: i32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: DEFAULT_SIZE.0,
            height: DEFAULT_SIZE.1,
            maximized: false,
            zoom: 0,
        }
    }
}

impl WindowState {
    /// The saved state, or the default when there is none or it is unreadable.
    pub fn load(home: &Path) -> Self {
        std::fs::read(home.join(STATE_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .unwrap_or_default()
            .normalized()
    }

    /// Replaces the saved state; a reader never sees a partial file.
    pub fn save(&self, home: &Path) -> std::io::Result<()> {
        let staged = home.join(format!("{STATE_FILE}.tmp"));
        let mut file = std::fs::File::create(&staged)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.sync_all()?;
        std::fs::rename(staged, home.join(STATE_FILE))
    }

    fn normalized(self) -> Self {
        let length = |value: f64, default: f64, min: f64| {
            if value.is_finite() && value > 0.0 {
                value.max(min)
            } else {
                default
            }
        };
        Self {
            width: length(self.width, DEFAULT_SIZE.0, MIN_SIZE.0),
            height: length(self.height, DEFAULT_SIZE.1, MIN_SIZE.1),
            maximized: self.maximized,
            zoom: self.zoom.clamp(*ZOOM_LEVELS.start(), *ZOOM_LEVELS.end()),
        }
    }

    fn zoom_factor(&self) -> f64 {
        ZOOM_STEP.powi(self.zoom)
    }
}

/// A zoom command from the View menu.
#[derive(Debug, Clone, Copy)]
pub enum Zoom {
    In,
    Out,
    Reset,
}

/// The UUID naming the macOS-managed WebKit store of this Desktop home, so
/// homes keep separate cookies and page preferences.
pub fn webview_store_id(home: &Path) -> Result<[u8; 16]> {
    let file = home.join("webview-id");
    match std::fs::read_to_string(&file) {
        Ok(value) => Ok(uuid::Uuid::parse_str(value.trim())?.into_bytes()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let id = uuid::Uuid::new_v4();
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(file)?;
            writeln!(file, "{id}")?;
            Ok(id.into_bytes())
        }
        Err(error) => Err(error.into()),
    }
}

/// Opens the main window on the boot page. It may navigate to the boot page
/// and the local service's origin; other web links open in the default
/// browser, and no page opens a window of its own.
pub fn build(app: &AppHandle, desktop: &Arc<Desktop>, store: [u8; 16]) -> Result<WebviewWindow> {
    let state = *desktop.window_state.lock().unwrap();
    let navigation = app.clone();
    let popups = app.clone();
    let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("Lacus")
        .inner_size(state.width, state.height)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .center()
        .maximized(state.maximized)
        .data_store_identifier(store)
        .disable_drag_drop_handler()
        .on_navigation(move |url| {
            if url.scheme() == "tauri" && url.host_str() == Some("localhost") {
                return true;
            }
            if navigation.state::<Arc<Desktop>>().service.in_workspace(url) {
                return true;
            }
            open_external(&navigation, url);
            false
        })
        .on_new_window(move |url, _| {
            open_external(&popups, &url);
            tauri::webview::NewWindowResponse::Deny
        })
        .build()?;
    let _ = window.set_zoom(state.zoom_factor());
    Ok(window)
}

fn open_external(app: &AppHandle, url: &Url) {
    if matches!(url.scheme(), "http" | "https") {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    }
}

pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Closing the window hides it; the service and its sessions keep running
/// until the app quits.
pub fn hide(app: &AppHandle) {
    remember(app);
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

/// Saves the window's current size and whether it is maximized.
pub fn remember(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let desktop = app.state::<Arc<Desktop>>();
    let mut state = desktop.window_state.lock().unwrap();
    let maximized = window.is_maximized().unwrap_or(false);
    let normal = !maximized
        && !window.is_fullscreen().unwrap_or(false)
        && !window.is_minimized().unwrap_or(false);
    if normal && let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
        // macOS can report a point more than the size the window opened
        // with; keeping such a difference would grow the window each launch.
        let size = size.to_logical::<f64>(scale);
        if (size.width - state.width).abs() > 1.0 || (size.height - state.height).abs() > 1.0 {
            state.width = size.width;
            state.height = size.height;
        }
    }
    state.maximized = maximized;
    *state = state.normalized();
    if let Err(error) = state.save(desktop.service.home()) {
        eprintln!("Lacus: cannot save the window state: {error}");
    }
}

/// Applies a zoom command to the page and keeps the level for later launches.
pub fn zoom(app: &AppHandle, zoom: Zoom) {
    let desktop = app.state::<Arc<Desktop>>();
    let mut state = desktop.window_state.lock().unwrap();
    state.zoom = match zoom {
        Zoom::In => state.zoom + 1,
        Zoom::Out => state.zoom - 1,
        Zoom::Reset => 0,
    };
    *state = state.normalized();
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.set_zoom(state.zoom_factor());
    }
    if let Err(error) = state.save(desktop.service.home()) {
        eprintln!("Lacus: cannot save the window state: {error}");
    }
}
