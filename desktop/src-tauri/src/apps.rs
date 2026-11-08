//! The applications the workspace page opens a folder in, as Claude Code's
//! "Open in" does: the known editors and terminals installed, and Finder.

use crate::{Desktop, commands::in_local_workspace};
use base64::Engine;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};
use tauri::{AppHandle, Manager, WebviewWindow};

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Editor,
    Terminal,
    Finder,
}

/// The applications offered, by bundle identifier, in menu order; each
/// opens a folder handed to it by `open -b`.
const KNOWN: [(&str, &str, Kind); 20] = [
    ("com.microsoft.VSCode", "VS Code", Kind::Editor),
    (
        "com.microsoft.VSCodeInsiders",
        "VS Code Insiders",
        Kind::Editor,
    ),
    ("com.todesktop.230313mzl4w4u92", "Cursor", Kind::Editor),
    ("com.exafunction.windsurf", "Windsurf", Kind::Editor),
    ("dev.zed.Zed", "Zed", Kind::Editor),
    ("com.sublimetext.4", "Sublime Text", Kind::Editor),
    ("com.jetbrains.intellij", "IntelliJ IDEA", Kind::Editor),
    (
        "com.jetbrains.intellij.ce",
        "IntelliJ IDEA CE",
        Kind::Editor,
    ),
    ("com.jetbrains.WebStorm", "WebStorm", Kind::Editor),
    ("com.jetbrains.pycharm", "PyCharm", Kind::Editor),
    ("com.jetbrains.pycharm.ce", "PyCharm CE", Kind::Editor),
    ("com.jetbrains.goland", "GoLand", Kind::Editor),
    ("com.jetbrains.rustrover", "RustRover", Kind::Editor),
    ("com.google.android.studio", "Android Studio", Kind::Editor),
    ("com.apple.dt.Xcode", "Xcode", Kind::Editor),
    ("com.apple.Terminal", "Terminal", Kind::Terminal),
    ("com.googlecode.iterm2", "iTerm", Kind::Terminal),
    ("com.mitchellh.ghostty", "Ghostty", Kind::Terminal),
    ("dev.warp.Warp-Stable", "Warp", Kind::Terminal),
    ("com.apple.finder", "Finder", Kind::Finder),
];

#[derive(Serialize)]
pub struct InstalledApp {
    id: &'static str,
    name: &'static str,
    kind: Kind,
    /// A 64-pixel PNG data URL of the application's icon.
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

/// The known applications installed, with their icons.
#[tauri::command]
pub async fn installed_apps(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Vec<InstalledApp>, String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Applications are only listed for the local workspace".into());
    }
    let desktop = app.state::<Arc<Desktop>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        KNOWN
            .iter()
            .filter_map(|&(id, name, kind)| {
                let bundle = locate(id)?;
                Some(InstalledApp {
                    id,
                    name,
                    kind,
                    icon: desktop.icon(&bundle),
                })
            })
            .collect()
    })
    .await
    .map_err(|error| error.to_string())
}

/// Opens an absolute path in a known application. Finder shows a file in
/// its folder rather than opening it.
#[tauri::command]
pub async fn open_in_app(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    path: String,
) -> Result<(), String> {
    if !in_local_workspace(&app, &window)? {
        return Err("Opening other applications is only available in the local workspace".into());
    }
    let &(id, _, kind) = KNOWN
        .iter()
        .find(|(known, _, _)| *known == id)
        .ok_or_else(|| format!("{id} is not an application Lacus opens folders in"))?;
    let target = PathBuf::from(&path);
    if !target.is_absolute() || !target.exists() {
        return Err(format!("{path} does not exist"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = Command::new("/usr/bin/open");
        if kind == Kind::Finder && !target.is_dir() {
            command.arg("-R");
        } else {
            command.args(["-b", id]);
        }
        let output = command
            .arg(&target)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Where Launch Services finds the application with this bundle identifier.
fn locate(id: &str) -> Option<PathBuf> {
    objc2::rc::autoreleasepool(|_| {
        let url = NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(id))?;
        Some(PathBuf::from(url.path()?.to_string()))
    })
}

impl Desktop {
    /// The icon of an application bundle, converted once per bundle path.
    fn icon(&self, bundle: &Path) -> Option<String> {
        if let Some(icon) = self.icons.lock().unwrap().get(bundle) {
            return icon.clone();
        }
        let icon = convert_icon(bundle);
        self.icons
            .lock()
            .unwrap()
            .insert(bundle.to_owned(), icon.clone());
        icon
    }
}

/// The bundle's `CFBundleIconFile` as a 64-pixel PNG data URL, by `sips`.
fn convert_icon(bundle: &Path) -> Option<String> {
    let info = plist::Value::from_file(bundle.join("Contents/Info.plist")).ok()?;
    let name = info.as_dictionary()?.get("CFBundleIconFile")?.as_string()?;
    let mut source = bundle.join("Contents/Resources").join(name);
    if source.extension().is_none() {
        source.set_extension("icns");
    }
    let output = std::env::temp_dir().join(format!("lacus-icon-{}.png", uuid::Uuid::new_v4()));
    let converted = Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", "-Z", "64"])
        .arg(&source)
        .arg("--out")
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let png = converted.then(|| std::fs::read(&output).ok()).flatten();
    let _ = std::fs::remove_file(&output);
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png?)
    ))
}
