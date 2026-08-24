use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::models::account::Balance;
use crate::models::api_requests::{
    ApiAddMarginRequest, ApiCancelAllOrdersRequest, ApiCloseAllPositionsRequest,
    ApiCreateTpslRequest, ApiReplaceOrderRequest, ApiReplaceTpslRequest, ApiSetLeverageRequest,
    ApiSwitchMarginModeRequest, ApiSwitchSeparatePositionModeRequest, ApiTransferRequest,
};
use crate::models::trading::{CancelOrderRequest, Order, PlaceOrderRequest, Position};

use super::client::ApiClient;
use super::endpoints;
use super::mapper::{
    build_cancel_order_body, build_order_query_params, build_place_order_body,
    build_transfer_history_params, parse_balances, parse_order, parse_orders, parse_positions,
};
use super::response::{extract_list, CreateOrderOutcome};

pub struct PrivateApi;

impl PrivateApi {
    pub async fn open_orders(client: &ApiClient, symbol: Option<&str>) -> AppResult<Vec<Order>> {
        let params =
            build_order_query_params(symbol, None, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::OPEN_ORDERS, params).await?;
        Ok(parse_orders(&payload))
    }

    pub async fn order_history(
        client: &ApiClient,
        symbol: Option<&str>,
        limit: Option<u32>,
    ) -> AppResult<Vec<Order>> {
        let params = build_order_query_params(
            symbol, None, None, None, None, limit, None, None, None, None,
        );
        let payload = client.private_get(endpoints::ORDERS, params).await?;
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
            symbol, coin, order_id, None, None, limit, cursor, start_time, end_time, exec_type,
        );
        client.private_get(endpoints::TRADE_FILLS, params).await
    }

    pub async fn fee_rate(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
    ) -> AppResult<Value> {
        let params =
            build_order_query_params(symbol, coin, None, None, None, None, None, None, None, None);
        client.private_get(endpoints::FEE_RATE, params).await
    }

    pub async fn balances(client: &ApiClient, coin: Option<&str>) -> AppResult<Vec<Balance>> {
        let params =
            build_order_query_params(None, coin, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::BALANCES, params).await?;
        Ok(parse_balances(&payload))
    }

    pub async fn positions(
        client: &ApiClient,
        symbol: Option<&str>,
        coin: Option<&str>,
    ) -> AppResult<Vec<Position>> {
        let params =
            build_order_query_params(symbol, coin, None, None, None, None, None, None, None, None);
        let payload = client.private_get(endpoints::POSITIONS, params).await?;
        Ok(parse_positions(&payload))
    }

    pub async fn create_order(client: &ApiClient, request: &PlaceOrderRequest) -> AppResult<Order> {
        let body = build_place_order_body(request);
        let payload = client.private_post(endpoints::CREATE_ORDER, body).await?;
        Self::parse_create_order_payload(&payload)
    }

    fn parse_create_order_payload(payload: &Value) -> AppResult<Order> {
        let order = match super::response::classify_create_order_outcome(payload) {
            CreateOrderOutcome::Accepted(candidate) => parse_order(candidate),
            CreateOrderOutcome::Rejected => {
                return Err(AppError::TradingFailure(
                    crate::models::trading::TradingFailure::rejected(),
                ));
            }
            CreateOrderOutcome::ProviderFailure | CreateOrderOutcome::Ambiguous => {
                return Err(AppError::Internal("订单提交结果不明确".into()));
            }
        };
        if order.order_id.trim().is_empty() {
            return Err(AppError::Internal("订单提交结果不明确".into()));
        }
        Ok(order)
    }

    pub async fn cancel_order(
        client: &ApiClient,
        request: &CancelOrderRequest,
    ) -> AppResult<Order> {
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
            symbol, coin, None, None, None, limit, cursor, start_time, end_time, None,
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
            .private_post(endpoints::SWITCH_SEPARATE_POSITION_MODE, request.to_value())
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
        client.private_get(endpoints::USER_ID, Vec::new()).await
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

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use super::*;
    use crate::models::config::ApiCredential;

    fn place_order_request(order_link_id: Option<&str>) -> PlaceOrderRequest {
        PlaceOrderRequest {
            symbol: "BTCUSDT".into(),
            side: "Buy".into(),
            order_type: "Market".into(),
            qty: "1".into(),
            position_idx: 0,
            price: None,
            time_in_force: None,
            order_link_id: order_link_id.map(str::to_owned),
            reduce_only: None,
        }
    }

    async fn create_order_from_response(payload: serde_json::Value) -> AppResult<Order> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let response_body = serde_json::to_string(&payload).unwrap();
        let server = std::thread::spawn(move || {
            for body in [
                r#"{"code":0,"data":{"time":"1782850580"}}"#.to_string(),
                response_body,
            ] {
                let (mut socket, _) = listener.accept().expect("accept API request");
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).expect("read API request");
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .write_all(response.as_bytes())
                    .expect("write API response");
            }
        });
        let client = ApiClient::new();
        client
            .set_credential(ApiCredential {
                api_key: "test-key".into(),
                api_secret: "test-secret".into(),
                base_url: format!("http://{address}"),
                label: "test".into(),
            })
            .await;

        let result = PrivateApi::create_order(&client, &place_order_request(None)).await;
        server.join().expect("test server exits");
        result
    }

    #[test]
    fn empty_create_order_success_body_is_ambiguous_not_a_confirmed_rejection() {
        let error = PrivateApi::parse_create_order_payload(&serde_json::json!({})).unwrap_err();

        assert!(matches!(error, AppError::Internal(_)));
        assert!(!matches!(error, AppError::TradingFailure(_)));
    }

    #[tokio::test]
    async fn undocumented_create_order_lists_are_ambiguous_while_direct_rejection_is_confirmed() {
        for data in [
            serde_json::json!({
                "items": [{"order_id": "123", "order_status": "Rejected"}]
            }),
            serde_json::json!({
                "rows": [{"order_id": "123", "order_status": "Rejected"}]
            }),
            serde_json::json!({
                "list": [{"order_id": "123", "order_status": "Rejected"}]
            }),
        ] {
            let error = create_order_from_response(serde_json::json!({
                "code": 0,
                "data": data,
            }))
            .await
            .expect_err("an undocumented create-order list is ambiguous");
            assert!(matches!(error, AppError::Internal(_)));
        }

        let error = create_order_from_response(serde_json::json!({
            "code": 0,
            "data": {"order_id": "direct-rejected-1", "orderStatus": "Rejected"},
        }))
        .await
        .expect_err("a documented direct rejection is confirmed");
        assert!(matches!(error, AppError::TradingFailure(_)));
    }
}
