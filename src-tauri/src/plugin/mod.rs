pub mod builtin;
pub(crate) mod discovery;
pub(crate) mod import;
pub mod manifest;
pub(crate) mod ownership;
pub mod record;
pub mod registry;
pub(crate) mod removal;
mod runtime;
#[cfg(any(test, feature = "plugin-smoke"))]
pub(crate) mod smoke;

pub use registry::PluginRegistry;
pub(crate) use runtime::PluginRuntime;
