use tauri::State;

use crate::api::endpoints;
use crate::api::mapper::{build_order_query_params, parse_orders, parse_positions};
use crate::api::response::{describe_data_shape, extract_list, first_object_keys, response_code};
use crate::api::{ApiClient, PrivateApi};
use crate::error::AppResult;
use crate::models::config::ApiCredential;
use crate::models::diagnostic::{ProbeEndpointResult, ProbePrivateEndpointsResult};
use crate::services::account_profiles::run_account_private_operation;
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

async fn select_probe_client(shared: &ApiClient, credential: Option<ApiCredential>) -> ApiClient {
    match credential {
        None => shared.clone(),
        Some(credential) => {
            let client = ApiClient::new();
            client.set_credential(credential.normalize()).await;
            client
        }
    }
}

#[tauri::command]
pub async fn probe_private_endpoints(
    state: State<'_, AppState>,
    credential: Option<ApiCredential>,
) -> AppResult<ProbePrivateEndpointsResult> {
    let coordinator = state.account_lifecycle.clone();
    run_account_private_operation(coordinator.as_ref(), || async move {
        let api = select_probe_client(state.api.as_ref(), credential).await;
        let balances = PrivateApi::balances(&api, None).await;
        let balances_ok = balances.is_ok();
        let balance_count = balances.as_ref().map(|b| b.len() as u32).unwrap_or(0);

        let api = &api;
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
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some(20),
                        None,
                        None,
                        None,
                        None,
                    );
                    api.private_get(endpoints::ORDERS, params).await
                },
                |p| parse_orders(p).len(),
            )
            .await,
            probe_endpoint(
                "trade/fills",
                || async {
                    PrivateApi::trade_fills(api, None, None, None, None, None, None, Some(20), None)
                        .await
                },
                |p| extract_list(p).len(),
            )
            .await,
            probe_endpoint(
                "position/closed-pnl",
                || async {
                    PrivateApi::closed_pnl(api, None, None, None, None, Some(20), None).await
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
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;

    fn credential(key: &str, secret: &str, base_url: &str) -> ApiCredential {
        ApiCredential {
            api_key: key.into(),
            api_secret: secret.into(),
            base_url: base_url.into(),
            label: "probe".into(),
        }
    }

    #[tokio::test]
    async fn optional_probe_credential_uses_an_isolated_client() {
        let shared = ApiClient::new();
        shared
            .set_credential(credential(
                "shared-key",
                "shared-secret",
                "https://shared.example.test/",
            ))
            .await;

        let probe = select_probe_client(
            &shared,
            Some(credential(
                "probe-key",
                "probe-secret",
                " https://probe.example.test/ ",
            )),
        )
        .await;

        assert_eq!(shared.base_url().await, "https://shared.example.test");
        assert!(shared.has_credential().await);
        assert_eq!(probe.base_url().await, "https://probe.example.test");
        assert!(probe.has_credential().await);
        probe.clear_credential().await;
        assert!(shared.has_credential().await);
    }

    #[tokio::test]
    async fn absent_probe_credential_uses_the_shared_client() {
        let shared = ApiClient::new();
        shared.set_base_url("https://shared.example.test").await;

        let probe = select_probe_client(&shared, None).await;
        probe.set_base_url("https://updated.example.test").await;

        assert_eq!(shared.base_url().await, "https://updated.example.test");
    }
}
