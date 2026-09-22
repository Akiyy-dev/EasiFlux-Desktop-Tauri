mod contract;
pub(crate) use contract::*;
mod types;
pub(crate) use types::*;
mod execution;
mod recovery;
mod store;
mod supervisor;
pub(crate) use store::production_strategy_store;
#[cfg(feature = "plugin-smoke")]
pub(crate) use store::FileStrategyStore;
pub(crate) use supervisor::StrategySupervisor;

pub(crate) fn error(code: &'static str) -> crate::error::AppError {
    crate::error::AppError::Plugin {
        code,
        message: "自动策略未完成，请检查授权、账户与恢复状态",
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests;
