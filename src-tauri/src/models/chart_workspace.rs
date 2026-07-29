use crate::models::{config::KLINE_INTERVALS, market::Kline};
use serde::{Deserialize, Serialize};

pub const CHART_WORKSPACE_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_CHART_LOAD_LIMIT: usize = 200;
pub const MAX_CHART_KLINES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceKey {
    pub symbol: String,
    pub interval: String,
}

impl ChartWorkspaceKey {
    pub fn parse(symbol: &str, interval: &str) -> Result<Self, String> {
        let symbol = symbol.trim().to_ascii_uppercase();
        let interval = interval.trim().to_string();
        let valid_symbol = (1..=64).contains(&symbol.len())
            && symbol.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            });

        if !valid_symbol {
            return Err(
                "symbol must be 1-64 uppercase ASCII letters, digits, underscores, or hyphens"
                    .into(),
            );
        }
        if !KLINE_INTERVALS.contains(&interval.as_str()) {
            return Err("unsupported chart interval".into());
        }

        Ok(Self { symbol, interval })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChartPaneRef {
    Candle,
    Indicator { indicator_name: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPointSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartOverlaySnapshot {
    pub id: String,
    pub group_id: String,
    pub pane: ChartPaneRef,
    pub name: String,
    pub lock: bool,
    pub visible: bool,
    pub z_level: i32,
    pub mode: String,
    pub mode_sensitivity: f64,
    pub points: Vec<ChartPointSnapshot>,
    pub extend_data: serde_json::Value,
    pub styles: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartViewportSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar_space: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_timestamp: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartViewStateV1 {
    pub schema_version: u32,
    pub symbol: String,
    pub interval: String,
    pub revision: u64,
    pub saved_at_ms: i64,
    pub overlays: Vec<ChartOverlaySnapshot>,
    pub viewport: ChartViewportSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartPreferencesV1 {
    pub schema_version: u32,
    pub revision: u64,
    pub saved_at_ms: i64,
    pub main_indicators: Vec<String>,
    pub sub_indicators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceSnapshot {
    pub key: ChartWorkspaceKey,
    pub klines: Vec<Kline>,
    pub view_state: ChartViewStateV1,
    pub preferences: ChartPreferencesV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveChartWorkspaceRequest {
    pub key: ChartWorkspaceKey,
    #[serde(default)]
    pub view_state: Option<ChartViewStateV1>,
    #[serde(default)]
    pub preferences: Option<ChartPreferencesV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartWorkspaceSaveResult {
    pub key: ChartWorkspaceKey,
    pub view_revision: u64,
    pub preferences_revision: u64,
    pub saved_at_ms: i64,
    pub kline_saved: bool,
    pub view_state_saved: bool,
    pub preferences_saved: bool,
    pub kline_error: Option<String>,
    pub view_state_error: Option<String>,
    pub preferences_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_key_normalizes_symbol_and_accepts_supported_intervals() {
        let key = ChartWorkspaceKey::parse(" btcusdt ", "15").unwrap();
        assert_eq!(key.symbol, "BTCUSDT");
        assert_eq!(key.interval, "15");
    }

    #[test]
    fn workspace_key_rejects_path_segments_and_unknown_intervals() {
        assert!(ChartWorkspaceKey::parse("../BTCUSDT", "15").is_err());
        assert!(ChartWorkspaceKey::parse("BTC/USDT", "15").is_err());
        assert!(ChartWorkspaceKey::parse("BTCUSDT", "2").is_err());
    }

    #[test]
    fn pane_reference_and_workspace_snapshot_serialize_camel_case() {
        let value = serde_json::to_value(sample_snapshot()).unwrap();
        assert_eq!(value["viewState"]["schemaVersion"], 1);
        assert_eq!(
            value["viewState"]["overlays"][0]["pane"],
            serde_json::json!({ "kind": "indicator", "indicatorName": "MACD" })
        );
    }

    fn sample_snapshot() -> ChartWorkspaceSnapshot {
        let key = ChartWorkspaceKey::parse("BTCUSDT", "15").unwrap();
        ChartWorkspaceSnapshot {
            key: key.clone(),
            klines: Vec::new(),
            view_state: ChartViewStateV1 {
                schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
                symbol: key.symbol,
                interval: key.interval,
                revision: 1,
                saved_at_ms: 0,
                overlays: vec![ChartOverlaySnapshot {
                    id: "segment-1".into(),
                    group_id: "group-1".into(),
                    pane: ChartPaneRef::Indicator {
                        indicator_name: "MACD".into(),
                    },
                    name: "segment".into(),
                    lock: false,
                    visible: true,
                    z_level: 0,
                    mode: "normal".into(),
                    mode_sensitivity: 8.0,
                    points: vec![ChartPointSnapshot {
                        timestamp: Some(1_000),
                        data_index: None,
                        value: Some(1.0),
                    }],
                    extend_data: serde_json::Value::Null,
                    styles: serde_json::Value::Null,
                }],
                viewport: ChartViewportSnapshot::default(),
            },
            preferences: ChartPreferencesV1 {
                schema_version: CHART_WORKSPACE_SCHEMA_VERSION,
                revision: 1,
                saved_at_ms: 0,
                main_indicators: vec!["MA".into(), "EMA".into()],
                sub_indicators: vec!["VOL".into(), "MACD".into()],
            },
        }
    }
}
