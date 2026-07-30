#[path = "build_support/news_build_config.rs"]
mod news_build_config;

use std::env;

use news_build_config::validate_news_build_config;

fn main() {
    println!("cargo:rerun-if-env-changed=EASIFLUX_NEWS_API_BASE_URL");
    println!("cargo:rerun-if-env-changed=EASIFLUX_NEWS_SOURCE_EPOCH");
    println!("cargo:rerun-if-env-changed=EASIFLUX_NEWS_API_TOKEN");

    let profile = env::var("PROFILE").unwrap_or_default();
    let base_url = env::var("EASIFLUX_NEWS_API_BASE_URL").ok();
    let source_epoch = env::var("EASIFLUX_NEWS_SOURCE_EPOCH").ok();
    let api_token = env::var("EASIFLUX_NEWS_API_TOKEN").ok();

    match validate_news_build_config(
        &profile,
        base_url.as_deref(),
        source_epoch.as_deref(),
        api_token.as_deref(),
    ) {
        Ok(_) => {}
        Err(error) if profile == "release" => {
            panic!("invalid release news build configuration: {error}");
        }
        Err(error) => panic!("invalid news build configuration: {error}"),
    }

    tauri_build::build()
}
