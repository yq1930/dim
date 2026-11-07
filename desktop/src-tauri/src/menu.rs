//! The application menu: the standard macOS items under Chinese titles, and
//! the commands Desktop adds for the page and the local service.

use crate::{
    Desktop,
    update::{self, BuildInfo, Trigger},
    window::{self, Zoom},
};
use std::sync::Arc;
use tauri::{
    AppHandle, Manager, Wry,
    menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};
use tauri_plugin_opener::OpenerExt;

const RELOAD: &str = "reload";
const ZOOM_IN: &str = "zoom-in";
const ZOOM_OUT: &str = "zoom-out";
const ZOOM_RESET: &str = "zoom-reset";
const DEVTOOLS: &str = "devtools";
const OPEN_DATA: &str = "open-data";
const SHOW_LOG: &str = "show-log";
const RESTART_SERVICE: &str = "restart-service";
const CHECK_UPDATES: &str = "check-updates";

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let separator = || PredefinedMenuItem::separator(app);
    let item = |id: &str, text: &str, accelerator: Option<&str>| {
        MenuItem::with_id(app, id, text, true, accelerator)
    };
    let product = Submenu::with_items(
        app,
        "Lacus",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("关于 Lacus"), Some(about(app)))?,
            &item(CHECK_UPDATES, "检查更新…", None)?,
            &separator()?,
            &PredefinedMenuItem::services(app, Some("服务"))?,
            &separator()?,
            &PredefinedMenuItem::hide(app, Some("隐藏 Lacus"))?,
            &PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?,
            &PredefinedMenuItem::show_all(app, Some("全部显示"))?,
            &separator()?,
            &PredefinedMenuItem::quit(app, Some("退出 Lacus"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "编辑",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("撤销"))?,
            &PredefinedMenuItem::redo(app, Some("重做"))?,
            &separator()?,
            &PredefinedMenuItem::cut(app, Some("剪切"))?,
            &PredefinedMenuItem::copy(app, Some("拷贝"))?,
            &PredefinedMenuItem::paste(app, Some("粘贴"))?,
            &PredefinedMenuItem::select_all(app, Some("全选"))?,
        ],
    )?;
    let view = Submenu::with_items(
        app,
        "显示",
        true,
        &[
            &item(RELOAD, "重新加载", Some("CmdOrCtrl+R"))?,
            &separator()?,
            &item(ZOOM_RESET, "实际大小", Some("CmdOrCtrl+0"))?,
            &item(ZOOM_IN, "放大", Some("CmdOrCtrl+="))?,
            &item(ZOOM_OUT, "缩小", Some("CmdOrCtrl+-"))?,
            &separator()?,
            &PredefinedMenuItem::fullscreen(app, Some("进入全屏幕"))?,
            &separator()?,
            &item(DEVTOOLS, "开发者工具", Some("Alt+CmdOrCtrl+I"))?,
        ],
    )?;
    let windows = Submenu::with_items(
        app,
        "窗口",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("最小化"))?,
            &PredefinedMenuItem::maximize(app, Some("缩放"))?,
            &separator()?,
            &PredefinedMenuItem::close_window(app, Some("关闭窗口"))?,
            &separator()?,
            &PredefinedMenuItem::bring_all_to_front(app, Some("前置全部窗口"))?,
        ],
    )?;
    let help = Submenu::with_items(
        app,
        "帮助",
        true,
        &[
            &item(OPEN_DATA, "打开数据目录", None)?,
            &item(SHOW_LOG, "显示本地服务日志", None)?,
            &separator()?,
            &item(RESTART_SERVICE, "重启本地服务", None)?,
        ],
    )?;
    Menu::with_items(app, &[&product, &edit, &view, &windows, &help])
}

/// The About panel's version, and the commit and date the bundle was built
/// from when `build-info.json` records them.
fn about(app: &AppHandle) -> AboutMetadata<'static> {
    let info = BuildInfo::read(app);
    let text = |value: &Option<String>, length: usize| {
        value
            .as_deref()
            .map(|value| value.chars().take(length).collect::<String>())
    };
    let credits = match (text(&info.source_commit, 9), text(&info.built_at, 10)) {
        (Some(commit), Some(built)) => Some(format!("提交 {commit} · 构建于 {built}")),
        (Some(commit), None) => Some(format!("提交 {commit}")),
        _ => None,
    };
    AboutMetadata {
        name: Some("Lacus".into()),
        version: Some(
            info.version
                .unwrap_or_else(|| app.package_info().version.to_string()),
        ),
        credits,
        ..Default::default()
    }
}

pub fn handle(app: &AppHandle, event: MenuEvent) {
    let main = || app.get_webview_window(window::MAIN);
    match event.id().as_ref() {
        RELOAD => {
            if let Some(window) = main() {
                let _ = window.reload();
            }
        }
        ZOOM_IN => window::zoom(app, Zoom::In),
        ZOOM_OUT => window::zoom(app, Zoom::Out),
        ZOOM_RESET => window::zoom(app, Zoom::Reset),
        DEVTOOLS => {
            if let Some(window) = main() {
                if window.is_devtools_open() {
                    window.close_devtools();
                } else {
                    window.open_devtools();
                }
            }
        }
        OPEN_DATA => {
            let home = app.state::<Arc<Desktop>>().service.home().to_owned();
            let _ = app.opener().open_path(home.to_string_lossy(), None::<&str>);
        }
        SHOW_LOG => {
            let service = app.state::<Arc<Desktop>>().service.clone();
            let log = service.log();
            let _ = if log.is_file() {
                app.opener().reveal_item_in_dir(log)
            } else {
                app.opener()
                    .open_path(service.home().to_string_lossy(), None::<&str>)
            };
        }
        RESTART_SERVICE => app.state::<Arc<Desktop>>().service.clone().restart(app),
        CHECK_UPDATES => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { update::check(&app, Trigger::Menu).await });
        }
        _ => {}
    }
}
