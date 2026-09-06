use serde_json::Value;
use tauri::State;

use crate::api::PrivateApi;
use crate::error::AppResult;
use crate::models::api_requests::{
    ApiAddMarginRequest, ApiCancelAllOrdersRequest, ApiCloseAllPositionsRequest,
    ApiCreateTpslRequest, ApiReplaceOrderRequest, ApiReplaceTpslRequest, ApiSetLeverageRequest,
    ApiSwitchMarginModeRequest, ApiSwitchSeparatePositionModeRequest,
};
use crate::models::config::normalize_account_id;
use crate::models::trading::{
    CancelOrderRequest, Order, OrderStreamContext, PlaceOrderRequest, SubmissionContext,
};
use crate::services::account_profiles::run_account_private_mutation;
use crate::state::AppState;

async fn run_value_mutation<F, Fut>(state: &AppState, mutation: F) -> AppResult<Value>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = AppResult<Value>>,
{
    run_account_private_mutation(state.account_lifecycle.as_ref(), mutation).await
}

fn new_submission_context(
    submission_id: String,
    account_id: String,
    session_epoch: u64,
) -> SubmissionContext {
    SubmissionContext {
        submission_id,
        account_id,
        session_epoch,
    }
}

#[tauri::command]
pub async fn place_order(
    state: State<'_, AppState>,
    mut request: PlaceOrderRequest,
) -> AppResult<Order> {
    let submission_id = uuid::Uuid::new_v4().to_string();
    request
        .order_link_id
        .get_or_insert_with(|| submission_id.clone());
    run_account_private_mutation(state.account_lifecycle.as_ref(), || async {
        let account_id = normalize_account_id(&state.config.read().await.active_account_id);
        let scope = state
            .api
            .order_submission_scope(&account_id)
            .await
            .map_err(|error| {
                crate::error::AppError::OrderSubmissionRejected(error.user_message())
            })?;
        let context = new_submission_context(
            submission_id,
            account_id,
            state.account_lifecycle.current_session_epoch(),
        );
        crate::services::order_submission::submit_once(
            &state.order_submissions,
            &scope,
            request,
            state.time.local_now_ms(),
            |request| state.trading.place_order(context, request),
        )
        .await
    })
    .await
}

#[tauri::command]
pub async fn list_pending_order_submissions(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::models::order_submission::PendingOrderSubmission>> {
    crate::services::account_profiles::run_account_private_operation(
        state.account_lifecycle.as_ref(),
        || async {
            let account_id = normalize_account_id(&state.config.read().await.active_account_id);
            let scope = state.api.order_submission_scope(&account_id).await?;
            state.order_submissions.list_pending(&scope)
        },
    )
    .await
}

#[tauri::command]
pub async fn acknowledge_order_submission(
    state: State<'_, AppState>,
    order_link_id: String,
) -> AppResult<()> {
    run_account_private_mutation(state.account_lifecycle.as_ref(), || async {
        let account_id = normalize_account_id(&state.config.read().await.active_account_id);
        let scope = state.api.order_submission_scope(&account_id).await?;
        state.order_submissions.acknowledge(&scope, &order_link_id)
    })
    .await
}

#[tauri::command]
pub async fn reconcile_order_submission(
    state: State<'_, AppState>,
    order_link_id: String,
) -> AppResult<Option<Order>> {
    run_account_private_mutation(state.account_lifecycle.as_ref(), || async {
        let account_id = normalize_account_id(&state.config.read().await.active_account_id);
        let scope = state.api.order_submission_scope(&account_id).await?;
        let api = &state.api;
        let order = crate::services::order_submission::reconcile(
            &state.order_submissions,
            &scope,
            &order_link_id,
            |request| async move {
                let params = crate::api::mapper::build_order_query_params(
                    Some(&request.symbol),
                    None,
                    None,
                    request.order_link_id.as_deref(),
                    None,
                    Some(100),
                    None,
                    None,
                    None,
                    None,
                );
                let (open, history) = tokio::try_join!(
                    api.private_get(crate::api::endpoints::OPEN_ORDERS, params.clone()),
                    api.private_get(crate::api::endpoints::ORDERS, params),
                )?;
                let mut orders = crate::api::mapper::parse_orders(&open);
                orders.extend(crate::api::mapper::parse_orders(&history));
                Ok(orders)
            },
        )
        .await?;
        if let Some(order) = &order {
            let context = OrderStreamContext {
                account_id,
                session_epoch: state.account_lifecycle.current_session_epoch(),
            };
            state.emitter.emit_order(&context, order.clone());
            state.analytics.record_order(order.clone()).await;
        }
        Ok(order)
    })
    .await
}

#[tauri::command]
pub async fn cancel_order(
    state: State<'_, AppState>,
    request: CancelOrderRequest,
) -> AppResult<Order> {
    run_account_private_mutation(state.account_lifecycle.as_ref(), || async {
        let context = OrderStreamContext {
            account_id: normalize_account_id(&state.config.read().await.active_account_id),
            session_epoch: state.account_lifecycle.current_session_epoch(),
        };
        state.trading.cancel_order(context, request).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_context_is_a_single_v4_identity_with_bound_session() {
        let submission_id = uuid::Uuid::new_v4().to_string();
        let context = new_submission_context(submission_id.clone(), "alpha".into(), 17);
        let parsed = uuid::Uuid::parse_str(&context.submission_id).unwrap();

        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
        assert_eq!(context.submission_id, submission_id);
        assert_eq!(context.account_id, "alpha");
        assert_eq!(context.session_epoch, 17);
    }
}

#[tauri::command]
pub async fn cancel_all_orders(
    state: State<'_, AppState>,
    request: ApiCancelAllOrdersRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::cancel_all_orders(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn replace_order(
    state: State<'_, AppState>,
    request: ApiReplaceOrderRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::replace_order(&state.api, &request)).await
}

#[tauri::command]
pub async fn set_leverage(
    state: State<'_, AppState>,
    request: ApiSetLeverageRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::set_leverage(&state.api, &request)).await
}

#[tauri::command]
pub async fn add_margin(
    state: State<'_, AppState>,
    request: ApiAddMarginRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::add_margin(&state.api, &request)).await
}

#[tauri::command]
pub async fn close_all_positions(
    state: State<'_, AppState>,
    request: ApiCloseAllPositionsRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::close_all_positions(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn create_tpsl(
    state: State<'_, AppState>,
    request: ApiCreateTpslRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::create_tpsl(&state.api, &request)).await
}

#[tauri::command]
pub async fn replace_tpsl(
    state: State<'_, AppState>,
    request: ApiReplaceTpslRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || PrivateApi::replace_tpsl(&state.api, &request)).await
}

#[tauri::command]
pub async fn switch_margin_mode(
    state: State<'_, AppState>,
    request: ApiSwitchMarginModeRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::switch_margin_mode(&state.api, &request)
    })
    .await
}

#[tauri::command]
pub async fn switch_separate_position_mode(
    state: State<'_, AppState>,
    request: ApiSwitchSeparatePositionModeRequest,
) -> AppResult<Value> {
    run_value_mutation(&state, || {
        PrivateApi::switch_separate_position_mode(&state.api, &request)
    })
    .await
}
