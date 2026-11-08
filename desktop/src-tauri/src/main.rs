//! Lacus for macOS: a Tauri window over the local Web workspace that the
//! bundled `dimcode` serves.

mod apps;
mod awake;
mod commands;
mod environment;
mod menu;
mod preview;
mod service;
mod update;
mod window;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64},
    },
};
use tauri::{Manager, RunEvent, WindowEvent};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// What the parts of Desktop share.
pub struct Desktop {
    service: Arc<service::Service>,
    window_state: Mutex<window::WindowState>,
    /// Attention requests since the window was last in front, shown on the
    /// Dock badge.
    unread: AtomicI64,
    /// A check for updates, an open offer or an install is under way.
    updating: AtomicBool,
    /// Application icons as data URLs, by bundle path; `None` when one has none.
    icons: Mutex<HashMap<PathBuf, Option<String>>>,
    awake: awake::KeepAwake,
    previews: preview::Previews,
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            window::show(app)
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            apps::installed_apps,
            apps::open_in_app,
            awake::keep_awake,
            commands::choose_workspace,
            commands::notify_attention,
            commands::open_workspace_path,
            preview::show_preview
        ])
        .on_menu_event(menu::handle)
        .setup(|app| {
            let home = match std::env::var_os("DIM_DESKTOP_HOME") {
                Some(home) => PathBuf::from(home),
                None => app.path().app_data_dir()?,
            };
            if !home.is_absolute() {
                return Err("DIM_DESKTOP_HOME must be an absolute path".into());
            }
            std::fs::create_dir_all(&home)?;
            let store = window::webview_store_id(&home).map_err(|error| error.to_string())?;
            let desktop = Arc::new(Desktop {
                window_state: Mutex::new(window::WindowState::load(&home)),
                service: Arc::new(service::Service::new(
                    home,
                    app.path().resource_dir()?,
                    std::env::var_os("DIMCODE_MODEL_CONFIG").map(PathBuf::from),
                )),
                unread: AtomicI64::new(0),
                updating: AtomicBool::new(false),
                icons: Mutex::default(),
                awake: awake::KeepAwake::default(),
                previews: preview::Previews::default(),
            });
            app.manage(desktop.clone());
            app.set_menu(menu::build(app.handle())?)?;
            window::build(app.handle(), &desktop, store).map_err(|error| error.to_string())?;
            desktop.service.start(app.handle());
            update::watch(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize the Lacus desktop");

    app.run(|app, event| match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if label == window::MAIN => {
            api.prevent_close();
            window::hide(app);
        }
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Focused(true),
            ..
        } if label == window::MAIN => commands::clear_attention(app),
        RunEvent::Reopen { .. } => window::show(app),
        RunEvent::Exit => {
            window::remember(app);
            let _ = app.state::<Arc<Desktop>>().awake.set(false);
            let service = app.state::<Arc<Desktop>>().service.clone();
            if let Err(error) = tauri::async_runtime::block_on(service.quit(app)) {
                eprintln!("Lacus shutdown: {error}");
            }
        }
        _ => {}
    });
}
