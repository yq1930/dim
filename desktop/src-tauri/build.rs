fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "choose_workspace",
            "installed_apps",
            "keep_awake",
            "notify_attention",
            "open_in_app",
            "open_workspace_path",
            "show_preview",
        ]),
    ))
    .expect("Cannot build desktop permissions");
}
