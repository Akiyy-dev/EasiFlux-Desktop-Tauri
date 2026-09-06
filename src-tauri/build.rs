fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["get_plugin_catalog", "set_plugin_enabled"]),
    ))
    .expect("error while building Tauri application resources")
}
