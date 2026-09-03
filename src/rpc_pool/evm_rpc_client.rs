use std::{collections::HashMap, time::Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::shared::{
    error::ApplicationError,
    hex::{
        extract_method_id, normalize_hex, parse_hex_u64, parse_hex_u64_quantity,
        parse_hex_u256_to_dec,
    },
    validation::{normalize_address, normalize_hash},
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
pub struct JsonRpcResponse<T> {
    pub id: Option<u64>,
    pub result: Option<T>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RpcTransaction {
    pub hash: String,
    pub block_number: Option<String>,
    pub block_hash: Option<String>,
    pub transaction_index: Option<String>,
    pub from: String,
    pub to: Option<String>,
    pub value: String,
    pub nonce: String,
    pub gas: String,
    pub gas_price: Option<String>,
    pub max_fee_per_gas: Option<String>,
    pub max_priority_fee_per_gas: Option<String>,
    #[serde(
        rename = "type",
        default,
        deserialize_with = "deserialize_optional_hex_or_num_u64"
    )]
    pub tx_type: Option<u64>,
    pub input: Option<String>,
}

fn deserialize_optional_hex_or_num_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum HexOrNum {
        Num(u64),
        Str(String),
        Null,
    }

    match Option::<HexOrNum>::deserialize(deserializer)? {
        Some(HexOrNum::Num(n)) => Ok(Some(n)),
        Some(HexOrNum::Str(s)) => {
            let trimmed = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(&s);
            if trimmed.is_empty() {
                Ok(None)
            } else {
                u64::from_str_radix(trimmed, 16)
                    .map(Some)
                    .map_err(serde::de::Error::custom)
            }
        }
        Some(HexOrNum::Null) | None => Ok(None),
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RpcBlock {
    pub number: Option<String>,
    pub hash: Option<String>,
    pub parent_hash: String,
    pub timestamp: String,
    pub gas_limit: String,
    pub gas_used: String,
    pub base_fee_per_gas: Option<String>,
    pub miner: Option<String>,
    pub author: Option<String>,
    pub transactions_root: Option<String>,
    pub receipts_root: Option<String>,
    pub state_root: Option<String>,
    pub size: Option<String>,
    pub withdrawals_root: Option<String>,
    pub blob_gas_used: Option<String>,
    pub excess_blob_gas: Option<String>,
    pub parent_beacon_block_root: Option<String>,
    #[serde(default)]
    pub transactions: Vec<RpcTransaction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBlock {
    pub chain_id: i64,
    pub block_number: i64,
    pub block_hash: String,
    pub parent_hash: String,
    pub timestamp: i64,
    pub gas_limit: String,
    pub gas_used: String,
    pub base_fee_per_gas: Option<String>,
    pub beneficiary: Option<String>,
    pub transactions_root: Option<String>,
    pub receipts_root: Option<String>,
    pub state_root: Option<String>,
    pub size: Option<String>,
    pub withdrawals_root: Option<String>,
    pub blob_gas_used: Option<String>,
    pub excess_blob_gas: Option<String>,
    pub parent_beacon_block_root: Option<String>,
    pub transaction_count: i64,
    pub transactions: Vec<DecodedTransaction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTransaction {
    pub chain_id: i64,
    pub tx_hash: String,
    pub block_number: i64,
    pub transaction_index: i64,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: String,
    pub nonce: String,
    pub gas: String,
    pub gas_price: Option<String>,
    pub max_fee_per_gas: Option<String>,
    pub max_priority_fee_per_gas: Option<String>,
    pub tx_type: Option<i64>,
    pub method_id: Option<String>,
}

pub fn decode_rpc_block(
    chain_id: i64,
    rpc_block: RpcBlock,
    expected_block_number: Option<i64>,
) -> Result<DecodedBlock, ApplicationError> {
    let block_number_str = rpc_block.number.as_deref().ok_or_else(|| {
        ApplicationError::ExternalService("block missing number in RPC response".to_owned())
    })?;
    let block_number = parse_hex_u64_quantity(block_number_str)? as i64;
    if let Some(expected) = expected_block_number
        && block_number != expected
    {
        return Err(ApplicationError::ExternalService(format!(
            "block number mismatch: expected {expected}, got {block_number}"
        )));
    }

    let block_hash_str = rpc_block.hash.as_deref().ok_or_else(|| {
        ApplicationError::ExternalService("block missing hash in RPC response".to_owned())
    })?;
    let block_hash = normalize_hash(block_hash_str)?;
    let parent_hash = normalize_hash(&rpc_block.parent_hash)?;
    let timestamp = parse_hex_u64_quantity(&rpc_block.timestamp)? as i64;
    let gas_limit = parse_hex_u256_to_dec(&rpc_block.gas_limit)?;
    let gas_used = parse_hex_u256_to_dec(&rpc_block.gas_used)?;
    let base_fee_per_gas = rpc_block
        .base_fee_per_gas
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_hex_u256_to_dec)
        .transpose()?;

    let beneficiary = match (
        rpc_block.miner.as_deref().filter(|s| !s.is_empty()),
        rpc_block.author.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(m), _) => Some(normalize_address(m)?),
        (None, Some(a)) => Some(normalize_address(a)?),
        (None, None) => None,
    };

    let transactions_root = rpc_block
        .transactions_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalize_hash)
        .transpose()?;
    let receipts_root = rpc_block
        .receipts_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalize_hash)
        .transpose()?;
    let state_root = rpc_block
        .state_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalize_hash)
        .transpose()?;
    let size = rpc_block
        .size
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_hex_u256_to_dec)
        .transpose()?;
    let withdrawals_root = rpc_block
        .withdrawals_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalize_hash)
        .transpose()?;
    let blob_gas_used = rpc_block
        .blob_gas_used
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_hex_u256_to_dec)
        .transpose()?;
    let excess_blob_gas = rpc_block
        .excess_blob_gas
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(parse_hex_u256_to_dec)
        .transpose()?;
    let parent_beacon_block_root = rpc_block
        .parent_beacon_block_root
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(normalize_hash)
        .transpose()?;

    let transaction_count = rpc_block.transactions.len() as i64;
    let mut transactions = Vec::with_capacity(rpc_block.transactions.len());

    for (idx, tx) in rpc_block.transactions.into_iter().enumerate() {
        let tx_hash = normalize_hash(&tx.hash)?;
        let tx_block_number = if let Some(bn) = tx.block_number.as_deref() {
            let parsed_bn = parse_hex_u64_quantity(bn)? as i64;
            if parsed_bn != block_number {
                return Err(ApplicationError::ExternalService(format!(
                    "transaction {tx_hash} blockNumber {parsed_bn} does not match block {block_number}"
                )));
            }
            parsed_bn
        } else {
            block_number
        };

        let transaction_index = if let Some(t_idx) = tx.transaction_index.as_deref() {
            parse_hex_u64_quantity(t_idx)? as i64
        } else {
            idx as i64
        };

        let from_address = normalize_address(&tx.from)?;
        let to_address = tx
            .to
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "0x")
            .map(normalize_address)
            .transpose()?;

        let value = parse_hex_u256_to_dec(&tx.value)?;
        let nonce = parse_hex_u256_to_dec(&tx.nonce)?;
        let gas = parse_hex_u256_to_dec(&tx.gas)?;
        let gas_price = tx
            .gas_price
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_hex_u256_to_dec)
            .transpose()?;
        let max_fee_per_gas = tx
            .max_fee_per_gas
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_hex_u256_to_dec)
            .transpose()?;
        let max_priority_fee_per_gas = tx
            .max_priority_fee_per_gas
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(parse_hex_u256_to_dec)
            .transpose()?;
        let tx_type = tx.tx_type.map(|t| t as i64);
        let method_id = tx.input.as_deref().and_then(extract_method_id);

        transactions.push(DecodedTransaction {
            chain_id,
            tx_hash,
            block_number: tx_block_number,
            transaction_index,
            from_address,
            to_address,
            value,
            nonce,
            gas,
            gas_price,
            max_fee_per_gas,
            max_priority_fee_per_gas,
            tx_type,
            method_id,
        });
    }

    Ok(DecodedBlock {
        chain_id,
        block_number,
        block_hash,
        parent_hash,
        timestamp,
        gas_limit,
        gas_used,
        base_fee_per_gas,
        beneficiary,
        transactions_root,
        receipts_root,
        state_root,
        size,
        withdrawals_root,
        blob_gas_used,
        excess_blob_gas,
        parent_beacon_block_root,
        transaction_count,
        transactions,
    })
}

pub fn validate_block_sequence(blocks: &[DecodedBlock]) -> Result<(), ApplicationError> {
    for window in blocks.windows(2) {
        let prev = &window[0];
        let curr = &window[1];
        if curr.block_number != prev.block_number + 1 {
            return Err(ApplicationError::ExternalService(format!(
                "block height gap detected: prev {}, curr {}",
                prev.block_number, curr.block_number
            )));
        }
        if curr.parent_hash != prev.block_hash {
            return Err(ApplicationError::ExternalService(format!(
                "parent hash mismatch at block {}: parent_hash {}, prev block_hash {}",
                curr.block_number, curr.parent_hash, prev.block_hash
            )));
        }
    }
    Ok(())
}

pub async fn check_endpoint(
    client: &Client,
    rpc_url: &str,
) -> Result<RpcHealthCheck, ApplicationError> {
    let started_at = Instant::now();
    let block_number = eth_block_number(client, rpc_url).await?;
    let _logs = eth_get_logs(client, rpc_url, &[ZERO_ADDRESS], block_number, block_number).await?;
    Ok(RpcHealthCheck {
        latency_ms: started_at.elapsed().as_millis() as i64,
    })
}

pub async fn eth_block_number(client: &Client, rpc_url: &str) -> Result<i64, ApplicationError> {
    let result: String = call(client, rpc_url, "eth_blockNumber", json!([])).await?;
    parse_hex_u64(&result)
}

pub async fn eth_get_block_by_number(
    client: &Client,
    rpc_url: &str,
    chain_id: i64,
    block_number: i64,
) -> Result<Option<DecodedBlock>, ApplicationError> {
    let hex_num = format!("0x{:x}", block_number);
    let result: Option<RpcBlock> = call(
        client,
        rpc_url,
        "eth_getBlockByNumber",
        json!([hex_num, true]),
    )
    .await?;

    match result {
        Some(rpc_block) => {
            let decoded = decode_rpc_block(chain_id, rpc_block, Some(block_number))?;
            Ok(Some(decoded))
        }
        None => Ok(None),
    }
}

pub async fn eth_get_blocks_by_number_batch(
    client: &Client,
    rpc_url: &str,
    chain_id: i64,
    block_numbers: &[i64],
) -> Result<Vec<DecodedBlock>, ApplicationError> {
    if block_numbers.is_empty() {
        return Ok(Vec::new());
    }

    let requests: Vec<JsonRpcRequest> = block_numbers
        .iter()
        .map(|&bn| JsonRpcRequest {
            jsonrpc: "2.0",
            method: "eth_getBlockByNumber",
            params: json!([format!("0x{:x}", bn), true]),
            id: bn as u64,
        })
        .collect();

    let http_response = client
        .post(rpc_url)
        .json(&requests)
        .send()
        .await
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?
        .error_for_status()
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?;

    let response_value = http_response
        .json::<Value>()
        .await
        .map_err(|error| ApplicationError::ExternalService(error.to_string()))?;

    parse_batch_block_response(chain_id, block_numbers, response_value)
}

pub fn parse_batch_block_response(
    chain_id: i64,
    block_numbers: &[i64],
    response_value: Value,
) -> Result<Vec<DecodedBlock>, ApplicationError> {
    match response_value {
        Value::Array(items) => {
            let mut response_by_id: HashMap<u64, Result<Option<RpcBlock>, String>> =
                HashMap::with_capacity(items.len());

            for item in items {
                let parsed_item: JsonRpcResponse<RpcBlock> =
                    serde_json::from_value(item).map_err(|err| {
                        ApplicationError::ExternalService(format!(
                            "failed to parse json-rpc response item: {err}"
                        ))
                    })?;

                let id = parsed_item.id.ok_or_else(|| {
                    ApplicationError::ExternalService(
                        "json-rpc batch response item missing id".to_owned(),
                    )
                })?;

                if let Some(err) = parsed_item.error {
                    response_by_id.insert(
                        id,
                        Err(format!("json-rpc error {}: {}", err.code, err.message)),
                    );
                } else {
                    response_by_id.insert(id, Ok(parsed_item.result));
                }
            }

            let mut decoded_blocks = Vec::with_capacity(block_numbers.len());
            for &bn in block_numbers {
                let resp_res = response_by_id.get(&(bn as u64)).ok_or_else(|| {
                    ApplicationError::ExternalService(format!(
                        "json-rpc batch response missing result for block {bn}"
                    ))
                })?;

                match resp_res {
                    Ok(Some(rpc_block)) => {
                        let decoded = decode_rpc_block(chain_id, rpc_block.clone(), Some(bn))?;
                        decoded_blocks.push(decoded);
                    }
                    Ok(None) => {
                        return Err(ApplicationError::ExternalService(format!(
                            "json-rpc returned null for block {bn}"
                        )));
                    }
                    Err(err_msg) => {
                        return Err(ApplicationError::ExternalService(format!(
                            "block {bn} failed with json-rpc error: {err_msg}"
                        )));
                    }
                }
            }

            validate_block_sequence(&decoded_blocks)?;
            Ok(decoded_blocks)
        }
        Value::Object(map) => {
            if let Some(error_val) = map.get("error")
                && let Ok(error_obj) = serde_json::from_value::<JsonRpcError>(error_val.clone())
            {
                return Err(ApplicationError::ExternalService(format!(
                    "json-rpc batch error {}: {}",
                    error_obj.code, error_obj.message
                )));
            }
            Err(ApplicationError::ExternalService(
                "unexpected single response object for batch request".to_owned(),
            ))
        }
        _ => Err(ApplicationError::ExternalService(
            "unexpected response type from json-rpc batch".to_owned(),
        )),
    }
}

pub(crate) fn build_get_logs_filter<S: AsRef<str>>(
    contract_addresses: &[S],
    from_block: i64,
    to_block: i64,
) -> Value {
    let mut filter = serde_json::Map::new();
    if contract_addresses.len() == 1 {
        filter.insert(
            "address".to_owned(),
            json!(normalize_hex(contract_addresses[0].as_ref())),
        );
    } else if contract_addresses.len() > 1 {
        let addresses: Vec<String> = contract_addresses
            .iter()
            .map(|a| normalize_hex(a.as_ref()))
            .collect();
        filter.insert("address".to_owned(), json!(addresses));
    }
    filter.insert("fromBlock".to_owned(), json!(format!("0x{:x}", from_block)));
    filter.insert("toBlock".to_owned(), json!(format!("0x{:x}", to_block)));
    json!([filter])
}

pub async fn eth_get_logs<S: AsRef<str>>(
    client: &Client,
    rpc_url: &str,
    contract_addresses: &[S],
    from_block: i64,
    to_block: i64,
) -> Result<Vec<RpcLog>, ApplicationError> {
    let params = build_get_logs_filter(contract_addresses, from_block, to_block);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_full_rpc_block_and_transactions() {
        let block_json = json!({
            "number": "0x1b4",
            "hash": "0x4a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a7",
            "parentHash": "0x3a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a6",
            "timestamp": "0x64b8a210",
            "gasLimit": "0x1c9c380",
            "gasUsed": "0x5208",
            "baseFeePerGas": "0x7",
            "miner": "0x000000000000000000000000000000000000dEaD",
            "transactionsRoot": "0x5a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a8",
            "receiptsRoot": "0x6a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a9",
            "stateRoot": "0x7a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60aa",
            "size": "0x120",
            "withdrawalsRoot": "0x8a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60ab",
            "blobGasUsed": "0x0",
            "excessBlobGas": "0x0",
            "parentBeaconBlockRoot": "0x9a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60ac",
            "transactions": [
                {
                    "hash": "0x111102cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001",
                    "blockNumber": "0x1b4",
                    "transactionIndex": "0x0",
                    "from": "0x0000000000000000000000000000000000000001",
                    "to": "0x0000000000000000000000000000000000000002",
                    "value": "0xde0b6b3a7640000",
                    "nonce": "0x1",
                    "gas": "0x5208",
                    "gasPrice": "0x3b9aca00",
                    "maxFeePerGas": "0x3b9aca00",
                    "maxPriorityFeePerGas": "0x3b9aca00",
                    "type": "0x2",
                    "input": "0xa9059cbb000000000000000000000000"
                },
                {
                    "hash": "0x222202cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6002",
                    "blockNumber": "0x1b4",
                    "transactionIndex": "0x1",
                    "from": "0x0000000000000000000000000000000000000003",
                    "to": null,
                    "value": "0x0",
                    "nonce": "0x0",
                    "gas": "0x186a0",
                    "type": 0,
                    "input": "0x"
                }
            ]
        });

        let rpc_block: RpcBlock =
            serde_json::from_value(block_json).expect("deserializes rpc block");
        let decoded = decode_rpc_block(1, rpc_block, Some(436)).expect("decodes block");

        assert_eq!(decoded.chain_id, 1);
        assert_eq!(decoded.block_number, 436);
        assert_eq!(
            decoded.beneficiary,
            Some("0x000000000000000000000000000000000000dead".to_owned())
        );
        assert_eq!(decoded.gas_limit, "30000000");
        assert_eq!(decoded.gas_used, "21000");
        assert_eq!(decoded.base_fee_per_gas, Some("7".to_owned()));
        assert_eq!(decoded.transaction_count, 2);

        let tx0 = &decoded.transactions[0];
        assert_eq!(
            tx0.tx_hash,
            "0x111102cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001"
        );
        assert_eq!(tx0.transaction_index, 0);
        assert_eq!(tx0.value, "1000000000000000000");
        assert_eq!(tx0.tx_type, Some(2));
        assert_eq!(tx0.method_id, Some("0xa9059cbb".to_owned()));

        let tx1 = &decoded.transactions[1];
        assert_eq!(tx1.to_address, None);
        assert_eq!(tx1.value, "0");
        assert_eq!(tx1.tx_type, Some(0));
        assert_eq!(tx1.method_id, None);
    }

    #[test]
    fn decodes_author_fallback_and_missing_optional_fields() {
        let block_json = json!({
            "number": "0x1",
            "hash": "0x4a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a7",
            "parentHash": "0x3a1b02cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a60a6",
            "timestamp": "0x10",
            "gasLimit": "0x1000",
            "gasUsed": "0x0",
            "author": "0x00000000000000000000000000000000000000aa",
            "transactions": []
        });

        let rpc_block: RpcBlock =
            serde_json::from_value(block_json).expect("deserializes rpc block");
        let decoded = decode_rpc_block(10, rpc_block, None).expect("decodes block");

        assert_eq!(
            decoded.beneficiary,
            Some("0x00000000000000000000000000000000000000aa".to_owned())
        );
        assert_eq!(decoded.base_fee_per_gas, None);
        assert_eq!(decoded.withdrawals_root, None);
        assert_eq!(decoded.transaction_count, 0);
    }

    #[test]
    fn parse_batch_block_response_handles_out_of_order_responses() {
        let raw_batch_response = json!([
            {
                "id": 102,
                "result": {
                    "number": "0x66",
                    "hash": "0x0000000000000000000000000000000000000000000000000000000000000102",
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000101",
                    "timestamp": "0x64b8a212",
                    "gasLimit": "0x10000",
                    "gasUsed": "0x10",
                    "transactions": []
                }
            },
            {
                "id": 100,
                "result": {
                    "number": "0x64",
                    "hash": "0x0000000000000000000000000000000000000000000000000000000000000100",
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000099",
                    "timestamp": "0x64b8a210",
                    "gasLimit": "0x10000",
                    "gasUsed": "0x0",
                    "transactions": []
                }
            },
            {
                "id": 101,
                "result": {
                    "number": "0x65",
                    "hash": "0x0000000000000000000000000000000000000000000000000000000000000101",
                    "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000100",
                    "timestamp": "0x64b8a211",
                    "gasLimit": "0x10000",
                    "gasUsed": "0x0",
                    "transactions": []
                }
            }
        ]);

        let block_numbers = vec![100, 101, 102];
        let decoded = parse_batch_block_response(1, &block_numbers, raw_batch_response)
            .expect("parses out of order batch");

        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded[0].block_number, 100);
        assert_eq!(decoded[1].block_number, 101);
        assert_eq!(decoded[2].block_number, 102);
    }

    #[test]
    fn parse_batch_block_response_fails_on_single_error_or_discontinuity() {
        let raw_batch_error = json!([
            {
                "id": 100,
                "error": {
                    "code": -32000,
                    "message": "header not found"
                }
            },
            {
                "id": 101,
                "result": null
            }
        ]);

        let res = parse_batch_block_response(1, &[100, 101], raw_batch_error);
        assert!(res.is_err());
    }

    #[test]
    fn build_get_logs_filter_handles_empty_single_and_multiple_addresses() {
        // 1. Empty address filter (all_events)
        let empty_filter = build_get_logs_filter(&[] as &[&str], 100, 200);
        assert_eq!(
            empty_filter,
            json!([{
                "fromBlock": "0x64",
                "toBlock": "0xc8"
            }])
        );

        // 2. Single address filter
        let single_filter =
            build_get_logs_filter(&["0x1111111111111111111111111111111111111111"], 100, 200);
        assert_eq!(
            single_filter,
            json!([{
                "address": "0x1111111111111111111111111111111111111111",
                "fromBlock": "0x64",
                "toBlock": "0xc8"
            }])
        );

        // 3. Multi-address array filter
        let multi_filter = build_get_logs_filter(
            &[
                "0x1111111111111111111111111111111111111111",
                "0x2222222222222222222222222222222222222222",
            ],
            100,
            200,
        );
        assert_eq!(
            multi_filter,
            json!([{
                "address": [
                    "0x1111111111111111111111111111111111111111",
                    "0x2222222222222222222222222222222222222222"
                ],
                "fromBlock": "0x64",
                "toBlock": "0xc8"
            }])
        );
    }
}
