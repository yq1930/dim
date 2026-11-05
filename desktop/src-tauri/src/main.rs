use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::{
    ShellExt,
    process::{Command, CommandChild, CommandEvent},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn webview_store_id(home: &Path) -> Result<[u8; 16]> {
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

struct Desktop {
    home: PathBuf,
    resources: PathBuf,
    model_config: Option<PathBuf>,
    holder: Mutex<Option<CommandChild>>,
    workspace: Mutex<Option<Url>>,
    startup: tokio::sync::watch::Sender<bool>,
    quitting: AtomicBool,
}

impl Desktop {
    fn command(&self, app: &AppHandle) -> Result<Command> {
        let user_home = app.path().home_dir()?;
        let mut paths: Vec<_> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        for path in [
            user_home.join(".local/bin"),
            user_home.join(".cargo/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/sbin"),
        ] {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        let mut command = app
            .shell()
            .sidecar("dimcode")?
            .current_dir(user_home)
            .args([std::ffi::OsStr::new("--home"), self.home.as_os_str()])
            .env("PATH", std::env::join_paths(paths)?)
            .env("NO_COLOR", "1")
            .env("DIMCODE_WEB_ASSETS", self.resources.join("web"))
            .env(
                "DIMCODE_DEFAULT_PLUGINS",
                self.resources.join("plugins-payload/default-plugins.json"),
            );
        if let Some(path) = &self.model_config {
            command = command.args([std::ffi::OsStr::new("--model-config"), path.as_os_str()]);
        }
        Ok(command)
    }

    fn finish_startup(&self) {
        self.startup.send_replace(true);
    }
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
async fn choose_workspace(
    app: AppHandle,
    window: tauri::WebviewWindow,
    initial_directory: Option<String>,
) -> std::result::Result<Option<String>, String> {
    let url = window.url().map_err(|error| error.to_string())?;
    if window.label() != "main"
        || !app
            .state::<Arc<Desktop>>()
            .workspace
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|workspace| workspace.origin() == url.origin())
    {
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

fn open_external(app: &AppHandle, url: &Url) {
    if matches!(url.scheme(), "http" | "https") {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    }
}

fn fail(app: &AppHandle, message: impl Into<String>) {
    let state = app.state::<Arc<Desktop>>();
    state.finish_startup();
    if state.quitting.load(Ordering::Acquire) {
        return;
    }
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Dim 无法连接本地服务")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

async fn hold_workspace(app: AppHandle, state: Arc<Desktop>) -> Result<()> {
    for file in ["web/index.html", "plugins-payload/default-plugins.json"] {
        if !state.resources.join(file).is_file() {
            return Err(format!("应用缺少资源 {file}，请重新构建 Dim.app。").into());
        }
    }
    let (mut events, child) = state.command(&app)?.args(["web", "--no-open"]).spawn()?;
    *state.holder.lock().unwrap() = Some(child);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut stderr = Vec::new();
    let mut connected = false;
    loop {
        let event = if connected {
            events.recv().await
        } else {
            tokio::time::timeout_at(deadline, events.recv())
                .await
                .map_err(|_| "本地服务启动超时，请检查应用数据目录中的 rust/daemon.log。")?
        };
        match event {
            Some(CommandEvent::Stdout(bytes)) if !connected => {
                let line = String::from_utf8_lossy(&bytes);
                let Some(url) = line.split_whitespace().find_map(|part| {
                    let url = Url::parse(part).ok()?;
                    (url.scheme() == "http"
                        && url.host_str() == Some("127.0.0.1")
                        && url.port().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.path() == "/"
                        && url.query().is_none()
                        && url
                            .fragment()
                            .is_some_and(|value| value.starts_with("bootstrap=")))
                    .then_some(url)
                }) else {
                    continue;
                };
                *state.workspace.lock().unwrap() = Some(url.clone());
                app.add_capability(
                    tauri::ipc::CapabilityBuilder::new("workspace-picker")
                        .window("main")
                        .local(false)
                        .remote(format!("{}/*", url.origin().ascii_serialization()))
                        .permission("allow-choose-workspace"),
                )?;
                connected = true;
                state.finish_startup();
                if !state.quitting.load(Ordering::Acquire) {
                    app.get_webview_window("main")
                        .ok_or("桌面窗口已经关闭。")?
                        .navigate(url)?;
                }
            }
            Some(CommandEvent::Stderr(bytes)) => {
                stderr.extend(bytes);
                stderr.push(b'\n');
                if stderr.len() > 8192 {
                    stderr.drain(..stderr.len() - 8192);
                }
            }
            Some(CommandEvent::Error(error)) => return Err(error.into()),
            Some(CommandEvent::Terminated(_)) | None => {
                if state.quitting.load(Ordering::Acquire) {
                    return Ok(());
                }
                return Err(format!(
                    "本地服务已停止。\n{}\n日志：{}",
                    String::from_utf8_lossy(&stderr).trim(),
                    state.home.join("rust/daemon.log").display()
                )
                .into());
            }
            _ => {}
        }
    }
}

async fn stop_workspace(app: &AppHandle, state: &Desktop) -> Result<()> {
    let mut startup = state.startup.subscribe();
    let _ = tokio::time::timeout(Duration::from_secs(30), startup.wait_for(|ready| *ready)).await;
    let result: Result<()> = async {
        let (mut events, child) = state.command(app)?.args(["daemon", "stop"]).spawn()?;
        let stopped = tokio::time::timeout(Duration::from_secs(35), async {
            while let Some(event) = events.recv().await {
                match event {
                    CommandEvent::Terminated(status) => return status.code == Some(0),
                    CommandEvent::Error(_) => return false,
                    _ => {}
                }
            }
            false
        })
        .await;
        let _ = child.kill();
        if !matches!(stopped, Ok(true)) {
            return Err("本地服务未确认关闭，请检查应用数据目录中的 rust/daemon.log。".into());
        }
        Ok(())
    }
    .await;
    if let Some(holder) = state.holder.lock().unwrap().take() {
        let _ = holder.kill();
    }
    result
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main(app)
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![choose_workspace])
        .setup(|app| {
            let home = std::env::var_os("DIM_DESKTOP_HOME")
                .map(PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            if !home.is_absolute() {
                return Err("DIM_DESKTOP_HOME must be an absolute path".into());
            }
            std::fs::create_dir_all(&home)?;
            let webview_id = webview_store_id(&home).map_err(|error| error.to_string())?;
            let state = Arc::new(Desktop {
                home,
                resources: app.path().resource_dir()?,
                model_config: std::env::var_os("DIMCODE_MODEL_CONFIG").map(PathBuf::from),
                holder: Mutex::new(None),
                workspace: Mutex::new(None),
                startup: tokio::sync::watch::channel(false).0,
                quitting: AtomicBool::new(false),
            });
            app.manage(state.clone());
            let navigation = app.handle().clone();
            let popups = app.handle().clone();
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Dim")
                .inner_size(1320.0, 880.0)
                .min_inner_size(840.0, 600.0)
                .center()
                .data_store_identifier(webview_id)
                .disable_drag_drop_handler()
                .on_navigation(move |url| {
                    if url.scheme() == "tauri" && url.host_str() == Some("localhost") {
                        return true;
                    }
                    let state = navigation.state::<Arc<Desktop>>();
                    if state
                        .workspace
                        .lock()
                        .unwrap()
                        .as_ref()
                        .is_some_and(|workspace| workspace.origin() == url.origin())
                    {
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
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = hold_workspace(handle.clone(), state.clone()).await;
                state.finish_startup();
                if let Err(error) = result {
                    fail(&handle, error.to_string());
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize Dim desktop");

    app.run(|app, event| match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if label == "main" => {
            api.prevent_close();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
        }
        RunEvent::Reopen { .. } => show_main(app),
        RunEvent::Exit => {
            let state = app.state::<Arc<Desktop>>().inner().clone();
            state.quitting.store(true, Ordering::Release);
            if let Err(error) = tauri::async_runtime::block_on(stop_workspace(app, &state)) {
                eprintln!("Dim shutdown: {error}");
            }
        }
        _ => {}
    });
}
