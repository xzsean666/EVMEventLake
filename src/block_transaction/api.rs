use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
#[cfg(feature = "clickhouse")]
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
            "/api/chains/{chain_id}/transactions/{tx_hash}",
            get(get_transaction),
        )
        .route(
            "/api/chains/{chain_id}/addresses/{address}/transactions",
            get(get_address_transactions),
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
        get_sync_status,
        update_sync_config,
        pause_sync,
        resume_sync
    ),
    components(schemas(
        BlockDetailResponse,
        TransactionDetailResponse,
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
}

#[cfg(feature = "clickhouse")]
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

#[cfg(feature = "clickhouse")]
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

#[cfg(feature = "clickhouse")]
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

#[cfg(not(feature = "clickhouse"))]
fn require_clickhouse_client(_state: &ApplicationState) -> Result<(), ApplicationError> {
    Err(ApplicationError::ServiceUnavailable(
        "block transaction storage unavailable: binary compiled without clickhouse feature"
            .to_owned(),
    ))
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

    #[cfg(not(feature = "clickhouse"))]
    {
        let _ = (chain_id, block_ref, parsed_ref);
        require_clickhouse_client(&state)?;
        unreachable!();
    }

    #[cfg(feature = "clickhouse")]
    {
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

    #[cfg(not(feature = "clickhouse"))]
    {
        let _ = (chain_id, block_ref, parsed_ref, limit, query);
        require_clickhouse_client(&state)?;
        unreachable!();
    }

    #[cfg(feature = "clickhouse")]
    {
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

    #[cfg(not(feature = "clickhouse"))]
    {
        let _ = (chain_id, tx_hash, normalized_hash);
        require_clickhouse_client(&state)?;
        unreachable!();
    }

    #[cfg(feature = "clickhouse")]
    {
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

    #[cfg(not(feature = "clickhouse"))]
    {
        let _ = (
            chain_id,
            address,
            normalized_address,
            direction,
            limit,
            query,
        );
        require_clickhouse_client(&state)?;
        unreachable!();
    }

    #[cfg(feature = "clickhouse")]
    {
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
