//! Updates: the release manifest is read shortly after launch, every six
//! hours and from the menu; a newer build is offered on the main window and,
//! once its signature checks out, replaces the bundle before Lacus restarts.

use crate::{Desktop, Result, window};
use reqwest::{
    StatusCode,
    header::{ACCEPT, RANGE},
};
use std::{
    cmp::Ordering as Precedence,
    io::Write,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tauri::{
    AppHandle, Manager, Url, Wry,
    window::{ProgressBarState, ProgressBarStatus},
};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogBuilder, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};
use tauri_plugin_updater::{Update, UpdaterExt};

const FIRST_CHECK: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const SKIP_FILE: &str = "desktop-update.json";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// A download that receives nothing for this long resumes from where it stopped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Download attempts in a row that receive nothing before the update gives up.
const ATTEMPTS: u32 = 4;
/// How much of a release's notes the offer shows.
const NOTE_LINES: usize = 12;
const INSTALL: &str = "更新并重启";
const SKIP: &str = "跳过此版本";
const LATER: &str = "稍后";
const OK: &str = "好";

/// What started a check. A scheduled check offers only an update that was not
/// skipped, only while the window is on screen, and keeps failures to the log;
/// the menu always answers.
#[derive(Clone, Copy, PartialEq)]
pub enum Trigger {
    Schedule,
    Menu,
}

/// What `build-info.json` records about the running bundle.
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildInfo {
    pub version: Option<String>,
    pub source_commit: Option<String>,
    pub built_at: Option<String>,
}

impl BuildInfo {
    pub fn read(app: &AppHandle) -> Self {
        app.path()
            .resource_dir()
            .ok()
            .and_then(|directory| std::fs::read(directory.join("build-info.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// The build time in the form a release version carries after `+`.
    fn stamp(&self) -> Option<String> {
        let digits: String = self
            .built_at
            .as_deref()?
            .chars()
            .take("2026-01-01T00:00:00".len())
            .filter(char::is_ascii_digit)
            .collect();
        (digits.len() == 14).then_some(digits)
    }

    fn describe(&self, app: &AppHandle) -> String {
        match (self.stamp(), self.source_commit.as_deref()) {
            (Some(stamp), Some(commit)) => built(&stamp, commit),
            _ => app.package_info().version.to_string(),
        }
    }
}

/// Starts the scheduled checks: in release builds, or in any build pointed at
/// another manifest by `LACUS_UPDATE_ENDPOINT`; `LACUS_UPDATE_CHECK=0` turns
/// them off.
pub fn watch(app: &AppHandle) {
    let enabled = std::env::var_os("LACUS_UPDATE_CHECK").is_none_or(|value| value != "0")
        && (!cfg!(debug_assertions) || std::env::var_os("LACUS_UPDATE_ENDPOINT").is_some());
    if !enabled {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(&app, Trigger::Schedule).await;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// Looks for a newer build and offers it. Nothing else starts while a check,
/// an open offer or an install is under way.
pub async fn check(app: &AppHandle, trigger: Trigger) {
    let desktop = app.state::<Arc<Desktop>>().inner().clone();
    if desktop.updating.swap(true, Ordering::AcqRel) {
        return;
    }
    let build = BuildInfo::read(app);
    match find(app, &build).await {
        Ok(Some(update)) if trigger == Trigger::Menu || offerable(app, &desktop, &update) => {
            return offer(app, desktop, &build, update, trigger);
        }
        Ok(None) if trigger == Trigger::Menu => tell(
            app,
            MessageDialogKind::Info,
            "Lacus 已是最新版本",
            format!("当前：{}", build.describe(app)),
        ),
        Err(error) if trigger == Trigger::Menu => tell(
            app,
            MessageDialogKind::Error,
            "检查更新失败",
            error.to_string(),
        ),
        Err(error) => eprintln!("Lacus update check: {error}"),
        Ok(_) => {}
    }
    desktop.updating.store(false, Ordering::Release);
}

/// The release the manifest names when it is newer than this build: a higher
/// version, or the same version built later.
async fn find(app: &AppHandle, build: &BuildInfo) -> Result<Option<Update>> {
    let built = build.stamp();
    let mut updater =
        app.updater_builder()
            .version_comparator(move |current, release| {
                match release.version.cmp_precedence(&current) {
                    Precedence::Equal => built
                        .as_deref()
                        .zip(stamp_of(&release.version.build))
                        .is_some_and(|(built, stamp)| stamp > built),
                    precedence => precedence.is_gt(),
                }
            });
    if let Some(endpoint) = std::env::var_os("LACUS_UPDATE_ENDPOINT") {
        updater = updater.endpoints(vec![Url::parse(&endpoint.to_string_lossy())?])?;
    }
    Ok(updater.build()?.check().await?)
}

/// The build time a release version carries after `+`, before its commit.
fn stamp_of(build: &str) -> Option<&str> {
    let stamp = build.split('.').next()?;
    (stamp.len() == 14 && stamp.bytes().all(|byte| byte.is_ascii_digit())).then_some(stamp)
}

fn built(stamp: &str, commit: &str) -> String {
    let commit: String = commit.chars().take(7).collect();
    format!(
        "构建于 {}-{}-{}，提交 {commit}",
        &stamp[..4],
        &stamp[4..6],
        &stamp[6..8]
    )
}

fn describe_release(version: &str) -> String {
    let parts = version
        .split_once('+')
        .and_then(|(_, build)| Some((stamp_of(build)?, build.split('.').nth(1)?)));
    match parts {
        Some((stamp, commit)) => built(stamp, commit),
        None => version.to_owned(),
    }
}

fn offerable(app: &AppHandle, desktop: &Desktop, update: &Update) -> bool {
    let on_screen = app.get_webview_window(window::MAIN).is_some_and(|main| {
        main.is_visible().unwrap_or(false) && !main.is_minimized().unwrap_or(true)
    });
    on_screen && skipped(desktop.service.home()).as_deref() != Some(update.version.as_str())
}

/// A scheduled offer waits on the window without taking the focus, so a key
/// pressed in another app cannot answer it; one the menu asked for comes to the
/// front.
fn offer(
    app: &AppHandle,
    desktop: Arc<Desktop>,
    build: &BuildInfo,
    update: Update,
    trigger: Trigger,
) {
    let notes = update
        .body
        .as_deref()
        .map(str::trim)
        .filter(|notes| !notes.is_empty())
        .map(|notes| {
            let lines: Vec<&str> = notes.lines().take(NOTE_LINES).collect();
            format!("\n\n{}", lines.join("\n"))
        })
        .unwrap_or_default();
    let message = format!(
        "当前：{}\n最新：{}{notes}\n\n更新会先下载并校验新版本，完成后 Lacus 自动重启，正在运行的任务会中断。",
        build.describe(app),
        describe_release(&update.version),
    );
    if trigger == Trigger::Menu {
        window::show(app);
    }
    let Some(dialog) = sheet(app, MessageDialogKind::Info, "Lacus 有新版本", message) else {
        return desktop.updating.store(false, Ordering::Release);
    };
    let handle = app.clone();
    // The first button is the default one, which Return presses: installing
    // restarts Lacus and stops its running tasks, so it takes a click.
    dialog
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            LATER.into(),
            INSTALL.into(),
            SKIP.into(),
        ))
        .show_with_result(move |answer| match answer {
            MessageDialogResult::Custom(label) if label == INSTALL => {
                tauri::async_runtime::spawn(install(handle, desktop, update));
            }
            MessageDialogResult::Custom(label) if label == SKIP => {
                if let Err(error) = skip(desktop.service.home(), &update.version) {
                    eprintln!("Lacus update skip: {error}");
                }
                desktop.updating.store(false, Ordering::Release);
            }
            _ => desktop.updating.store(false, Ordering::Release),
        });
}

/// Downloads the release and checks its signature, stops the service with the
/// running bundle's own CLI, swaps the bundle and restarts. A download that
/// gives up or a signature failure leaves Lacus running as it was; a failed
/// swap restores the previous bundle, which the restart then opens.
async fn install(app: AppHandle, desktop: Arc<Desktop>, update: Update) {
    let main = app.get_webview_window(window::MAIN);
    let progress = |progress: Option<u64>| {
        if let Some(main) = &main {
            let status = progress.map_or(ProgressBarStatus::None, |_| ProgressBarStatus::Normal);
            let _ = main.set_progress_bar(ProgressBarState {
                status: Some(status),
                progress,
            });
        }
    };
    let mut shown = None;
    let downloaded = download(&update, |received, total| {
        let percent = total.map(|total| (received * 100 / total.max(1)).min(100));
        if percent != shown {
            shown = percent;
            progress(Some(percent.unwrap_or(0)));
        }
    })
    .await
    .and_then(|bytes| Ok(update.verify(&bytes).map(|()| bytes)?));
    progress(None);
    let bytes = match downloaded {
        Ok(bytes) => bytes,
        Err(error) => {
            desktop.updating.store(false, Ordering::Release);
            tell(
                &app,
                MessageDialogKind::Error,
                "Lacus 更新失败",
                format!(
                    "无法下载或校验新版本：{error}\n\nLacus 保持当前版本，可以稍后从“Lacus > 检查更新…”重试。"
                ),
            );
            return;
        }
    };
    if let Some(main) = &main
        && let Ok(boot) = Url::parse(window::BOOT_URL)
    {
        let _ = main.navigate(boot);
    }
    if let Err(error) = desktop.service.quit(&app).await {
        eprintln!("Lacus update: {error}");
    }
    match update.install(bytes) {
        Ok(()) => app.request_restart(),
        Err(error) => {
            let message = format!("无法替换应用：{error}\n\nLacus 将以当前版本重新启动。");
            window::show(&app);
            match sheet(&app, MessageDialogKind::Error, "Lacus 更新失败", message) {
                Some(dialog) => {
                    let handle = app.clone();
                    dialog
                        .buttons(MessageDialogButtons::OkCustom(OK.into()))
                        .show(move |_| handle.request_restart());
                }
                None => app.request_restart(),
            }
        }
    }
}

/// Downloads the release package. A connection that drops or stays idle for
/// `IDLE_TIMEOUT` resumes from the bytes already received; the download gives
/// up after `ATTEMPTS` attempts in a row that receive nothing.
async fn download(update: &Update, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
    // Already installed when the updater has made a request.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent("Lacus")
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(IDLE_TIMEOUT)
        .build()?;
    let (mut bytes, mut total, mut stalled) = (Vec::new(), None, 0);
    loop {
        let received = bytes.len();
        let attempt = resume(&client, update, &mut bytes, &mut total, &mut progress).await;
        if attempt.is_ok() && total.is_none_or(|total| bytes.len() as u64 >= total) {
            return Ok(bytes);
        }
        stalled = if bytes.len() > received {
            0
        } else {
            stalled + 1
        };
        if stalled == ATTEMPTS {
            return Err(attempt
                .err()
                .map_or_else(|| "下载多次中断".into(), Into::into));
        }
        tokio::time::sleep(Duration::from_secs(1 << stalled)).await;
    }
}

/// One request for the bytes not received yet; a server that ignores `Range`
/// sends the whole package again.
async fn resume(
    client: &reqwest::Client,
    update: &Update,
    bytes: &mut Vec<u8>,
    total: &mut Option<u64>,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> reqwest::Result<()> {
    let mut request = client
        .get(update.download_url.clone())
        .header(ACCEPT, "application/octet-stream");
    if !bytes.is_empty() {
        request = request.header(RANGE, format!("bytes={}-", bytes.len()));
    }
    let mut response = request.send().await?.error_for_status()?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        bytes.clear();
        *total = response.content_length();
    }
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        progress(bytes.len() as u64, *total);
    }
    Ok(())
}

/// A dialog as a sheet on the main window: without a parent, macOS may not
/// present it and its silent answer would be taken.
fn sheet(
    app: &AppHandle,
    kind: MessageDialogKind,
    title: &str,
    message: String,
) -> Option<MessageDialogBuilder<Wry>> {
    let main = app.get_webview_window(window::MAIN)?;
    Some(
        app.dialog()
            .message(message)
            .title(title)
            .kind(kind)
            .parent(&main),
    )
}

/// Answers something the user asked for, in front.
fn tell(app: &AppHandle, kind: MessageDialogKind, title: &str, message: String) {
    window::show(app);
    if let Some(dialog) = sheet(app, kind, title, message) {
        dialog
            .buttons(MessageDialogButtons::OkCustom(OK.into()))
            .show(|_| {});
    }
}

/// The version the user chose to skip, kept in the Desktop home.
fn skipped(home: &Path) -> Option<String> {
    let bytes = std::fs::read(home.join(SKIP_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["skippedVersion"].as_str().map(str::to_owned)
}

fn skip(home: &Path, version: &str) -> std::io::Result<()> {
    let staged = home.join(format!("{SKIP_FILE}.tmp"));
    let mut file = std::fs::File::create(&staged)?;
    file.write_all(&serde_json::to_vec_pretty(
        &serde_json::json!({ "skippedVersion": version }),
    )?)?;
    file.sync_all()?;
    std::fs::rename(staged, home.join(SKIP_FILE))
}
