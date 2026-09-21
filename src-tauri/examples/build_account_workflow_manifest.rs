use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

const MODULES: [(&str, &str); 3] = [
    ("account.available-balance", "balance.wat"),
    ("account.place-order", "place.wat"),
    ("account.cancel-first-open-order", "cancel.wat"),
];

fn example_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .join("examples/plugins/account-workflow")
}

fn generated_manifest() -> Result<(PathBuf, Vec<u8>), String> {
    let directory = example_directory();
    let manifest_path = directory.join("manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let mut manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("cannot parse {}: {error}", manifest_path.display()))?;
    let contributions = manifest
        .get_mut("contributions")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "manifest contributions are missing".to_owned())?;

    for (contribution_id, source_name) in MODULES {
        let wat_path = directory.join(source_name);
        let source = std::fs::read_to_string(&wat_path)
            .map_err(|error| format!("cannot read {}: {error}", wat_path.display()))?;
        let module = wat::parse_str(&source)
            .map_err(|error| format!("cannot compile {}: {error}", wat_path.display()))?;
        if module.len() > 8192 {
            return Err(format!("{} exceeds 8192 decoded bytes", wat_path.display()));
        }
        let contribution = contributions
            .iter_mut()
            .find(|item| item.get("contributionId").and_then(Value::as_str) == Some(contribution_id))
            .ok_or_else(|| format!("manifest contribution {contribution_id} is missing"))?;
        let slot = contribution
            .pointer_mut("/params/moduleBase64")
            .ok_or_else(|| format!("manifest module slot for {contribution_id} is missing"))?;
        *slot = Value::String(STANDARD.encode(module));
    }

    let generated = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    if generated.len() > 16 * 1024 {
        return Err("generated manifest exceeds 16 KiB".into());
    }
    Ok((manifest_path, [generated, vec![b'\n']].concat()))
}

fn run() -> Result<(), String> {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("--check") => "check",
        Some("--write") => "write",
        _ => return Err("usage: build_account_workflow_manifest (--check | --write)".into()),
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
            "{} is stale; run examples/plugins/account-workflow/build.mjs --write",
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
