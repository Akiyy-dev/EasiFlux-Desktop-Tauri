use std::path::PathBuf;
use std::sync::Mutex;

use crate::plugin::smoke::{PluginSmokeProfile, SmokeNativeChecks};

fn has_webview_override(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy().to_ascii_uppercase();
    name.starts_with("WEBVIEW2_") || name.starts_with("COREWEBVIEW2_")
}

fn reject_policy_probe(status: u32) -> Result<(), String> {
    match status {
        2 | 3 => Ok(()), // ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND
        0 => Err("plugin smoke rejects an existing WebView2 policy root".into()),
        _ => Err(format!(
            "cannot establish WebView2 policy absence (status {status})"
        )),
    }
}

#[cfg(all(feature = "plugin-smoke", target_os = "windows"))]
fn verify_webview_environment() -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ,
        KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };
    // Presence only: values are neither interpreted nor logged.
    if std::env::vars_os().any(|(name, _)| has_webview_override(&name)) {
        return Err("plugin smoke rejects WebView2 environment overrides".into());
    }
    let policy: Vec<u16> = "Software\\Policies\\Microsoft\\Edge\\WebView2\0"
        .encode_utf16()
        .collect();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            let mut key = std::ptr::null_mut();
            // SAFETY: policy is NUL-terminated UTF-16; key is an initialized output
            // pointer. Open is read-only and no values/subkeys are enumerated.
            let status =
                unsafe { RegOpenKeyExW(root, policy.as_ptr(), 0, KEY_READ | view, &mut key) };
            if status == 0 {
                // SAFETY: success yielded an owned registry handle, closed once.
                if unsafe { RegCloseKey(key) } != 0 {
                    return Err("cannot close WebView2 policy probe".into());
                }
            }
            reject_policy_probe(status)?;
        }
    }
    Ok(())
}

struct SmokeArgs {
    parent: PathBuf,
    self_test: bool,
}
impl SmokeArgs {
    fn parse(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut parent = None;
        let mut self_test = false;
        while let Some(arg) = args.next() {
            if arg == "--parent" && parent.is_none() {
                let value = PathBuf::from(
                    args.next()
                        .ok_or("--parent requires an absolute directory")?,
                );
                if !value.is_absolute() {
                    return Err("--parent must be absolute".into());
                }
                parent = Some(value);
            } else if arg == "--self-test" && !self_test {
                self_test = true;
            } else {
                return Err("unknown or duplicate plugin smoke argument".into());
            }
        }
        Ok(Self {
            parent: parent.ok_or("--parent is required")?,
            self_test,
        })
    }
}
fn verdict(ui_success: bool, native_checks: &SmokeNativeChecks) -> (bool, i32) {
    let passed = ui_success && native_checks.passed();
    let exit_code = if passed { 0 } else { 1 };
    (passed, exit_code)
}
fn bounded_detail(detail: &str) -> String {
    let mut end = detail.len().min(2_000);
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    detail[..end].to_owned()
}
#[derive(Default)]
struct CompletionGate(Mutex<bool>);
impl CompletionGate {
    fn finish(
        &self,
        profile: &PluginSmokeProfile,
        success: bool,
        detail: &str,
    ) -> Result<i32, String> {
        use std::io::Write;
        let mut claimed = self.0.lock().map_err(|_| "completion lock poisoned")?;
        if *claimed {
            return Err("plugin smoke decision already accepted".into());
        }
        *claimed = true;
        drop(claimed);

        let detail_valid = detail.len() <= 2_000;
        let inspection = profile.inspect_final_state();
        let (passed, exit_code) = inspection
            .as_ref()
            .map(|checks| verdict(success && detail_valid, checks))
            .unwrap_or((false, 1));
        let report = serde_json::json!({
            "passed": passed, "exitCode": exit_code, "uiSuccess": success,
            "detail": bounded_detail(detail), "detailValid": detail_valid,
            "nativeChecks": inspection.as_ref().ok(),
            "nativeError": inspection.as_ref().err().map(|error| bounded_detail(error)),
            "nativePickerTested": false, "wholeAppTested": false,
        });
        let report_path = profile.root.join("report.json");
        let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&report_path)
            .map_err(|e| format!("cannot create fixed smoke report: {e}"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("cannot persist smoke report: {e}"))?;
        eprintln!(
            "plugin smoke report: {} (exit {exit_code})",
            report_path.display()
        );
        Ok(exit_code)
    }
}

#[cfg(feature = "plugin-smoke")]
fn smoke_context() -> tauri::Context<tauri::Wry> {
    // A distinct directory is essential: Tauri resolves the canonical basename
    // from the supplied path's parent, not an alternate basename beside production.
    tauri::generate_context!("plugin-smoke/tauri.conf.json")
}

#[cfg(feature = "plugin-smoke")]
struct HostState {
    profile: std::sync::Arc<PluginSmokeProfile>,
    gate: std::sync::Arc<CompletionGate>,
    self_test: bool,
}

#[cfg(feature = "plugin-smoke")]
#[tauri::command]
fn finish_plugin_smoke(
    app: tauri::AppHandle,
    state: tauri::State<'_, HostState>,
    success: bool,
    detail: String,
) -> Result<(), String> {
    if !state.self_test {
        return Err("manual mode cannot submit an automatic verdict".into());
    }
    match state.gate.finish(&state.profile, success, &detail) {
        Ok(code) => {
            app.exit(code);
            Ok(())
        }
        Err(error) if error == "plugin smoke decision already accepted" => Err(error),
        Err(error) => {
            eprintln!("plugin smoke report failure: {error}");
            app.exit(1);
            Err(error)
        }
    }
}

#[cfg(feature = "plugin-smoke")]
pub fn run_plugin_smoke() -> Result<(), String> {
    use crate::commands::plugin::{self, PluginCommandState};
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    // Never reserve a profile on an unsupported host/build.
    if !cfg!(all(debug_assertions, target_os = "windows")) {
        return Err("plugin smoke supports only debug Windows execution".into());
    }
    let args = SmokeArgs::parse(std::env::args_os().skip(1))?;
    #[cfg(target_os = "windows")]
    verify_webview_environment()?;
    let profile = Arc::new(PluginSmokeProfile::create(&args.parent)?);
    let gate = Arc::new(CompletionGate::default());
    eprintln!("plugin smoke profile: {}", profile.root.display());
    eprintln!(
        "plugin smoke fixture (select only this file in manual mode): {}",
        profile.source.display()
    );
    eprintln!(
        "automatic lane: {}; native picker and whole app unverified",
        args.self_test
    );

    if args.self_test {
        let profile = Arc::clone(&profile);
        let gate = Arc::clone(&gate);
        // Starts before WebView construction, so a stalled WebView cannot evade the deadline.
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(90));
            match gate.finish(&profile, false, "90-second self-test deadline exceeded") {
                Ok(code) => std::process::exit(code),
                Err(error) if error == "plugin smoke decision already accepted" => (),
                Err(error) => {
                    eprintln!("plugin smoke timeout report failure: {error}");
                    std::process::exit(1);
                }
            }
        });
    }

    let result: Result<(), String> = (|| {
        std::net::TcpStream::connect_timeout(
            &"127.0.0.1:1430".parse().unwrap(),
            Duration::from_secs(2),
        )
        .map_err(|e| format!("dedicated Vite server unavailable: {e}"))?;
        let command_state = if args.self_test {
            PluginCommandState::with_selector(profile.runtime(), profile.selector())
        } else {
            PluginCommandState::new(profile.runtime())
        };
        let webview_data = profile.root.join("webview2");
        std::fs::create_dir(&webview_data)
            .map_err(|e| format!("cannot reserve WebView2 directory: {e}"))?;
        let app = tauri::Builder::default()
            .plugin(tauri_plugin_dialog::init())
            .manage(command_state)
            .manage(HostState {
                profile: Arc::clone(&profile),
                gate: Arc::clone(&gate),
                self_test: args.self_test,
            })
            .invoke_handler(tauri::generate_handler![
                plugin::get_plugin_catalog,
                plugin::reload_plugin_catalog,
                plugin::set_plugin_enabled,
                plugin::prepare_local_manifest_import,
                plugin::cancel_local_manifest_import,
                plugin::commit_local_manifest_import,
                plugin::remove_managed_local_plugin,
                finish_plugin_smoke,
            ])
            .build(smoke_context())
            .map_err(|e| format!("cannot build isolated host: {e}"))?;
        // Build directly before run: a window creation error returns through the
        // report path instead of panicking inside Tauri's deferred setup callback.
        let path = if args.self_test {
            "plugin-smoke.html?self-test=1"
        } else {
            "plugin-smoke.html"
        };
        WebviewWindowBuilder::new(&app, "main", WebviewUrl::App(path.into()))
            .title("Isolated Plugin Smoke — fixture data only")
            .inner_size(1100.0, 820.0)
            .visible(!args.self_test)
            .data_directory(webview_data)
            .on_navigation(|url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port() == Some(1430)
            })
            .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
            .build()
            .map_err(|e| format!("cannot create isolated WebView: {e}"))?;
        app.run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::CloseRequested { .. },
                    ..
                }
            ) {
                let state = app.state::<HostState>();
                if state.self_test {
                    match state.gate.finish(
                        &state.profile,
                        false,
                        "automatic window closed before completion",
                    ) {
                        Ok(code) => app.exit(code),
                        Err(error) if error == "plugin smoke decision already accepted" => (),
                        Err(error) => {
                            eprintln!("plugin smoke close report failure: {error}");
                            app.exit(1);
                        }
                    }
                }
            }
        });
        Ok(())
    })();
    if let Err(error) = &result {
        if let Err(report_error) = gate.finish(&profile, false, error) {
            eprintln!("plugin smoke failure report: {report_error}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn parse(args: &[&str]) -> Result<SmokeArgs, String> {
        SmokeArgs::parse(args.iter().map(OsString::from))
    }

    // Catches environment-based WebView2 user-data overrides, including mixed case.
    #[test]
    fn webview_override_names_are_rejected_case_insensitively_without_values() {
        for name in [
            "WEBVIEW2_USER_DATA_FOLDER",
            "webview2_browser_executable_folder",
            "CoreWebView2_Additional_Browser_Arguments",
        ] {
            assert!(has_webview_override(std::ffi::OsStr::new(name)), "{name}");
        }
        for name in ["PATH", "MY_WEBVIEW2_SETTING", "WEBVIEW20", "COREWEBVIEW20"] {
            assert!(!has_webview_override(std::ffi::OsStr::new(name)), "{name}");
        }
    }

    // Catches treating a policy root or inaccessible registry as safely absent.
    #[test]
    fn registry_policy_probe_allows_only_missing_root() {
        assert!(reject_policy_probe(2).is_ok()); // ERROR_FILE_NOT_FOUND
        assert!(reject_policy_probe(3).is_ok()); // ERROR_PATH_NOT_FOUND
        assert!(reject_policy_probe(0).is_err()); // present
        assert!(reject_policy_probe(5).is_err()); // access denied
        assert!(reject_policy_probe(87).is_err()); // invalid parameter
    }
    fn all_checks() -> SmokeNativeChecks {
        SmokeNativeChecks {
            source_unchanged: true,
            disabled_decision_retained: true,
            ownership_empty: true,
            local_empty: true,
            staging_empty: true,
        }
    }
    fn profile() -> PluginSmokeProfile {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target");
        std::fs::create_dir_all(&base).unwrap();
        PluginSmokeProfile::create(&base).unwrap()
    }

    // Catches accidental implicit automation and accepting ambiguous arguments.
    #[test]
    fn arguments_require_explicit_absolute_parent_and_self_test_is_opt_in() {
        let parent = env!("CARGO_MANIFEST_DIR");
        let manual = parse(&["--parent", parent]).unwrap();
        assert_eq!(manual.parent, PathBuf::from(parent));
        assert!(!manual.self_test);
        assert!(
            parse(&["--self-test", "--parent", parent])
                .unwrap()
                .self_test
        );
        for args in [
            vec![],
            vec!["--parent"],
            vec!["--self-test"],
            vec!["--parent", "relative"],
            vec!["--parent", parent, "--unknown"],
            vec!["--parent", parent, "--self-test", "--self-test"],
            vec!["--parent", parent, "--parent", parent],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }
    // Catches OR instead of AND, or dropping any independent native check.
    #[test]
    fn verdict_requires_ui_success_and_every_native_check() {
        assert_eq!(verdict(false, &all_checks()), (false, 1));
        assert_eq!(verdict(true, &all_checks()), (true, 0));
        for index in 0..5 {
            let mut checks = all_checks();
            match index {
                0 => checks.source_unchanged = false,
                1 => checks.disabled_decision_retained = false,
                2 => checks.ownership_empty = false,
                3 => checks.local_empty = false,
                _ => checks.staging_empty = false,
            }
            assert_eq!(verdict(true, &checks), (false, 1), "check {index}");
        }
    }
    // Catches timeout not preserving evidence and a duplicate replacing first failure.
    #[test]
    fn timeout_writes_failure_once_and_retains_source() {
        let profile = profile();
        let original = std::fs::read(&profile.source).unwrap();
        let gate = CompletionGate::default();
        assert_eq!(
            gate.finish(&profile, false, "90-second self-test deadline exceeded")
                .unwrap(),
            1
        );
        let path = profile.root.join("report.json");
        let before = std::fs::read(&path).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&before).unwrap();
        assert_eq!(report["passed"], false);
        assert_eq!(report["exitCode"], 1);
        assert_eq!(report["detail"], "90-second self-test deadline exceeded");
        assert!(gate.finish(&profile, true, "late success").is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert_eq!(std::fs::read(&profile.source).unwrap(), original);
    }
    // Catches trusting frontend success or accepting an unbounded UTF-8 report.
    #[test]
    fn premature_success_and_malformed_detail_cannot_pass() {
        for detail in ["premature".to_owned(), "界".repeat(667)] {
            let profile = profile();
            assert_eq!(
                CompletionGate::default()
                    .finish(&profile, true, &detail)
                    .unwrap(),
                1
            );
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(profile.root.join("report.json")).unwrap())
                    .unwrap();
            assert_eq!(report["passed"], false);
            assert!(report["detail"].as_str().unwrap().len() <= 2_000);
        }
    }

    // Catches accidental production-config/bootstrap reuse (not source-string matching).
    #[cfg(feature = "plugin-smoke")]
    #[test]
    fn smoke_context_is_independent_and_has_no_automatic_windows() {
        let context = smoke_context();
        assert_eq!(context.config().identifier, "io.easiflux.plugin-smoke");
        assert_eq!(
            context.config().build.dev_url.as_ref().unwrap().as_str(),
            "http://127.0.0.1:1430/"
        );
        assert!(context.config().app.windows.is_empty());
    }

    // Catches implicit default/window/opener grants or remote/secondary completion authority.
    #[cfg(feature = "plugin-smoke")]
    #[test]
    fn smoke_authority_grants_only_local_main_plugin_surface() {
        use tauri::ipc::Origin;
        let mut context = smoke_context();
        let authority = context.runtime_authority_mut();
        for command in [
            "get_plugin_catalog",
            "reload_plugin_catalog",
            "set_plugin_enabled",
            "prepare_local_manifest_import",
            "cancel_local_manifest_import",
            "commit_local_manifest_import",
            "remove_managed_local_plugin",
            "finish_plugin_smoke",
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &Origin::Local)
                    .is_some(),
                "{command}"
            );
            assert!(authority
                .resolve_access(command, "main", "secondary", &Origin::Local)
                .is_none());
            assert!(authority
                .resolve_access(command, "secondary", "secondary", &Origin::Local)
                .is_none());
            assert!(authority
                .resolve_access(
                    command,
                    "main",
                    "main",
                    &Origin::Remote {
                        url: "https://example.invalid".parse().unwrap(),
                    }
                )
                .is_none());
        }
        for command in [
            "plugin:window|destroy",
            "plugin:opener|open_url",
            "plugin:dialog|open",
        ] {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &Origin::Local)
                    .is_none(),
                "{command}"
            );
        }
    }

    #[test]
    fn default_context_denies_smoke_completion() {
        let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!(test = true);
        assert!(context
            .runtime_authority_mut()
            .resolve_access(
                "finish_plugin_smoke",
                "main",
                "main",
                &tauri::ipc::Origin::Local
            )
            .is_none());
    }
}
