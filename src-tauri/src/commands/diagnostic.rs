use tauri::State;

use crate::api::mapper::{build_order_query_params, parse_orders, parse_positions};
use crate::api::response::{describe_data_shape, extract_list, first_object_keys, response_code};
use crate::api::{ApiClient, PrivateApi};
use crate::api::endpoints;
use crate::error::AppResult;
use crate::models::config::ApiCredential;
use crate::models::diagnostic::{ProbeEndpointResult, ProbePrivateEndpointsResult};
use crate::state::AppState;

async fn probe_endpoint<F, Fut>(
    endpoint: &str,
    fetch: F,
    parse_count: impl Fn(&serde_json::Value) -> usize,
) -> ProbeEndpointResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<serde_json::Value>>,
{
    match fetch().await {
        Ok(payload) => {
            let (data_type, data_keys) = describe_data_shape(&payload);
            let meta = crate::api::mapper::list_envelope_meta(&payload);
            let parsed_count = parse_count(&payload);
            ProbeEndpointResult {
                endpoint: endpoint.into(),
                success: true,
                code: response_code(&payload),
                data_type,
                data_keys,
                envelope_hint: meta.hint,
                raw_count: meta.raw_count as u32,
                parsed_count: parsed_count as u32,
                first_item_keys: first_object_keys(&payload),
                error: None,
            }
        }
        Err(e) => ProbeEndpointResult {
            endpoint: endpoint.into(),
            success: false,
            code: None,
            data_type: String::new(),
            data_keys: vec![],
            envelope_hint: String::new(),
            raw_count: 0,
            parsed_count: 0,
            first_item_keys: vec![],
            error: Some(e.user_message()),
        },
    }
}

#[tauri::command]
pub async fn probe_private_endpoints(
    state: State<'_, AppState>,
    credential: Option<ApiCredential>,
) -> AppResult<ProbePrivateEndpointsResult> {
    if let Some(cred) = credential {
        state.api.set_credential(cred.normalize()).await;
    }

    let balances = PrivateApi::balances(&state.api, None).await;
    let balances_ok = balances.is_ok();
    let balance_count = balances.as_ref().map(|b| b.len() as u32).unwrap_or(0);

    let api = &state.api;
    let endpoints_result = vec![
        probe_endpoint(
            "activity-orders",
            || async {
                let params = build_order_query_params(
                    None, None, None, None, None, None, None, None, None, None,
                );
                api.private_get(endpoints::OPEN_ORDERS, params).await
            },
            |p| parse_orders(p).len(),
        )
        .await,
        probe_endpoint(
            "position/list",
            || async {
                let params = build_order_query_params(
                    None, None, None, None, None, None, None, None, None, None,
                );
                api.private_get(endpoints::POSITIONS, params).await
            },
            |p| parse_positions(p).len(),
        )
        .await,
        probe_endpoint(
            "trade/orders",
            || async {
                let params = build_order_query_params(
                    None, None, None, None, None, Some(20), None, None, None, None,
                );
                api.private_get(endpoints::ORDERS, params).await
            },
            |p| parse_orders(p).len(),
        )
        .await,
        probe_endpoint(
            "trade/fills",
            || async {
                PrivateApi::trade_fills(
                    api,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(20),
                    None,
                )
                .await
            },
            |p| extract_list(p).len(),
        )
        .await,
        probe_endpoint(
            "position/closed-pnl",
            || async {
                PrivateApi::closed_pnl(
                    api,
                    None,
                    None,
                    None,
                    None,
                    Some(20),
                    None,
                )
                .await
            },
            |p| extract_list(p).len(),
        )
        .await,
    ];

    Ok(ProbePrivateEndpointsResult {
        balances_ok,
        balance_count,
        endpoints: endpoints_result,
    })
}
