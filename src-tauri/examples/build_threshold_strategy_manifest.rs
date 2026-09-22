use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

fn example_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .join("examples/plugins/threshold-strategy")
}

fn generated_manifest() -> Result<(PathBuf, Vec<u8>, usize), String> {
    let directory = example_directory();
    let wat_path = directory.join("strategy.wat");
    let manifest_path = directory.join("manifest.json");
    let source = std::fs::read_to_string(&wat_path)
        .map_err(|error| format!("cannot read {}: {error}", wat_path.display()))?;
    let module = wat::parse_str(&source)
        .map_err(|error| format!("cannot compile {}: {error}", wat_path.display()))?;
    if module.len() > 8192 {
        return Err(format!(
            "{} exceeds 8192 decoded bytes",
            wat_path.display()
        ));
    }

    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("cannot parse {}: {error}", manifest_path.display()))?;
    let slot = manifest
        .pointer("/contributions/0/params/moduleBase64")
        .and_then(Value::as_str)
        .ok_or_else(|| "manifest strategy moduleBase64 slot is missing".to_owned())?;
    let encoded = STANDARD.encode(&module);
    let quoted_slot = serde_json::to_string(slot).map_err(|error| error.to_string())?;
    let quoted_encoded = serde_json::to_string(&encoded).map_err(|error| error.to_string())?;
    let document =
        String::from_utf8(manifest_bytes).map_err(|_| "manifest must be UTF-8".to_owned())?;
    if document.matches(&quoted_slot).count() != 1 {
        return Err("manifest moduleBase64 value must occur exactly once".into());
    }
    let generated = document.replacen(&quoted_slot, &quoted_encoded, 1).into_bytes();
    if generated.len() > 16 * 1024 {
        return Err("generated manifest exceeds 16 KiB".into());
    }
    Ok((manifest_path, generated, module.len()))
}

fn run() -> Result<(), String> {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("--check") => "check",
        Some("--write") => "write",
        _ => return Err("usage: build_threshold_strategy_manifest (--check | --write)".into()),
    };
    if std::env::args().nth(2).is_some() {
        return Err("unexpected extra argument".into());
    }
    let (path, generated, module_bytes) = generated_manifest()?;
    if mode == "write" {
        std::fs::write(&path, &generated)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!(
            "updated {} (module {} bytes, manifest {} bytes)",
            path.display(),
            module_bytes,
            generated.len()
        );
        return Ok(());
    }
    let checked_in =
        std::fs::read(&path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if checked_in != generated {
        return Err(format!(
            "{} is stale; run examples/plugins/threshold-strategy/build.mjs --write",
            path.display()
        ));
    }
    println!(
        "verified {} (module {} bytes, manifest {} bytes)",
        path.display(),
        module_bytes,
        generated.len()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
