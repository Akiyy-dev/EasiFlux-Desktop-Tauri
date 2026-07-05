use serde_json::Value;

use crate::error::AppResult;
use crate::models::account::Balance;
use crate::models::api_requests::{
    ApiAddMarginRequest, ApiCancelAllOrdersRequest, ApiCloseAllPositionsRequest,
    ApiCreateTpslRequest, ApiReplaceOrderRequest, ApiReplaceTpslRequest,
    ApiSetLeverageRequest, ApiSwitchMarginModeRequest, ApiSwitchSeparatePositionModeRequest,
    ApiTransferRequest,
};
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest, Position};

use super::client::ApiClient;
use super::endpoints;
use super::mapper::{
    build_cancel_order_body, build_order_query_params, build_place_order_body, build_transfer_history_params,
    parse_balances, parse_order, parse_orders, parse_positions,
};
use super::response::extract_list;

pub struct PrivateApi;

impl PrivateApi {
    pub async fn open_orders(client: &ApiClient, symbol: Option<&str>) -> AppResult<Vec<Order>> {
        let params = build_order_query_params(symbol, None, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::OPEN_ORDERS, params).await?;
        Ok(parse_orders(&payload))
    }

    pub async fn orders(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
        order_id: Option<&str>,
        order_link_id: Option<&str>,
        order_filter: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> AppResult<Value> {
        let params = build_order_query_params(
            symbol,
            coin,
            order_id,
            order_link_id,
            order_filter,
            limit,
            cursor,
            None,
            None,
            None,
        );
        client.private_get(endpoints::ORDERS, params).await
    }

    pub async fn trade_fills(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
        order_id: Option<&str>,
        start_time: Option<i64>,
        end_time: Option<i64>,
        exec_type: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> AppResult<Value> {
        let params = build_order_query_params(
            symbol,
            coin,
            order_id,
            None,
            None,
            limit,
            cursor,
            start_time,
            end_time,
            exec_type,
        );
        client.private_get(endpoints::TRADE_FILLS, params).await
    }

    pub async fn fee_rate(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
    ) -> AppResult<Value> {
        let params = build_order_query_params(symbol, coin, None, None, None, None, None, None, None, None);
        client.private_get(endpoints::FEE_RATE, params).await
    }

    pub async fn balances(client: &ApiClient, coin: Option<&str>) -> AppResult<Vec<Balance>> {
        let params = build_order_query_params(None, coin, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::BALANCES, params).await?;
        Ok(parse_balances(&payload))
    }

    pub async fn positions(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
    ) -> AppResult<Vec<Position>> {
        let params = build_order_query_params(symbol, coin, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::POSITIONS, params).await?;
        Ok(parse_positions(&payload))
    }

    pub async fn create_order(client: &ApiClient, request: &PlaceOrderRequest) -> AppResult<Order> {
        let body = build_place_order_body(request);
        let payload = client.private_post(endpoints::CREATE_ORDER, body).await?;
        let items = extract_list(&payload);
        if let Some(first) = items.first() {
            return Ok(parse_order(first));
        }
        Ok(parse_order(super::response::extract_data(&payload)))
    }

    pub async fn cancel_order(client: &ApiClient, request: &CancelOrderRequest) -> AppResult<Order> {
        let body = build_cancel_order_body(request);
        let payload = client.private_post(endpoints::CANCEL_ORDER, body).await?;
        let items = extract_list(&payload);
        if let Some(first) = items.first() {
            return Ok(parse_order(first));
        }
        Ok(parse_order(&serde_json::json!({
            "order_id": request.order_id,
            "order_link_id": request.order_link_id,
            "symbol": request.symbol,
            "status": "Cancelled"
        })))
    }

    pub async fn cancel_all_orders(
        client: &ApiClient,
        request: &ApiCancelAllOrdersRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::CANCEL_ALL_ORDERS, request.to_value())
            .await
    }

    pub async fn replace_order(
        client: &ApiClient,
        request: &ApiReplaceOrderRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::REPLACE_ORDER, request.to_value())
            .await
    }

    pub async fn set_leverage(
        client: &ApiClient,
        request: &ApiSetLeverageRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::SET_LEVERAGE, request.to_value())
            .await
    }

    pub async fn add_margin(client: &ApiClient, request: &ApiAddMarginRequest) -> AppResult<Value> {
        client
            .private_post(endpoints::ADD_MARGIN, request.to_value())
            .await
    }

    pub async fn close_all_positions(
        client: &ApiClient,
        request: &ApiCloseAllPositionsRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::CLOSE_ALL_POSITIONS, request.to_value())
            .await
    }

    pub async fn closed_pnl(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> AppResult<Value> {
        let params = build_order_query_params(
            symbol,
            coin,
            None,
            None,
            None,
            limit,
            cursor,
            start_time,
            end_time,
            None,
        );
        client.private_get(endpoints::CLOSED_PNL, params).await
    }

    pub async fn create_tpsl(
        client: &ApiClient,
        request: &ApiCreateTpslRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::CREATE_TPSL, request.to_value())
            .await
    }

    pub async fn replace_tpsl(
        client: &ApiClient,
        request: &ApiReplaceTpslRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::REPLACE_TPSL, request.to_value())
            .await
    }

    pub async fn switch_margin_mode(
        client: &ApiClient,
        request: &ApiSwitchMarginModeRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::SWITCH_MARGIN_MODE, request.to_value())
            .await
    }

    pub async fn switch_separate_position_mode(
        client: &ApiClient,
        request: &ApiSwitchSeparatePositionModeRequest,
    ) -> AppResult<Value> {
        client
            .private_post(
                endpoints::SWITCH_SEPARATE_POSITION_MODE,
                request.to_value(),
            )
            .await
    }

    pub async fn funding_balances(client: &ApiClient) -> AppResult<Value> {
        client
            .private_get(endpoints::FUNDING_BALANCES, Vec::new())
            .await
    }

    pub async fn transfer_funds(
        client: &ApiClient,
        request: &ApiTransferRequest,
    ) -> AppResult<Value> {
        client
            .private_post(endpoints::FUNDING_TRANSFER, request.to_value())
            .await
    }

    pub async fn user_id(client: &ApiClient) -> AppResult<Value> {
        client
            .private_get(endpoints::USER_ID, Vec::new())
            .await
    }

    pub async fn transfer_history(
        client: &ApiClient,
        start_time: i64,
        end_time: i64,
        coin: Option<&str>,
        page_num: Option<u32>,
        page_size: Option<u32>,
    ) -> AppResult<Value> {
        let params = build_transfer_history_params(start_time, end_time, coin, page_num, page_size);
        client
            .private_get(endpoints::TRANSFER_HISTORY, params)
            .await
    }
}
