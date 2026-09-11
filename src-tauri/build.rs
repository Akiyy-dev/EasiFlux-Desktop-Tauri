fn main() {
    let smoke = std::env::var_os("CARGO_FEATURE_PLUGIN_SMOKE").is_some();
    if smoke {
        // Both Tauri build and context macros otherwise merge this into the config.
        assert!(
            std::env::var_os("TAURI_CONFIG").is_none(),
            "plugin-smoke rejects TAURI_CONFIG overrides; use the dedicated Cargo-only commands"
        );
        println!("cargo:rerun-if-changed=plugin-smoke/tauri.conf.json");
    }
    let mut commands = vec![
        "get_plugin_catalog",
        "reload_plugin_catalog",
        "set_plugin_enabled",
        "prepare_local_manifest_import",
        "cancel_local_manifest_import",
        "commit_local_manifest_import",
        "remove_managed_local_plugin",
    ];
    if smoke {
        commands.push("finish_plugin_smoke");
    }
    // AppManifest requires a static slice; this build-script process ends immediately afterward.
    let commands = Box::leak(commands.into_boxed_slice());
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("error while building Tauri application resources")
}
