//! The local service: the bundled `dimcode web --no-open` under the Desktop
//! home, the workspace URL it prints, and what happens when it stops.

use crate::{Result, environment, window};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_shell::{
    ShellExt,
    process::{Command, CommandChild, CommandEvent},
};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STOP_TIMEOUT: Duration = Duration::from_secs(35);
/// How much of the service's stderr a failure shows.
const STDERR_TAIL: usize = 8192;

/// Why a run ended without Desktop asking it to.
enum Failure {
    Startup(String),
    Stopped(String),
}

pub struct Service {
    home: PathBuf,
    resources: PathBuf,
    model_config: Option<PathBuf>,
    holder: Mutex<Option<CommandChild>>,
    workspace: Mutex<Option<Url>>,
    /// Origins the native commands are already granted to.
    granted: Mutex<HashSet<String>>,
    /// Numbers the runs; the latest started run is the one a stop waits for.
    started: AtomicU64,
    /// The run whose exit is a failure; zero while a restart or quit is under way.
    current: AtomicU64,
    /// The latest run whose start has connected or failed.
    settled: tokio::sync::watch::Sender<u64>,
    quitting: AtomicBool,
}

impl Service {
    pub fn new(home: PathBuf, resources: PathBuf, model_config: Option<PathBuf>) -> Self {
        Self {
            home,
            resources,
            model_config,
            holder: Mutex::new(None),
            workspace: Mutex::new(None),
            granted: Mutex::new(HashSet::new()),
            started: AtomicU64::new(0),
            current: AtomicU64::new(0),
            settled: tokio::sync::watch::channel(0).0,
            quitting: AtomicBool::new(false),
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn log(&self) -> PathBuf {
        self.home.join("rust/daemon.log")
    }

    /// Whether `url` is on the origin of the running service's workspace.
    pub fn in_workspace(&self, url: &Url) -> bool {
        self.workspace
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|workspace| workspace.origin() == url.origin())
    }

    /// Starts a run and opens its workspace in the main window once the
    /// service prints it.
    pub fn start(self: &Arc<Self>, app: &AppHandle) {
        let run = self.started.fetch_add(1, Ordering::AcqRel) + 1;
        self.current.store(run, Ordering::Release);
        let (service, app) = (self.clone(), app.clone());
        tauri::async_runtime::spawn(async move {
            let result = service.hold(&app, run).await;
            service.settle(run);
            if let Err(failure) = result
                && service.current.load(Ordering::Acquire) == run
                && !service.quitting.load(Ordering::Acquire)
            {
                service.report(&app, failure);
            }
        });
    }

    /// Stops the current run and starts a new one; the window shows the boot
    /// page meanwhile. A restart reads the login shell and system proxy again.
    pub fn restart(self: &Arc<Self>, app: &AppHandle) {
        self.current.store(0, Ordering::Release);
        *self.workspace.lock().unwrap() = None;
        if let Some(window) = app.get_webview_window(window::MAIN)
            && let Ok(boot) = Url::parse(window::BOOT_URL)
        {
            let _ = window.navigate(boot);
        }
        let (service, app) = (self.clone(), app.clone());
        tauri::async_runtime::spawn(async move {
            if let Err(error) = service.stop(&app).await {
                eprintln!("Dim restart: {error}");
            }
            if !service.quitting.load(Ordering::Acquire) {
                service.start(&app);
            }
        });
    }

    /// Stops the service for good; Desktop is exiting.
    pub async fn quit(&self, app: &AppHandle) -> Result<()> {
        self.quitting.store(true, Ordering::Release);
        self.current.store(0, Ordering::Release);
        self.stop(app).await
    }

    fn settle(&self, run: u64) {
        self.settled.send_if_modified(|settled| {
            let newer = run > *settled;
            if newer {
                *settled = run;
            }
            newer
        });
    }

    /// The bundled CLI on this Desktop home, before its subcommand.
    fn command(&self, app: &AppHandle) -> Result<Command> {
        let mut command = app
            .shell()
            .sidecar("dimcode")?
            .current_dir(app.path().home_dir()?)
            .args([std::ffi::OsStr::new("--home"), self.home.as_os_str()]);
        if let Some(path) = &self.model_config {
            command = command.args([std::ffi::OsStr::new("--model-config"), path.as_os_str()]);
        }
        Ok(command)
    }

    async fn hold(&self, app: &AppHandle, run: u64) -> std::result::Result<(), Failure> {
        let startup =
            |error: Box<dyn std::error::Error + Send + Sync>| Failure::Startup(error.to_string());
        for file in ["web/index.html", "plugins-payload/default-plugins.json"] {
            if !self.resources.join(file).is_file() {
                return Err(Failure::Startup(format!(
                    "应用缺少资源 {file}，请重新构建 Dim.app。"
                )));
            }
        }
        let user_home = app
            .path()
            .home_dir()
            .map_err(|error| startup(error.into()))?;
        let environment = tauri::async_runtime::spawn_blocking(move || {
            environment::service_environment(&user_home)
        })
        .await
        .map_err(|error| startup(error.into()))?;
        let (mut events, child) = self
            .command(app)
            .map_err(startup)?
            .envs(environment)
            .env("NO_COLOR", "1")
            .env("DIMCODE_WEB_ASSETS", self.resources.join("web"))
            .env(
                "DIMCODE_DEFAULT_PLUGINS",
                self.resources.join("plugins-payload/default-plugins.json"),
            )
            .args(["web", "--no-open"])
            .spawn()
            .map_err(|error| startup(error.into()))?;
        *self.holder.lock().unwrap() = Some(child);
        let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
        let mut stderr = Vec::new();
        let mut connected = false;
        loop {
            let event = if connected {
                events.recv().await
            } else {
                tokio::time::timeout_at(deadline, events.recv())
                    .await
                    .map_err(|_| {
                        Failure::Startup(
                            "本地服务启动超时，请检查应用数据目录中的 rust/daemon.log。".into(),
                        )
                    })?
            };
            match event {
                Some(CommandEvent::Stdout(bytes)) if !connected => {
                    let Some(url) = workspace_url(&String::from_utf8_lossy(&bytes)) else {
                        continue;
                    };
                    connected = true;
                    self.settle(run);
                    if self.current.load(Ordering::Acquire) != run {
                        continue;
                    }
                    self.open(app, url).map_err(startup)?;
                }
                Some(CommandEvent::Stderr(bytes)) => {
                    stderr.extend(bytes);
                    stderr.push(b'\n');
                    if stderr.len() > STDERR_TAIL {
                        stderr.drain(..stderr.len() - STDERR_TAIL);
                    }
                }
                Some(CommandEvent::Error(error)) => return Err(Failure::Stopped(error)),
                Some(CommandEvent::Terminated(_)) | None => {
                    if self.current.load(Ordering::Acquire) != run {
                        return Ok(());
                    }
                    let message = format!(
                        "{}\n日志：{}",
                        String::from_utf8_lossy(&stderr).trim(),
                        self.log().display()
                    );
                    return Err(if connected {
                        Failure::Stopped(message)
                    } else {
                        Failure::Startup(message)
                    });
                }
                _ => {}
            }
        }
    }

    /// Grants the native commands to the workspace's origin and navigates
    /// the main window there.
    fn open(&self, app: &AppHandle, url: Url) -> Result<()> {
        *self.workspace.lock().unwrap() = Some(url.clone());
        let origin = url.origin().ascii_serialization();
        if self.granted.lock().unwrap().insert(origin.clone()) {
            app.add_capability(
                tauri::ipc::CapabilityBuilder::new(format!(
                    "workspace-{}",
                    url.port().unwrap_or(0)
                ))
                .window(window::MAIN)
                .local(false)
                .remote(format!("{origin}/*"))
                .permission("allow-choose-workspace")
                .permission("allow-notify-attention"),
            )?;
        }
        app.get_webview_window(window::MAIN)
            .ok_or("桌面窗口已经关闭。")?
            .navigate(url)?;
        Ok(())
    }

    /// Asks whether to restart the service or quit; nothing else in Dim works
    /// without it. The question is a sheet on the main window, shown first:
    /// without a parent, macOS presents it as a system alert that may not
    /// appear, and its silent answer would quit.
    fn report(self: &Arc<Self>, app: &AppHandle, failure: Failure) {
        let (title, message) = match failure {
            Failure::Startup(message) => ("Dim 无法启动本地服务", message),
            Failure::Stopped(message) => ("Dim 的本地服务已停止", message),
        };
        let Some(main) = app.get_webview_window(window::MAIN) else {
            return app.exit(1);
        };
        window::show(app);
        let (service, handle) = (self.clone(), app.clone());
        app.dialog()
            .message(message)
            .title(title)
            .kind(MessageDialogKind::Error)
            .parent(&main)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "重启本地服务".into(),
                "退出 Dim".into(),
            ))
            .show(move |restart| {
                if restart {
                    service.restart(&handle);
                } else {
                    handle.exit(0);
                }
            });
    }

    /// Waits for the latest start to settle, so no daemon is left behind,
    /// then stops the service of this home.
    async fn stop(&self, app: &AppHandle) -> Result<()> {
        let started = self.started.load(Ordering::Acquire);
        let mut settled = self.settled.subscribe();
        let _ =
            tokio::time::timeout(STARTUP_TIMEOUT, settled.wait_for(|run| *run >= started)).await;
        let result: Result<()> = async {
            let (mut events, child) = self.command(app)?.args(["daemon", "stop"]).spawn()?;
            let stopped = tokio::time::timeout(STOP_TIMEOUT, async {
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
        if let Some(holder) = self.holder.lock().unwrap().take() {
            let _ = holder.kill();
        }
        result
    }
}

/// The workspace URL `web --no-open` prints: the service's loopback root
/// with a one-time bootstrap fragment and nothing else.
fn workspace_url(line: &str) -> Option<Url> {
    line.split_whitespace().find_map(|part| {
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
    })
}
