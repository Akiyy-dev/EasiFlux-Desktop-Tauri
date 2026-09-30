#[path = "support/strategy_manifest.rs"]
mod strategy_manifest;

fn main() {
    strategy_manifest::main("managed-entry", "build_managed_entry_manifest");
}
