pub mod builtin;
pub(crate) mod discovery;
pub(crate) mod import;
pub mod manifest;
pub(crate) mod ownership;
pub mod record;
pub mod registry;
pub(crate) mod removal;
mod runtime;

pub use registry::PluginRegistry;
pub(crate) use runtime::PluginRuntime;
