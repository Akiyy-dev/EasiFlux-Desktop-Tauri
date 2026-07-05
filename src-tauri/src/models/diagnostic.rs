use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeEndpointResult {
    pub endpoint: String,
    pub success: bool,
    pub code: Option<String>,
    pub data_type: String,
    pub data_keys: Vec<String>,
    pub envelope_hint: String,
    pub raw_count: u32,
    pub parsed_count: u32,
    pub first_item_keys: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbePrivateEndpointsResult {
    pub balances_ok: bool,
    pub balance_count: u32,
    pub endpoints: Vec<ProbeEndpointResult>,
}
