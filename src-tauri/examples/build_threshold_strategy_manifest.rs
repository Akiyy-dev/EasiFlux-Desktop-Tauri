#[path = "support/strategy_manifest.rs"]
mod strategy_manifest;

fn main() {
    strategy_manifest::main("threshold-strategy", "build_threshold_strategy_manifest");
}
