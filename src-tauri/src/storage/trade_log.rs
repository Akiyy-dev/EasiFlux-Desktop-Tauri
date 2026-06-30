use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use crate::error::AppResult;
use crate::models::config::APP_NAME;
use crate::models::trading::Order;

pub struct TradeLogStore {
    path: PathBuf,
}

impl TradeLogStore {
    pub fn new() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(APP_NAME);
        let _ = fs::create_dir_all(&dir);
        Self {
            path: dir.join("orders.csv"),
        }
    }

    pub fn append_order(&self, order: &Order) -> AppResult<()> {
        let exists = self.path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        if !exists {
            writeln!(file, "order_id,symbol,side,type,price,qty,status,filled_qty,avg_price")?;
        }
        writeln!(
            file,
            "{},{},{},{},{},{},{:?},{},{}",
            order.order_id,
            order.symbol,
            order.side,
            order.order_type,
            order.price,
            order.qty,
            order.status,
            order.filled_qty,
            order.avg_price
        )?;
        Ok(())
    }

    pub fn export_path(&self) -> PathBuf {
        self.path.clone()
    }
}

impl Default for TradeLogStore {
    fn default() -> Self {
        Self::new()
    }
}
