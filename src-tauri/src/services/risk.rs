use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::config::RiskConfig;
use crate::models::risk::{RiskViolation, RiskViolationCode};
use crate::models::trading::PlaceOrderRequest;
use crate::services::time::{resolve_trading_day_timezone, trading_day_key};
use crate::storage::{RiskUsage, RiskUsageStore};

const LEDGER_UNAVAILABLE_MESSAGE: &str = "风控用量账本不可用，请检查本地存储权限或文件格式。";

mod status;
mod validation;
pub use validation::validate_risk_config;

#[derive(Debug, Clone)]
pub struct RiskReservation {
    trading_day: String,
    timezone: String,
    counted: bool,
}

impl RiskReservation {
    fn not_counted() -> Self {
        Self {
            trading_day: String::new(),
            timezone: String::new(),
            counted: false,
        }
    }

    fn counted(trading_day: String, timezone: String) -> Self {
        Self {
            trading_day,
            timezone,
            counted: true,
        }
    }
}

struct RiskUsageState {
    usage: Option<RiskUsage>,
    load_error: Option<String>,
}

pub struct RiskService {
    config: RiskConfig,
    store: RiskUsageStore,
    usage: Mutex<RiskUsageState>,
}

impl RiskService {
    pub fn new(config: RiskConfig) -> Self {
        Self::with_store(config, RiskUsageStore::new())
    }

    pub(crate) fn with_store(config: RiskConfig, store: RiskUsageStore) -> Self {
        let (usage, load_error) = if config.enabled {
            Self::load_usage(&store)
        } else {
            (None, None)
        };
        Self {
            config,
            store,
            usage: Mutex::new(RiskUsageState { usage, load_error }),
        }
    }

    pub fn update_config(&mut self, config: RiskConfig) {
        let was_enabled = self.config.enabled;
        self.config = config;
        if !self.config.enabled {
            if let Ok(mut state) = self.usage.lock() {
                state.usage = None;
                state.load_error = None;
            }
        } else if !was_enabled {
            let (usage, load_error) = Self::load_usage(&self.store);
            if let Ok(mut state) = self.usage.lock() {
                state.usage = usage;
                state.load_error = load_error;
            }
        }
    }

    fn load_usage(store: &RiskUsageStore) -> (Option<RiskUsage>, Option<String>) {
        match store.load() {
            Ok(usage) => (usage, None),
            Err(error) => (None, Some(error.user_message())),
        }
    }

    pub(crate) fn requires_reference_price(&self, request: &PlaceOrderRequest) -> bool {
        self.config.enabled && request.order_type.eq_ignore_ascii_case("limit")
    }

    pub fn reserve_order(
        &self,
        request: &PlaceOrderRequest,
        reference_price: Option<&str>,
        now_ms: u64,
    ) -> Result<RiskReservation, RiskViolation> {
        if !self.config.enabled {
            return Ok(RiskReservation::not_counted());
        }

        validation::validate_order(&self.config, request, reference_price)?;

        if request.reduce_only == Some(true) {
            return Ok(RiskReservation::not_counted());
        }

        let timezone = resolve_trading_day_timezone(&self.config.trading_day_timezone);
        let trading_day = trading_day_key(now_ms, &timezone);
        let mut state = self
            .usage
            .lock()
            .map_err(|_| RiskViolation::new(RiskViolationCode::LedgerUnavailable))?;
        if state.load_error.is_some() {
            return Err(RiskViolation::new(RiskViolationCode::LedgerUnavailable));
        }

        let mut next = match state.usage.as_ref() {
            Some(usage) if usage.trading_day == trading_day && usage.timezone == timezone => {
                usage.clone()
            }
            _ => RiskUsage {
                schema_version: 1,
                trading_day: trading_day.clone(),
                timezone: timezone.clone(),
                occupied_orders: 0,
                updated_at_ms: now_ms,
            },
        };

        if next.occupied_orders >= self.config.max_daily_orders {
            return Err(RiskViolation::with_limit(
                RiskViolationCode::DailyOrderLimit,
                f64::from(self.config.max_daily_orders),
            ));
        }

        next.occupied_orders += 1;
        next.updated_at_ms = now_ms;
        self.store
            .save(&next)
            .map_err(|_| RiskViolation::new(RiskViolationCode::LedgerUnavailable))?;
        state.usage = Some(next);

        Ok(RiskReservation::counted(trading_day, timezone))
    }

    pub fn release_reservation(&self, reservation: &RiskReservation, now_ms: u64) -> AppResult<()> {
        if !reservation.counted {
            return Ok(());
        }

        let mut state = self
            .usage
            .lock()
            .map_err(|_| AppError::Storage("风控用量锁已损坏".into()))?;
        let Some(current) = state.usage.as_ref() else {
            return Ok(());
        };
        if current.trading_day != reservation.trading_day
            || current.timezone != reservation.timezone
        {
            return Ok(());
        }

        let mut next = current.clone();
        next.occupied_orders = next.occupied_orders.saturating_sub(1);
        next.updated_at_ms = now_ms;
        self.store.save(&next)?;
        state.usage = Some(next);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
