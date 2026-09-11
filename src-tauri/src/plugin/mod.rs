pub mod builtin;
pub(crate) mod discovery;
pub mod manifest;
pub mod record;
pub mod registry;
mod runtime;

pub use registry::PluginRegistry;
pub(crate) use runtime::PluginRuntime;
