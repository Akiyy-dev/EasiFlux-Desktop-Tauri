use std::future::Future;
use std::pin::Pin;

use tauri::State;

use crate::api::endpoints;
use crate::api::mapper::{build_order_query_params, parse_orders, parse_positions};
use crate::api::response::{describe_data_shape, extract_list, first_object_keys, response_code};
use crate::api::{ApiClient, PrivateApi};
use crate::error::{AppError, AppResult};
use crate::models::account::Balance;
use crate::models::config::ApiCredential;
use crate::models::diagnostic::{ProbeEndpointResult, ProbePrivateEndpointsResult};
use crate::services::account_profiles::run_account_private_operation;
use crate::state::AppState;

type ProbeFuture<'a> = Pin<Box<dyn Future<Output = ProbeEndpointResult> + Send + 'a>>;

fn failed_probe_endpoint(endpoint: &str, error: AppError) -> ProbeEndpointResult {
    let notification_id = match &error {
        AppError::Notified {
            code: "AUTH_SESSION_EXPIRED",
            notification_id,
            ..
        } if !notification_id.trim().is_empty() => Some(notification_id.clone()),
        _ => None,
    };
    ProbeEndpointResult {
        endpoint: endpoint.into(),
        success: false,
        code: None,
        data_type: String::new(),
        data_keys: vec![],
        envelope_hint: String::new(),
        raw_count: 0,
        parsed_count: 0,
        first_item_keys: vec![],
        error: Some(error.user_message()),
        notification_id,
    }
}

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
                notification_id: None,
            }
        }
        Err(error) => failed_probe_endpoint(endpoint, error),
    }
}

async fn run_probe_sequence<B>(
    balances: B,
    probes: Vec<ProbeFuture<'_>>,
) -> ProbePrivateEndpointsResult
where
    B: Future<Output = AppResult<Vec<Balance>>>,
{
    let (balances_ok, balance_count) = match balances.await {
        Ok(balances) => (true, balances.len() as u32),
        Err(error) => {
            let failure = failed_probe_endpoint("account/balances", error);
            if failure.notification_id.is_some() {
                return ProbePrivateEndpointsResult {
                    balances_ok: false,
                    balance_count: 0,
                    endpoints: vec![failure],
                };
            }
            (false, 0)
        }
    };
    let mut endpoints = Vec::with_capacity(probes.len());
    for probe in probes {
        let result = probe.await;
        let owns_session_notification = result.notification_id.is_some();
        endpoints.push(result);
        if owns_session_notification {
            break;
        }
    }
    ProbePrivateEndpointsResult {
        balances_ok,
        balance_count,
        endpoints,
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
        let api = &api;
        let probes: Vec<ProbeFuture<'_>> = vec![
            Box::pin(probe_endpoint(
                "activity-orders",
                || async {
                    let params = build_order_query_params(
                        None, None, None, None, None, None, None, None, None, None,
                    );
                    api.private_get(endpoints::OPEN_ORDERS, params).await
                },
                |p| parse_orders(p).len(),
            )),
            Box::pin(probe_endpoint(
                "position/list",
                || async {
                    let params = build_order_query_params(
                        None, None, None, None, None, None, None, None, None, None,
                    );
                    api.private_get(endpoints::POSITIONS, params).await
                },
                |p| parse_positions(p).len(),
            )),
            Box::pin(probe_endpoint(
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
            )),
            Box::pin(probe_endpoint(
                "trade/fills",
                || async {
                    PrivateApi::trade_fills(api, None, None, None, None, None, None, Some(20), None)
                        .await
                },
                |p| extract_list(p).len(),
            )),
            Box::pin(probe_endpoint(
                "position/closed-pnl",
                || async {
                    PrivateApi::closed_pnl(api, None, None, None, None, Some(20), None).await
                },
                |p| extract_list(p).len(),
            )),
        ];
        Ok(run_probe_sequence(PrivateApi::balances(api, None), probes).await)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use crate::error::{AppError, NotificationCause};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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

    #[tokio::test]
    async fn shared_session_marker_stops_remaining_probe_requests() {
        let endpoint_calls = Arc::new(AtomicUsize::new(0));
        let first_calls = Arc::clone(&endpoint_calls);
        let second_calls = Arc::clone(&endpoint_calls);
        let probes: Vec<ProbeFuture<'_>> = vec![
            Box::pin(async move {
                first_calls.fetch_add(1, Ordering::SeqCst);
                probe_endpoint("first", || async { Ok(serde_json::json!({})) }, |_| 0).await
            }),
            Box::pin(async move {
                second_calls.fetch_add(1, Ordering::SeqCst);
                probe_endpoint("second", || async { Ok(serde_json::json!({})) }, |_| 0).await
            }),
        ];

        let result = run_probe_sequence(
            async {
                Err(AppError::Notified {
                    code: "AUTH_SESSION_EXPIRED",
                    message: "账户会话已失效",
                    notification_id: "notification-probe-session".into(),
                    cause: Some(NotificationCause::AuthFailure(
                        crate::api::response::AuthFailureKind::SessionExpired,
                    )),
                })
            },
            probes,
        )
        .await;

        assert!(!result.balances_ok);
        assert_eq!(result.endpoints.len(), 1);
        assert_eq!(
            result.endpoints[0].notification_id.as_deref(),
            Some("notification-probe-session")
        );
        assert_eq!(endpoint_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ordinary_probe_failures_remain_visible_and_continue() {
        let endpoint_calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&endpoint_calls);
        let probes: Vec<ProbeFuture<'_>> = vec![Box::pin(async move {
            counted.fetch_add(1, Ordering::SeqCst);
            probe_endpoint(
                "ordinary",
                || async { Err(AppError::Connection("probe unavailable".into())) },
                |_| 0,
            )
            .await
        })];

        let result = run_probe_sequence(
            async { Err(AppError::Connection("balances unavailable".into())) },
            probes,
        )
        .await;

        assert!(!result.balances_ok);
        assert_eq!(endpoint_calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.endpoints.len(), 1);
        assert!(result.endpoints[0].notification_id.is_none());
        assert_eq!(
            result.endpoints[0].error.as_deref(),
            Some("连接错误: probe unavailable")
        );

        let invalid_marker = failed_probe_endpoint(
            "invalid-marker",
            AppError::Notified {
                code: "AUTH_SESSION_EXPIRED",
                message: "账户会话已失效",
                notification_id: "".into(),
                cause: Some(NotificationCause::AuthFailure(
                    crate::api::response::AuthFailureKind::SessionExpired,
                )),
            },
        );
        assert!(invalid_marker.notification_id.is_none());
    }
}
