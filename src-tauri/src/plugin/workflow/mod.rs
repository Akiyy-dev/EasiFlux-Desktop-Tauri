//! Session-only native authority for import-free account workflows.
mod contract;
pub(crate) use contract::*;
mod host;
pub(crate) use host::*;
mod production;
pub(crate) use production::ProductionWorkflowHost;
#[cfg(test)]
mod tests;

pub(crate) fn error(code: &'static str) -> crate::error::AppError {
    crate::error::AppError::Plugin {
        code,
        message: "插件账户工作流未完成，请重新检查授权和账户状态",
        diagnostic: None,
    }
}
