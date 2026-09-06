pub mod builtin;
pub mod manifest;

#[derive(Default)]
pub struct PluginRegistry;

impl PluginRegistry {
    pub fn new() -> Self {
        Self
    }
}
