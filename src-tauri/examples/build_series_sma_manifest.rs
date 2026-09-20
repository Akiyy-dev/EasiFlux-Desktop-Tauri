use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

fn example_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .join("examples/plugins/series-sma")
}

fn generated_manifest() -> Result<(PathBuf, Vec<u8>), String> {
    let directory = example_directory();
    let wat_path = directory.join("plugin.wat");
    let manifest_path = directory.join("manifest.json");
    let source = std::fs::read_to_string(&wat_path)
        .map_err(|error| format!("cannot read {}: {error}", wat_path.display()))?;
    let module = wat::parse_str(&source)
        .map_err(|error| format!("cannot compile {}: {error}", wat_path.display()))?;
    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("cannot parse {}: {error}", manifest_path.display()))?;
    let slot = manifest
        .pointer("/contributions/0/params/moduleBase64")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest compute moduleBase64 slot is missing".to_owned())?;
    let encoded = STANDARD.encode(module);
    let quoted_slot = serde_json::to_string(slot).map_err(|error| error.to_string())?;
    let quoted_encoded = serde_json::to_string(&encoded).map_err(|error| error.to_string())?;
    let document =
        String::from_utf8(manifest_bytes).map_err(|_| "manifest must be UTF-8".to_owned())?;
    if document.matches(&quoted_slot).count() != 1 {
        return Err("manifest moduleBase64 value must occur exactly once".into());
    }
    let generated = document.replacen(&quoted_slot, &quoted_encoded, 1);
    Ok((manifest_path, generated.into_bytes()))
}

fn run() -> Result<(), String> {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("--check") => "check",
        Some("--write") => "write",
        _ => return Err("usage: build_series_sma_manifest (--check | --write)".into()),
    };
    if std::env::args().nth(2).is_some() {
        return Err("unexpected extra argument".into());
    }
    let (path, generated) = generated_manifest()?;
    if mode == "write" {
        std::fs::write(&path, generated)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("updated {}", path.display());
        return Ok(());
    }
    let checked_in =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if checked_in != generated {
        return Err(format!(
            "{} is stale; run examples/plugins/series-sma/build.mjs --write",
            path.display()
        ));
    }
    println!("verified {}", path.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
