//! Lacus for macOS: a Tauri window over the local Web workspace that the
//! bundled `dimcode` serves.

mod commands;
mod environment;
mod menu;
mod service;
mod window;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicI64},
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
        .invoke_handler(tauri::generate_handler![
            commands::choose_workspace,
            commands::notify_attention
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
            });
            app.manage(desktop.clone());
            app.set_menu(menu::build(app.handle())?)?;
            window::build(app.handle(), &desktop, store).map_err(|error| error.to_string())?;
            desktop.service.start(app.handle());
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
            let service = app.state::<Arc<Desktop>>().service.clone();
            if let Err(error) = tauri::async_runtime::block_on(service.quit(app)) {
                eprintln!("Lacus shutdown: {error}");
            }
        }
        _ => {}
    });
}
