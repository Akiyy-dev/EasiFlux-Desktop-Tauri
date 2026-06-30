use crate::error::AppResult;

pub struct PluginContext;

pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn on_init(&self, _ctx: &PluginContext) -> AppResult<()> {
        Ok(())
    }
}

pub struct PluginRegistry {
    _plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            _plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self._plugins.push(plugin);
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
