//! Keeps the Mac from idle-sleeping while a Session runs, as Claude Code's
//! desktop app does. The display may still sleep, and closing the lid still
//! sleeps the Mac.

use crate::{Desktop, commands::in_local_workspace};
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, WebviewWindow};

/// The `caffeinate` holding the assertion, while one does.
#[derive(Default)]
pub struct KeepAwake(Mutex<Option<Child>>);

impl KeepAwake {
    /// Holds the assertion while `active`. `caffeinate -w` also lets it go
    /// when Lacus exits without releasing it.
    pub fn set(&self, active: bool) -> std::io::Result<()> {
        let mut held = self.0.lock().unwrap();
        if let Some(child) = held.as_mut()
            && child.try_wait()?.is_some()
        {
            *held = None;
        }
        match (active, held.take()) {
            (true, Some(child)) => *held = Some(child),
            (true, None) => {
                *held = Some(
                    Command::new("/usr/bin/caffeinate")
                        .args(["-i", "-w", &std::process::id().to_string()])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()?,
                );
            }
            (false, Some(mut child)) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            (false, None) => {}
        }
        Ok(())
    }
}

/// The workspace page reports whether a Session is running.
#[tauri::command]
pub fn keep_awake(app: AppHandle, window: WebviewWindow, active: bool) -> Result<(), String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Keeping the computer awake is only available in the local workspace".into());
    }
    app.state::<Arc<Desktop>>()
        .awake
        .set(active)
        .map_err(|error| error.to_string())
}
