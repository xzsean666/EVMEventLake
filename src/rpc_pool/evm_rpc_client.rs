use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::shared::{
    error::ApplicationError,
    hex::{normalize_hex, parse_hex_u64},
};

const ZERO_ADDRESS: &str = "0x0000000000000000000000000000000000000000";

#[derive(Debug, Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug)]
pub struct RpcHealthCheck {
    pub latency_ms: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RpcLog {
    pub address: String,
    pub block_hash: String,
    pub block_number: String,
    pub data: String,
    pub log_index: String,
    pub removed: Option<bool>,
    pub topics: Vec<String>,
    pub transaction_hash: String,
    pub transaction_index: String,
}

pub async fn check_endpoint(
    client: &Client,
    rpc_url: &str,
) -> Result<RpcHealthCheck, ApplicationError> {
    let started_at = Instant::now();
    let block_number = eth_block_number(client, rpc_url).await?;
    let _logs = eth_get_logs(client, rpc_url, ZERO_ADDRESS, block_number, block_number).await?;
    Ok(RpcHealthCheck {
        latency_ms: started_at.elapsed().as_millis() as i64,
    })
}

pub async fn eth_block_number(client: &Client, rpc_url: &str) -> Result<i64, ApplicationError> {
    let result: String = call(client, rpc_url, "eth_blockNumber", json!([])).await?;
    parse_hex_u64(&result)
}

pub async fn eth_get_logs(
    client: &Client,
    rpc_url: &str,
    contract_address: &str,
    from_block: i64,
    to_block: i64,
) -> Result<Vec<RpcLog>, ApplicationError> {
    let params = json!([{
        "address": normalize_hex(contract_address),
        "fromBlock": format!("0x{:x}", from_block),
        "toBlock": format!("0x{:x}", to_block)
    }]);

    call(client, rpc_url, "eth_getLogs", params).await
}

async fn call<T>(
    client: &Client,
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<T, ApplicationError>
where
    T: for<'de> Deserialize<'de>,
{
    let request = JsonRpcRequest {
        jsonrpc: "2.0",
        method,
        params,
        id: 1,
    };

    let response = client
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?
        .error_for_status()
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?
        .json::<JsonRpcResponse<T>>()
        .await
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?;

    if let Some(error) = response.error {
        return Err(ApplicationError::ExternalService(format!(
            "json-rpc error {}: {}",
            error.code, error.message
        )));
    }

    response.result.ok_or_else(|| {
        ApplicationError::ExternalService("json-rpc response missing result".to_owned())
    })
}
