fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_plugin_catalog",
            "reload_plugin_catalog",
            "set_plugin_enabled",
            "prepare_local_manifest_import",
            "cancel_local_manifest_import",
            "commit_local_manifest_import",
            "remove_managed_local_plugin",
        ]),
    ))
    .expect("error while building Tauri application resources")
}
