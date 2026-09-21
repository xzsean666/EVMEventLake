use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::{OpenApi, ToSchema};

use crate::{
    api::response::{self, ApiResponse},
    app::application_state::ApplicationState,
    auth::{AuthenticatedPrincipal, Role},
    block_transaction::state::{self, BlockTransactionSyncStateRecord, UpdateSyncConfigRequest},
    shared::{
        error::ApplicationError,
        validation::{normalize_address, normalize_hash},
    },
};

pub fn routes() -> Router<ApplicationState> {
    Router::new()
        .route("/api/chains/{chain_id}/blocks/{block_ref}", get(get_block))
        .route(
            "/api/chains/{chain_id}/blocks/{block_ref}/transactions",
            get(get_block_transactions),
        )
        .route(
            "/api/chains/{chain_id}/blocks/{block_ref}/gas-consumers",
            get(get_block_gas_consumers),
        )
        .route(
            "/api/chains/{chain_id}/transactions/{tx_hash}",
            get(get_transaction),
        )
        .route(
            "/api/chains/{chain_id}/addresses/{address}/transactions",
            get(get_address_transactions),
        )
        .route(
            "/api/chains/{chain_id}/block-by-time",
            get(get_block_by_time),
        )
        .route(
            "/api/chains/{chain_id}/blocks-time-range",
            get(get_blocks_time_range),
        )
        .route(
            "/api/chains/{chain_id}/transactions/{tx_hash}/status",
            get(get_transaction_status),
        )
        .route(
            "/api/chains/{chain_id}/addresses/{address}/profile",
            get(get_address_profile),
        )
        .route(
            "/api/chains/{chain_id}/deployments",
            get(get_contract_deployments),
        )
        .route(
            "/api/chains/{chain_id}/gas-oracle",
            get(get_gas_oracle),
        )
        .route(
            "/api/chains/{chain_id}/network-stats",
            get(get_network_stats),
        )
        .route(
            "/api/chains/{chain_id}/whale-transfers",
            get(get_whale_transfers),
        )
        .route(
            "/api/chains/{chain_id}/top-contracts",
            get(get_top_contracts),
        )
        .route(
            "/api/chains/{chain_id}/failed-transactions",
            get(get_failed_transactions),
        )
        .route(
            "/api/chains/{chain_id}/block-transaction-sync",
            get(get_sync_status).put(update_sync_config),
        )
        .route("/api/chains/{chain_id}/sync-status", get(get_sync_status))
        .route(
            "/api/chains/{chain_id}/block-transaction-sync/pause",
            post(pause_sync),
        )
        .route(
            "/api/chains/{chain_id}/block-transaction-sync/resume",
            post(resume_sync),
        )
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_block,
        get_block_transactions,
        get_transaction,
        get_address_transactions,
        get_block_by_time,
        get_blocks_time_range,
        get_transaction_status,
        get_address_profile,
        get_contract_deployments,
        get_block_gas_consumers,
        get_gas_oracle,
        get_network_stats,
        get_whale_transfers,
        get_top_contracts,
        get_failed_transactions,
        get_sync_status,
        update_sync_config,
        pause_sync,
        resume_sync
    ),
    components(schemas(
        BlockDetailResponse,
        TransactionDetailResponse,
        BlockTimeRangeResponse,
        TransactionStatusResponse,
        AddressProfileResponse,
        BlockGasConsumerResponse,
        GasEstimate,
        GasOracleResponse,
        NetworkStatsResponse,
        TopContractResponse,
        BlockTransactionSyncStateRecord,
        UpdateSyncConfigRequest
    )),
    tags(
        (name = "block_transaction", description = "Block and transaction data endpoints")
    )
)]
struct BlockTransactionApiDocumentation;

pub fn openapi() -> utoipa::openapi::OpenApi {
    BlockTransactionApiDocumentation::openapi()
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct BlockDetailResponse {
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
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct TransactionDetailResponse {
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
    pub status: Option<u8>,
    pub gas_used: Option<String>,
    pub effective_gas_price: Option<String>,
    pub l1_fee: Option<String>,
}

impl From<crate::clickhouse::BlockRow> for BlockDetailResponse {
    fn from(row: crate::clickhouse::BlockRow) -> Self {
        Self {
            chain_id: row.chain_id as i64,
            block_number: row.block_number as i64,
            block_hash: row.block_hash,
            parent_hash: row.parent_hash,
            timestamp: row.timestamp as i64,
            gas_limit: row.gas_limit,
            gas_used: row.gas_used,
            base_fee_per_gas: row.base_fee_per_gas,
            beneficiary: row.beneficiary,
            transactions_root: row.transactions_root,
            receipts_root: row.receipts_root,
            state_root: row.state_root,
            size: row.size,
            withdrawals_root: row.withdrawals_root,
            blob_gas_used: row.blob_gas_used,
            excess_blob_gas: row.excess_blob_gas,
            parent_beacon_block_root: row.parent_beacon_block_root,
            transaction_count: row.transaction_count as i64,
        }
    }
}

impl From<crate::clickhouse::TransactionRow> for TransactionDetailResponse {
    fn from(row: crate::clickhouse::TransactionRow) -> Self {
        Self {
            chain_id: row.chain_id as i64,
            tx_hash: row.tx_hash,
            block_number: row.block_number as i64,
            transaction_index: row.transaction_index as i64,
            from_address: row.from_address,
            to_address: row.to_address,
            value: row.value,
            nonce: row.nonce,
            gas: row.gas,
            gas_price: row.gas_price,
            max_fee_per_gas: row.max_fee_per_gas,
            max_priority_fee_per_gas: row.max_priority_fee_per_gas,
            tx_type: row.tx_type.map(|t| t as i64),
            method_id: row.method_id,
            status: row.status,
            gas_used: row.gas_used,
            effective_gas_price: row.effective_gas_price,
            l1_fee: row.l1_fee,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BlockTransactionsQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddressTransactionsQuery {
    pub direction: Option<String>,
    pub from_block: Option<i64>,
    pub to_block: Option<i64>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct BlockTransactionsCursor {
    chain_id: i64,
    block_number: i64,
    transaction_index: u32,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct AddressTransactionsCursor {
    chain_id: i64,
    address: String,
    direction: String,
    block_number: u64,
    transaction_index: u32,
    tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct BlockTimeRangeResponse {
    pub chain_id: i64,
    pub start_time: i64,
    pub end_time: i64,
    pub start_block: Option<i64>,
    pub end_block: Option<i64>,
    pub block_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct TransactionStatusResponse {
    pub chain_id: i64,
    pub tx_hash: String,
    pub block_number: i64,
    pub status: Option<u8>,
    pub gas_used: Option<String>,
    pub current_head: i64,
    pub confirmations: i64,
    pub is_canonical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct AddressProfileResponse {
    pub chain_id: i64,
    pub address: String,
    pub first_block: Option<i64>,
    pub last_block: Option<i64>,
    pub sent_tx_count: u64,
    pub received_tx_count: u64,
    pub last_nonce: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BlockByTimeQuery {
    pub timestamp: i64,
    pub closest: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BlocksTimeRangeQuery {
    pub start_time: i64,
    pub end_time: i64,
}

#[derive(Debug, Deserialize)]
pub struct DeploymentsQuery {
    pub creator: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeploymentsCursor {
    chain_id: i64,
    creator: Option<String>,
    block_number: u64,
    transaction_index: u32,
    tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct BlockGasConsumerResponse {
    pub from_address: String,
    pub total_gas_used: String,
    pub tx_count: u64,
}

#[derive(Debug, Deserialize)]
pub struct BlockGasConsumersQuery {
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct GasEstimate {
    pub max_priority_fee_per_gas: String,
    pub max_fee_per_gas: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct GasOracleResponse {
    pub chain_id: i64,
    pub base_fee: Option<String>,
    pub slow: GasEstimate,
    pub normal: GasEstimate,
    pub fast: GasEstimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct NetworkStatsResponse {
    pub chain_id: i64,
    pub latest_block: i64,
    pub latest_timestamp: i64,
    pub tps_last_1h: f64,
    pub avg_gas_utilization_percent: f64,
    pub avg_block_time_seconds: f64,
}

#[derive(Debug, Deserialize)]
pub struct WhaleTransfersQuery {
    pub min_value: Option<String>,
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WhaleTransfersCursor {
    chain_id: i64,
    min_value: String,
    block_number: u64,
    transaction_index: u32,
    tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct TopContractResponse {
    pub contract_address: String,
    pub tx_count: u64,
    pub user_count: u64,
    pub total_gas_used: String,
}

#[derive(Debug, Deserialize)]
pub struct TopContractsQuery {
    pub window_blocks: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct FailedTransactionsQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FailedTransactionsCursor {
    chain_id: i64,
    block_number: u64,
    transaction_index: u32,
    tx_hash: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BlockRef {
    Number(i64),
    Hash(String),
}

pub fn parse_block_ref(block_ref: &str) -> Result<BlockRef, ApplicationError> {
    let trimmed = block_ref.trim();
    if trimmed.is_empty() {
        return Err(ApplicationError::BadRequest(
            "block_ref cannot be empty".to_owned(),
        ));
    }

    if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
        let hex_body = &trimmed[2..];
        if hex_body.len() == 64 && hex_body.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(BlockRef::Hash(trimmed.to_ascii_lowercase()));
        }
        if !hex_body.is_empty() && hex_body.chars().all(|c| c.is_ascii_hexdigit()) {
            let num = i64::from_str_radix(hex_body, 16).map_err(|_| {
                ApplicationError::BadRequest(format!("invalid hex block number: {block_ref}"))
            })?;
            if num < 0 {
                return Err(ApplicationError::BadRequest(
                    "block number cannot be negative".to_owned(),
                ));
            }
            return Ok(BlockRef::Number(num));
        }
        return Err(ApplicationError::BadRequest(format!(
            "invalid hex block_ref: {block_ref}"
        )));
    }

    if let Ok(num) = trimmed.parse::<i64>() {
        if num < 0 {
            return Err(ApplicationError::BadRequest(
                "block number cannot be negative".to_owned(),
            ));
        }
        return Ok(BlockRef::Number(num));
    }

    Err(ApplicationError::BadRequest(format!(
        "invalid block_ref format: {block_ref}"
    )))
}

#[allow(dead_code)]
fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, ApplicationError> {
    let bytes =
        serde_json::to_vec(cursor).map_err(|err| ApplicationError::Internal(err.to_string()))?;
    Ok(hex::encode(bytes))
}

#[allow(dead_code)]
fn decode_cursor<T: for<'de> Deserialize<'de>>(cursor_str: &str) -> Result<T, ApplicationError> {
    let bytes = hex::decode(cursor_str)
        .map_err(|_| ApplicationError::BadRequest("invalid opaque cursor token".to_owned()))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApplicationError::BadRequest("invalid cursor payload".to_owned()))
}

fn require_clickhouse_client(
    state: &ApplicationState,
) -> Result<clickhouse::Client, ApplicationError> {
    if !state.configuration.clickhouse.enabled {
        return Err(ApplicationError::ServiceUnavailable(
            "block transaction storage unavailable: ClickHouse is disabled".to_owned(),
        ));
    }

    state.clickhouse_client().ok_or_else(|| {
        ApplicationError::ServiceUnavailable(
            "block transaction storage unavailable: ClickHouse client is not connected".to_owned(),
        )
    })
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/blocks/{block_ref}",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("block_ref" = String, Path, description = "Block height or 32-byte hex hash")
    ),
    responses(
        (status = 200, description = "Block detail", body = ApiResponse<BlockDetailResponse>),
        (status = 400, description = "Invalid chain_id or block_ref"),
        (status = 404, description = "Block not found"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_block(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, block_ref)): Path<(i64, String)>,
) -> Result<Json<ApiResponse<BlockDetailResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let parsed_ref = parse_block_ref(&block_ref)?;

    let client = require_clickhouse_client(&state)?;
    let block_opt = match parsed_ref {
        BlockRef::Number(num) => {
            crate::clickhouse::get_block_by_number(&client, chain_id, num).await?
        }
        BlockRef::Hash(ref hash) => {
            crate::clickhouse::get_block_by_hash(&client, chain_id, hash).await?
        }
    };

    let block = block_opt.ok_or_else(|| {
        ApplicationError::NotFound(format!("block {block_ref} on chain {chain_id} not found"))
    })?;

    Ok(response::success(BlockDetailResponse::from(block)))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/blocks/{block_ref}/transactions",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("block_ref" = String, Path, description = "Block height or 32-byte hex hash"),
        ("limit" = Option<u64>, Query, description = "Page size (default 100, max 1000)"),
        ("cursor" = Option<String>, Query, description = "Opaque keyset cursor")
    ),
    responses(
        (status = 200, description = "Block transactions", body = ApiResponse<Vec<TransactionDetailResponse>>),
        (status = 400, description = "Invalid parameters or cursor"),
        (status = 404, description = "Block not found"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_block_transactions(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, block_ref)): Path<(i64, String)>,
    Query(query): Query<BlockTransactionsQuery>,
) -> Result<Json<ApiResponse<Vec<TransactionDetailResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let parsed_ref = parse_block_ref(&block_ref)?;
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);

    let client = require_clickhouse_client(&state)?;
        let block_number = match parsed_ref {
            BlockRef::Number(num) => num,
            BlockRef::Hash(ref hash) => {
                let block = crate::clickhouse::get_block_by_hash(&client, chain_id, hash)
                    .await?
                    .ok_or_else(|| {
                        ApplicationError::NotFound(format!(
                            "block {block_ref} on chain {chain_id} not found"
                        ))
                    })?;
                block.block_number as i64
            }
        };

        let cursor_index = if let Some(ref token) = query.cursor {
            let cursor: BlockTransactionsCursor = decode_cursor(token)?;
            if cursor.chain_id != chain_id || cursor.block_number != block_number {
                return Err(ApplicationError::BadRequest(
                    "cursor parameters do not match requested chain/block".to_owned(),
                ));
            }
            Some(cursor.transaction_index)
        } else {
            None
        };

        let mut rows = crate::clickhouse::get_block_transactions(
            &client,
            chain_id,
            block_number,
            limit + 1,
            cursor_index,
        )
        .await?;

        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.pop();
        }

        let next_cursor = if has_more && !rows.is_empty() {
            let last_row = rows.last().unwrap();
            let next_index = last_row.transaction_index + 1;
            Some(encode_cursor(&BlockTransactionsCursor {
                chain_id,
                block_number,
                transaction_index: next_index,
            })?)
        } else {
            None
        };

        let tx_details: Vec<TransactionDetailResponse> = rows
            .into_iter()
            .map(TransactionDetailResponse::from)
            .collect();

        Ok(response::success_with_meta(
            tx_details,
            json!({
                "chain_id": chain_id,
                "block_number": block_number,
                "limit": limit,
                "has_more": has_more,
                "next_cursor": next_cursor
            }),
        ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/transactions/{tx_hash}",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("tx_hash" = String, Path, description = "32-byte hex transaction hash")
    ),
    responses(
        (status = 200, description = "Transaction detail", body = ApiResponse<TransactionDetailResponse>),
        (status = 400, description = "Invalid chain_id or tx_hash format"),
        (status = 404, description = "Transaction not found"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_transaction(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, tx_hash)): Path<(i64, String)>,
) -> Result<Json<ApiResponse<TransactionDetailResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let normalized_hash = normalize_hash(&tx_hash)?;

    let client = require_clickhouse_client(&state)?;
    let row = crate::clickhouse::get_transaction_by_hash(&client, chain_id, &normalized_hash)
        .await?
        .ok_or_else(|| {
            ApplicationError::NotFound(format!(
                "transaction {tx_hash} on chain {chain_id} not found"
            ))
        })?;

    Ok(response::success(TransactionDetailResponse::from(row)))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/addresses/{address}/transactions",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("address" = String, Path, description = "20-byte hex EVM address"),
        ("direction" = Option<String>, Query, description = "Transaction direction: from, to, or any (default: any)"),
        ("from_block" = Option<i64>, Query, description = "Minimum block number"),
        ("to_block" = Option<i64>, Query, description = "Maximum block number"),
        ("limit" = Option<u64>, Query, description = "Page size (default 100, max 1000)"),
        ("cursor" = Option<String>, Query, description = "Opaque keyset cursor")
    ),
    responses(
        (status = 200, description = "Address transactions", body = ApiResponse<Vec<TransactionDetailResponse>>),
        (status = 400, description = "Invalid parameters or cursor"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_address_transactions(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, address)): Path<(i64, String)>,
    Query(query): Query<AddressTransactionsQuery>,
) -> Result<Json<ApiResponse<Vec<TransactionDetailResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let normalized_address = normalize_address(&address)?;
    let direction_str = query.direction.as_deref().unwrap_or("any");
    let direction = match direction_str.to_ascii_lowercase().as_str() {
        "from" => "from",
        "to" => "to",
        "any" => "any",
        other => {
            return Err(ApplicationError::BadRequest(format!(
                "invalid direction '{other}': must be 'from', 'to', or 'any'"
            )));
        }
    };

    if let (Some(fb), Some(tb)) = (query.from_block, query.to_block)
        && fb > tb
    {
        return Err(ApplicationError::BadRequest(
            "from_block cannot be greater than to_block".to_owned(),
        ));
    }
    if let Some(fb) = query.from_block
        && fb < 0
    {
        return Err(ApplicationError::BadRequest(
            "from_block cannot be negative".to_owned(),
        ));
    }
    if let Some(tb) = query.to_block
        && tb < 0
    {
        return Err(ApplicationError::BadRequest(
            "to_block cannot be negative".to_owned(),
        ));
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);

    let client = require_clickhouse_client(&state)?;

        let cursor_tuple = if let Some(ref token) = query.cursor {
            let cursor: AddressTransactionsCursor = decode_cursor(token)?;
            if cursor.chain_id != chain_id
                || cursor.address != normalized_address
                || cursor.direction != direction
            {
                return Err(ApplicationError::BadRequest(
                    "cursor parameters do not match requested address query".to_owned(),
                ));
            }
            Some((
                cursor.block_number,
                cursor.transaction_index,
                cursor.tx_hash,
            ))
        } else {
            None
        };

        let mut rows = crate::clickhouse::get_address_transactions(
            &client,
            chain_id,
            &normalized_address,
            direction,
            query.from_block,
            query.to_block,
            limit + 1,
            cursor_tuple,
        )
        .await?;

        let has_more = rows.len() > limit as usize;
        if has_more {
            rows.pop();
        }

        let next_cursor = if has_more && !rows.is_empty() {
            let last_row = rows.last().unwrap();
            Some(encode_cursor(&AddressTransactionsCursor {
                chain_id,
                address: normalized_address.clone(),
                direction: direction.to_owned(),
                block_number: last_row.block_number,
                transaction_index: last_row.transaction_index,
                tx_hash: last_row.tx_hash.clone(),
            })?)
        } else {
            None
        };

        let tx_details: Vec<TransactionDetailResponse> = rows
            .into_iter()
            .map(TransactionDetailResponse::from)
            .collect();

        Ok(response::success_with_meta(
            tx_details,
            json!({
                "chain_id": chain_id,
                "address": normalized_address,
                "direction": direction,
                "limit": limit,
                "has_more": has_more,
                "next_cursor": next_cursor
            }),
        ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/block-by-time",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("timestamp" = i64, Query, description = "Unix timestamp in seconds"),
        ("closest" = Option<String>, Query, description = "Closest direction: before (default) or after")
    ),
    responses(
        (status = 200, description = "Block detail", body = ApiResponse<BlockDetailResponse>),
        (status = 400, description = "Invalid parameters"),
        (status = 404, description = "No block found near specified timestamp"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_block_by_time(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<BlockByTimeQuery>,
) -> Result<Json<ApiResponse<BlockDetailResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    if query.timestamp < 0 {
        return Err(ApplicationError::BadRequest(
            "timestamp cannot be negative".to_owned(),
        ));
    }

    let closest = match query
        .closest
        .as_deref()
        .unwrap_or("before")
        .to_ascii_lowercase()
        .as_str()
    {
        "before" => "before",
        "after" => "after",
        other => {
            return Err(ApplicationError::BadRequest(format!(
                "invalid closest '{other}': must be 'before' or 'after'"
            )));
        }
    };

    let client = require_clickhouse_client(&state)?;
    let block =
        crate::clickhouse::get_block_by_timestamp(&client, chain_id, query.timestamp, closest)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound(format!(
                    "no block found for timestamp {} ({closest}) on chain {chain_id}",
                    query.timestamp
                ))
            })?;

    Ok(response::success(BlockDetailResponse::from(block)))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/blocks-time-range",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("start_time" = i64, Query, description = "Start unix timestamp (seconds)"),
        ("end_time" = i64, Query, description = "End unix timestamp (seconds)")
    ),
    responses(
        (status = 200, description = "Block time range summary", body = ApiResponse<BlockTimeRangeResponse>),
        (status = 400, description = "Invalid parameters"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_blocks_time_range(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<BlocksTimeRangeQuery>,
) -> Result<Json<ApiResponse<BlockTimeRangeResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    if query.start_time < 0 || query.end_time < 0 {
        return Err(ApplicationError::BadRequest(
            "start_time and end_time cannot be negative".to_owned(),
        ));
    }
    if query.start_time > query.end_time {
        return Err(ApplicationError::BadRequest(
            "start_time cannot be greater than end_time".to_owned(),
        ));
    }

    let client = require_clickhouse_client(&state)?;
    let row = crate::clickhouse::get_blocks_time_range(
        &client,
        chain_id,
        query.start_time,
        query.end_time,
    )
    .await?;

    Ok(response::success(BlockTimeRangeResponse {
        chain_id,
        start_time: query.start_time,
        end_time: query.end_time,
        start_block: row.min_block.map(|b| b as i64),
        end_block: row.max_block.map(|b| b as i64),
        block_count: row.block_count,
    }))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/transactions/{tx_hash}/status",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("tx_hash" = String, Path, description = "32-byte hex transaction hash")
    ),
    responses(
        (status = 200, description = "Transaction status and confirmations", body = ApiResponse<TransactionStatusResponse>),
        (status = 400, description = "Invalid chain_id or tx_hash"),
        (status = 404, description = "Transaction not found"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_transaction_status(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, tx_hash)): Path<(i64, String)>,
) -> Result<Json<ApiResponse<TransactionStatusResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let normalized_hash = normalize_hash(&tx_hash)?;

    let client = require_clickhouse_client(&state)?;
    let (tx, current_head) =
        crate::clickhouse::get_transaction_status(&client, chain_id, &normalized_hash)
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound(format!(
                    "transaction {tx_hash} on chain {chain_id} not found"
                ))
            })?;

    let confirmations = if current_head >= tx.block_number {
        (current_head - tx.block_number + 1) as i64
    } else {
        1
    };

    Ok(response::success(TransactionStatusResponse {
        chain_id,
        tx_hash: tx.tx_hash,
        block_number: tx.block_number as i64,
        status: tx.status,
        gas_used: tx.gas_used,
        current_head: current_head as i64,
        confirmations,
        is_canonical: tx.is_canonical,
    }))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/addresses/{address}/profile",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("address" = String, Path, description = "20-byte hex EVM address")
    ),
    responses(
        (status = 200, description = "Address profile", body = ApiResponse<AddressProfileResponse>),
        (status = 400, description = "Invalid chain_id or address"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_address_profile(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, address)): Path<(i64, String)>,
) -> Result<Json<ApiResponse<AddressProfileResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let normalized_address = normalize_address(&address)?;

    let client = require_clickhouse_client(&state)?;
    let row =
        crate::clickhouse::get_address_profile(&client, chain_id, &normalized_address).await?;

    Ok(response::success(AddressProfileResponse {
        chain_id,
        address: normalized_address,
        first_block: row.first_block.map(|b| b as i64),
        last_block: row.last_block.map(|b| b as i64),
        sent_tx_count: row.sent_tx_count,
        received_tx_count: row.received_tx_count,
        last_nonce: row.last_nonce.map(|n| n.to_string()),
    }))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/deployments",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("creator" = Option<String>, Query, description = "20-byte hex contract creator address"),
        ("limit" = Option<u64>, Query, description = "Page size (default 100, max 1000)"),
        ("cursor" = Option<String>, Query, description = "Opaque keyset cursor")
    ),
    responses(
        (status = 200, description = "Contract deployment transactions", body = ApiResponse<Vec<TransactionDetailResponse>>),
        (status = 400, description = "Invalid parameters or cursor"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_contract_deployments(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<DeploymentsQuery>,
) -> Result<Json<ApiResponse<Vec<TransactionDetailResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let normalized_creator = match query.creator {
        Some(ref c) if !c.trim().is_empty() => Some(normalize_address(c)?),
        _ => None,
    };

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let client = require_clickhouse_client(&state)?;

    let cursor_tuple = if let Some(ref token) = query.cursor {
        let cursor: DeploymentsCursor = decode_cursor(token)?;
        if cursor.chain_id != chain_id || cursor.creator != normalized_creator {
            return Err(ApplicationError::BadRequest(
                "cursor parameters do not match requested deployment query".to_owned(),
            ));
        }
        Some((
            cursor.block_number,
            cursor.transaction_index,
            cursor.tx_hash,
        ))
    } else {
        None
    };

    let mut rows = crate::clickhouse::get_contract_deployments(
        &client,
        chain_id,
        normalized_creator.as_deref(),
        limit + 1,
        cursor_tuple,
    )
    .await?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = if has_more && !rows.is_empty() {
        let last_row = rows.last().unwrap();
        Some(encode_cursor(&DeploymentsCursor {
            chain_id,
            creator: normalized_creator.clone(),
            block_number: last_row.block_number,
            transaction_index: last_row.transaction_index,
            tx_hash: last_row.tx_hash.clone(),
        })?)
    } else {
        None
    };

    let tx_details: Vec<TransactionDetailResponse> = rows
        .into_iter()
        .map(TransactionDetailResponse::from)
        .collect();

    Ok(response::success_with_meta(
        tx_details,
        json!({
            "chain_id": chain_id,
            "creator": normalized_creator,
            "limit": limit,
            "has_more": has_more,
            "next_cursor": next_cursor
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/blocks/{block_ref}/gas-consumers",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("block_ref" = String, Path, description = "Block height or 32-byte hex hash"),
        ("limit" = Option<u64>, Query, description = "Max users to return (default 50, max 500)")
    ),
    responses(
        (status = 200, description = "Block gas consumers leaderboard", body = ApiResponse<Vec<BlockGasConsumerResponse>>),
        (status = 400, description = "Invalid chain_id or block_ref"),
        (status = 404, description = "Block not found"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_block_gas_consumers(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path((chain_id, block_ref)): Path<(i64, String)>,
    Query(query): Query<BlockGasConsumersQuery>,
) -> Result<Json<ApiResponse<Vec<BlockGasConsumerResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }
    let parsed_ref = parse_block_ref(&block_ref)?;
    let limit = query.limit.unwrap_or(50).clamp(1, 500);

    let client = require_clickhouse_client(&state)?;
    let block_number = match parsed_ref {
        BlockRef::Number(num) => num,
        BlockRef::Hash(ref hash) => {
            let block = crate::clickhouse::get_block_by_hash(&client, chain_id, hash)
                .await?
                .ok_or_else(|| {
                    ApplicationError::NotFound(format!(
                        "block {block_ref} on chain {chain_id} not found"
                    ))
                })?;
            block.block_number as i64
        }
    };

    let rows = crate::clickhouse::get_block_gas_consumers(&client, chain_id, block_number, limit).await?;
    let consumers: Vec<BlockGasConsumerResponse> = rows
        .into_iter()
        .map(|r| BlockGasConsumerResponse {
            from_address: r.from_address,
            total_gas_used: r.total_gas_used.unwrap_or(0).to_string(),
            tx_count: r.tx_count,
        })
        .collect();

    Ok(response::success_with_meta(
        consumers,
        json!({
            "chain_id": chain_id,
            "block_number": block_number,
            "limit": limit
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/gas-oracle",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    responses(
        (status = 200, description = "Gas oracle estimates", body = ApiResponse<GasOracleResponse>),
        (status = 400, description = "Invalid chain_id"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_gas_oracle(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<GasOracleResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let client = require_clickhouse_client(&state)?;
    let (base_fee, priority_fees) = crate::clickhouse::get_gas_oracle(&client, chain_id).await?;

    let base_fee_u64 = base_fee
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let slow_priority = priority_fees.slow_priority_fee.unwrap_or(0);
    let normal_priority = priority_fees.normal_priority_fee.unwrap_or(0);
    let fast_priority = priority_fees.fast_priority_fee.unwrap_or(0);

    let slow_max_fee = base_fee_u64.saturating_add(slow_priority);
    let normal_max_fee = (base_fee_u64.saturating_mul(1125) / 1000).saturating_add(normal_priority);
    let fast_max_fee = (base_fee_u64.saturating_mul(1250) / 1000).saturating_add(fast_priority);

    Ok(response::success(GasOracleResponse {
        chain_id,
        base_fee,
        slow: GasEstimate {
            max_priority_fee_per_gas: slow_priority.to_string(),
            max_fee_per_gas: slow_max_fee.to_string(),
        },
        normal: GasEstimate {
            max_priority_fee_per_gas: normal_priority.to_string(),
            max_fee_per_gas: normal_max_fee.to_string(),
        },
        fast: GasEstimate {
            max_priority_fee_per_gas: fast_priority.to_string(),
            max_fee_per_gas: fast_max_fee.to_string(),
        },
    }))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/network-stats",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    responses(
        (status = 200, description = "Realtime network health and TPS statistics", body = ApiResponse<NetworkStatsResponse>),
        (status = 400, description = "Invalid chain_id"),
        (status = 404, description = "No block data available for chain"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_network_stats(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<NetworkStatsResponse>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let client = require_clickhouse_client(&state)?;
    let (agg, tps_1h) = crate::clickhouse::get_network_stats(&client, chain_id)
        .await?
        .ok_or_else(|| {
            ApplicationError::NotFound(format!("no block data found on chain {chain_id}"))
        })?;

    Ok(response::success(NetworkStatsResponse {
        chain_id,
        latest_block: agg.latest_block as i64,
        latest_timestamp: agg.latest_timestamp as i64,
        tps_last_1h: (tps_1h * 100.0).round() / 100.0,
        avg_gas_utilization_percent: (agg.avg_gas_utilization * 100.0).round() / 100.0,
        avg_block_time_seconds: (agg.avg_block_time * 100.0).round() / 100.0,
    }))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/whale-transfers",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("min_value" = Option<String>, Query, description = "Minimum value in wei (default 1000000000000000000 / 1 ETH)"),
        ("limit" = Option<u64>, Query, description = "Page size (default 100, max 1000)"),
        ("cursor" = Option<String>, Query, description = "Opaque keyset cursor")
    ),
    responses(
        (status = 200, description = "Whale transfer transactions", body = ApiResponse<Vec<TransactionDetailResponse>>),
        (status = 400, description = "Invalid parameters or cursor"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_whale_transfers(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<WhaleTransfersQuery>,
) -> Result<Json<ApiResponse<Vec<TransactionDetailResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let min_value = query
        .min_value
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("1000000000000000000");

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let client = require_clickhouse_client(&state)?;

    let cursor_tuple = if let Some(ref token) = query.cursor {
        let cursor: WhaleTransfersCursor = decode_cursor(token)?;
        if cursor.chain_id != chain_id || cursor.min_value != min_value {
            return Err(ApplicationError::BadRequest(
                "cursor parameters do not match requested whale transfer query".to_owned(),
            ));
        }
        Some((
            cursor.block_number,
            cursor.transaction_index,
            cursor.tx_hash,
        ))
    } else {
        None
    };

    let mut rows = crate::clickhouse::get_whale_transfers(
        &client,
        chain_id,
        min_value,
        limit + 1,
        cursor_tuple,
    )
    .await?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = if has_more && !rows.is_empty() {
        let last_row = rows.last().unwrap();
        Some(encode_cursor(&WhaleTransfersCursor {
            chain_id,
            min_value: min_value.to_owned(),
            block_number: last_row.block_number,
            transaction_index: last_row.transaction_index,
            tx_hash: last_row.tx_hash.clone(),
        })?)
    } else {
        None
    };

    let tx_details: Vec<TransactionDetailResponse> = rows
        .into_iter()
        .map(TransactionDetailResponse::from)
        .collect();

    Ok(response::success_with_meta(
        tx_details,
        json!({
            "chain_id": chain_id,
            "min_value": min_value,
            "limit": limit,
            "has_more": has_more,
            "next_cursor": next_cursor
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/top-contracts",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("window_blocks" = Option<u64>, Query, description = "Recent block count window (default 1000, max 50000)"),
        ("limit" = Option<u64>, Query, description = "Leaderboard size (default 20, max 100)")
    ),
    responses(
        (status = 200, description = "Top active contracts", body = ApiResponse<Vec<TopContractResponse>>),
        (status = 400, description = "Invalid parameters"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_top_contracts(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<TopContractsQuery>,
) -> Result<Json<ApiResponse<Vec<TopContractResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let window_blocks = query.window_blocks.unwrap_or(1000).clamp(1, 50000);
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let client = require_clickhouse_client(&state)?;
    let rows = crate::clickhouse::get_top_contracts(&client, chain_id, window_blocks, limit).await?;

    let top_contracts: Vec<TopContractResponse> = rows
        .into_iter()
        .map(|r| TopContractResponse {
            contract_address: r.contract_address.unwrap_or_default(),
            tx_count: r.tx_count,
            user_count: r.user_count,
            total_gas_used: r.total_gas_used.unwrap_or(0).to_string(),
        })
        .collect();

    Ok(response::success_with_meta(
        top_contracts,
        json!({
            "chain_id": chain_id,
            "window_blocks": window_blocks,
            "limit": limit
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/failed-transactions",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id"),
        ("limit" = Option<u64>, Query, description = "Page size (default 100, max 1000)"),
        ("cursor" = Option<String>, Query, description = "Opaque keyset cursor")
    ),
    responses(
        (status = 200, description = "Failed transactions (status = 0)", body = ApiResponse<Vec<TransactionDetailResponse>>),
        (status = 400, description = "Invalid parameters or cursor"),
        (status = 503, description = "ClickHouse unavailable")
    )
)]
pub async fn get_failed_transactions(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Query(query): Query<FailedTransactionsQuery>,
) -> Result<Json<ApiResponse<Vec<TransactionDetailResponse>>>, ApplicationError> {
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let client = require_clickhouse_client(&state)?;

    let cursor_tuple = if let Some(ref token) = query.cursor {
        let cursor: FailedTransactionsCursor = decode_cursor(token)?;
        if cursor.chain_id != chain_id {
            return Err(ApplicationError::BadRequest(
                "cursor parameters do not match requested failed transactions query".to_owned(),
            ));
        }
        Some((
            cursor.block_number,
            cursor.transaction_index,
            cursor.tx_hash,
        ))
    } else {
        None
    };

    let mut rows = crate::clickhouse::get_failed_transactions(&client, chain_id, limit + 1, cursor_tuple).await?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = if has_more && !rows.is_empty() {
        let last_row = rows.last().unwrap();
        Some(encode_cursor(&FailedTransactionsCursor {
            chain_id,
            block_number: last_row.block_number,
            transaction_index: last_row.transaction_index,
            tx_hash: last_row.tx_hash.clone(),
        })?)
    } else {
        None
    };

    let tx_details: Vec<TransactionDetailResponse> = rows
        .into_iter()
        .map(TransactionDetailResponse::from)
        .collect();

    Ok(response::success_with_meta(
        tx_details,
        json!({
            "chain_id": chain_id,
            "limit": limit,
            "has_more": has_more,
            "next_cursor": next_cursor
        }),
    ))
}

#[utoipa::path(
    get,
    path = "/api/chains/{chain_id}/sync-status",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    responses(
        (status = 200, description = "Sync status", body = ApiResponse<BlockTransactionSyncStateRecord>),
        (status = 404, description = "Sync state not initialized")
    )
)]
pub async fn get_sync_status(
    _principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<BlockTransactionSyncStateRecord>>, ApplicationError> {
    let sync_state = state::get_sync_state(&state.pool, chain_id)
        .await?
        .ok_or_else(|| {
            ApplicationError::NotFound(format!(
                "block-transaction sync state for chain {chain_id} not initialized"
            ))
        })?;

    Ok(response::success(sync_state))
}

#[utoipa::path(
    put,
    path = "/api/chains/{chain_id}/block-transaction-sync",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    request_body = UpdateSyncConfigRequest,
    responses(
        (status = 200, description = "Sync configuration updated", body = ApiResponse<BlockTransactionSyncStateRecord>),
        (status = 403, description = "Admin only")
    )
)]
pub async fn update_sync_config(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
    Json(request): Json<UpdateSyncConfigRequest>,
) -> Result<Json<ApiResponse<BlockTransactionSyncStateRecord>>, ApplicationError> {
    if principal.role != Role::Admin {
        return Err(ApplicationError::Forbidden);
    }
    if chain_id <= 0 {
        return Err(ApplicationError::BadRequest(
            "chain_id must be a positive integer".to_owned(),
        ));
    }

    let record = state::upsert_sync_config(&state.pool, chain_id, &request).await?;
    Ok(response::success(record))
}

#[utoipa::path(
    post,
    path = "/api/chains/{chain_id}/block-transaction-sync/pause",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    responses(
        (status = 200, description = "Sync paused", body = ApiResponse<BlockTransactionSyncStateRecord>),
        (status = 403, description = "Admin only")
    )
)]
pub async fn pause_sync(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<BlockTransactionSyncStateRecord>>, ApplicationError> {
    if principal.role != Role::Admin {
        return Err(ApplicationError::Forbidden);
    }

    let record = state::pause_sync(&state.pool, chain_id).await?;
    Ok(response::success(record))
}

#[utoipa::path(
    post,
    path = "/api/chains/{chain_id}/block-transaction-sync/resume",
    tag = "block_transaction",
    params(
        ("chain_id" = i64, Path, description = "EVM chain id")
    ),
    responses(
        (status = 200, description = "Sync resumed", body = ApiResponse<BlockTransactionSyncStateRecord>),
        (status = 403, description = "Admin only")
    )
)]
pub async fn resume_sync(
    principal: AuthenticatedPrincipal,
    State(state): State<ApplicationState>,
    Path(chain_id): Path<i64>,
) -> Result<Json<ApiResponse<BlockTransactionSyncStateRecord>>, ApplicationError> {
    if principal.role != Role::Admin {
        return Err(ApplicationError::Forbidden);
    }

    let record = state::resume_sync(&state.pool, chain_id).await?;
    Ok(response::success(record))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_block_ref_parses_decimals_hex_and_hashes() {
        assert_eq!(parse_block_ref("12345").unwrap(), BlockRef::Number(12345));
        assert_eq!(parse_block_ref("0x10").unwrap(), BlockRef::Number(16));
        assert_eq!(parse_block_ref("0X10").unwrap(), BlockRef::Number(16));
        assert_eq!(
            parse_block_ref("0x0000000000000000000000000000000000000000000000000000000000000100")
                .unwrap(),
            BlockRef::Hash(
                "0x0000000000000000000000000000000000000000000000000000000000000100".to_owned()
            )
        );

        assert!(parse_block_ref("").is_err());
        assert!(parse_block_ref("-5").is_err());
        assert!(parse_block_ref("notanumber").is_err());
    }

    #[test]
    fn block_transactions_cursor_roundtrip() {
        let cursor = BlockTransactionsCursor {
            chain_id: 1,
            block_number: 100,
            transaction_index: 5,
        };
        let encoded = encode_cursor(&cursor).expect("encodes cursor");
        let decoded: BlockTransactionsCursor = decode_cursor(&encoded).expect("decodes cursor");
        assert_eq!(decoded.chain_id, 1);
        assert_eq!(decoded.block_number, 100);
        assert_eq!(decoded.transaction_index, 5);
    }
}
