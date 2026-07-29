use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::models::chart_workspace::ChartWorkspaceKey;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum ChartDiagnosticKey {
    Kline(ChartWorkspaceKey),
    View(ChartWorkspaceKey),
    Preferences,
}

pub(super) struct ChartWorkspaceDiagnostics {
    reported: Mutex<HashSet<ChartDiagnosticKey>>,
    reporter: Arc<dyn Fn(String) + Send + Sync>,
}

impl ChartWorkspaceDiagnostics {
    pub fn new(reporter: Arc<dyn Fn(String) + Send + Sync>) -> Self {
        Self {
            reported: Mutex::new(HashSet::new()),
            reporter,
        }
    }

    pub fn report_once(&self, key: ChartDiagnosticKey, message: String) {
        let inserted = self
            .reported
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key);
        if inserted {
            (self.reporter)(message);
        }
    }

    pub fn clear(&self, key: &ChartDiagnosticKey) {
        self.reported
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(key);
    }
}
