fn main() {
    if let Err(error) = easiflux_desktop_lib::run_plugin_smoke() {
        eprintln!("plugin smoke failed: {error}");
        std::process::exit(1);
    }
}
