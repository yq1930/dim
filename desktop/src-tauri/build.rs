fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "choose_workspace",
            "notify_attention",
            "open_workspace_path",
        ]),
    ))
    .expect("Cannot build desktop permissions");
}
