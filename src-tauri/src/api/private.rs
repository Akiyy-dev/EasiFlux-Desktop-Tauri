use crate::error::AppResult;
use crate::models::account::Balance;
use crate::models::trading::{Order, PlaceOrderRequest, Position};

use super::client::ApiClient;
use super::endpoints;
use super::mapper::{
    build_cancel_order_body, build_order_query_params, build_place_order_body, parse_balances,
    parse_order, parse_orders, parse_positions,
};
use super::response::extract_list;

pub struct PrivateApi;

impl PrivateApi {
    pub async fn open_orders(client: &ApiClient, symbol: Option<&str>) -> AppResult<Vec<Order>> {
        let params = build_order_query_params(symbol);
        let payload = client.private_get(endpoints::OPEN_ORDERS, params).await?;
        Ok(parse_orders(&payload))
    }

    pub async fn balances(client: &ApiClient) -> AppResult<Vec<Balance>> {
        let payload = client
            .private_get(endpoints::BALANCES, Default::default())
            .await?;
        Ok(parse_balances(&payload))
    }

    pub async fn positions(client: &ApiClient, symbol: Option<&str>) -> AppResult<Vec<Position>> {
        let params = build_order_query_params(symbol);
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

    pub async fn cancel_order(
        client: &ApiClient,
        symbol: &str,
        order_id: &str,
    ) -> AppResult<Order> {
        let body = build_cancel_order_body(symbol, order_id);
        let payload = client.private_post(endpoints::CANCEL_ORDER, body).await?;
        let items = extract_list(&payload);
        if let Some(first) = items.first() {
            return Ok(parse_order(first));
        }
        Ok(parse_order(&serde_json::json!({
            "orderId": order_id,
            "symbol": symbol,
            "status": "Cancelled"
        })))
    }
}
