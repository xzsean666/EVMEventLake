use chrono::Utc;
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{as_u32, as_u64, execute_reorg_tombstone, to_offset_datetime, write_rows};
use crate::{
    rpc_pool::evm_rpc_client::DecodedBlock,
    shared::{
        error::ApplicationError,
        validation::{normalize_address, normalize_hash},
    },
};

#[derive(Row, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BlockRow {
    pub chain_id: u64,
    pub block_number: u64,
    pub block_hash: String,
    pub parent_hash: String,
    pub timestamp: u64,
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
    pub transaction_count: u32,
    pub is_canonical: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub stored_at: OffsetDateTime,
}

#[derive(Row, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TransactionRow {
    pub chain_id: u64,
    pub tx_hash: String,
    pub block_number: u64,
    pub transaction_index: u32,
    pub from_address: String,
    pub to_address: Option<String>,
    pub value: String,
    pub nonce: String,
    pub gas: String,
    pub gas_price: Option<String>,
    pub max_fee_per_gas: Option<String>,
    pub max_priority_fee_per_gas: Option<String>,
    pub tx_type: Option<u32>,
    pub method_id: Option<String>,
    pub is_canonical: bool,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub stored_at: OffsetDateTime,
}

pub async fn write_blocks_and_transactions(
    client: &Client,
    blocks: &[DecodedBlock],
) -> anyhow::Result<()> {
    if blocks.is_empty() {
        return Ok(());
    }

    let stored_at = Utc::now();
    let stored_at_offset = to_offset_datetime(stored_at)?;

    let mut block_rows = Vec::with_capacity(blocks.len());
    let mut tx_rows = Vec::new();

    for block in blocks {
        let chain_id = as_u64(block.chain_id, "chain_id")?;
        let block_number = as_u64(block.block_number, "block_number")?;
        let timestamp = as_u64(block.timestamp, "timestamp")?;
        let transaction_count = as_u32(block.transaction_count, "transaction_count")?;

        block_rows.push(BlockRow {
            chain_id,
            block_number,
            block_hash: block.block_hash.clone(),
            parent_hash: block.parent_hash.clone(),
            timestamp,
            gas_limit: block.gas_limit.clone(),
            gas_used: block.gas_used.clone(),
            base_fee_per_gas: block.base_fee_per_gas.clone(),
            beneficiary: block.beneficiary.clone(),
            transactions_root: block.transactions_root.clone(),
            receipts_root: block.receipts_root.clone(),
            state_root: block.state_root.clone(),
            size: block.size.clone(),
            withdrawals_root: block.withdrawals_root.clone(),
            blob_gas_used: block.blob_gas_used.clone(),
            excess_blob_gas: block.excess_blob_gas.clone(),
            parent_beacon_block_root: block.parent_beacon_block_root.clone(),
            transaction_count,
            is_canonical: true,
            stored_at: stored_at_offset,
        });

        for tx in &block.transactions {
            let tx_chain_id = as_u64(tx.chain_id, "chain_id")?;
            let tx_block_number = as_u64(tx.block_number, "block_number")?;
            let tx_index = as_u32(tx.transaction_index, "transaction_index")?;
            let tx_type = tx.tx_type.map(|t| as_u32(t, "tx_type")).transpose()?;

            tx_rows.push(TransactionRow {
                chain_id: tx_chain_id,
                tx_hash: tx.tx_hash.clone(),
                block_number: tx_block_number,
                transaction_index: tx_index,
                from_address: tx.from_address.clone(),
                to_address: tx.to_address.clone(),
                value: tx.value.clone(),
                nonce: tx.nonce.clone(),
                gas: tx.gas.clone(),
                gas_price: tx.gas_price.clone(),
                max_fee_per_gas: tx.max_fee_per_gas.clone(),
                max_priority_fee_per_gas: tx.max_priority_fee_per_gas.clone(),
                tx_type,
                method_id: tx.method_id.clone(),
                is_canonical: true,
                stored_at: stored_at_offset,
            });
        }
    }

    write_rows(client, "blocks", &block_rows).await?;
    if !tx_rows.is_empty() {
        write_rows(client, "transactions", &tx_rows).await?;
    }

    Ok(())
}

pub async fn invalidate_blocks_and_transactions_from_block(
    client: &Client,
    chain_id: i64,
    from_block: i64,
) -> anyhow::Result<()> {
    let chain_id = as_u64(chain_id, "chain_id")?;
    let from_block = as_u64(from_block, "from_block")?;

    execute_reorg_tombstone(
        client,
        r#"
        INSERT INTO blocks
        SELECT chain_id, block_number, block_hash, parent_hash, timestamp,
               gas_limit, gas_used, base_fee_per_gas, beneficiary,
               transactions_root, receipts_root, state_root, size,
               withdrawals_root, blob_gas_used, excess_blob_gas,
               parent_beacon_block_root, transaction_count, false, now64(3)
        FROM blocks FINAL
        WHERE chain_id = ? AND block_number >= ? AND is_canonical = true
        "#,
        chain_id,
        from_block,
    )
    .await?;

    execute_reorg_tombstone(
        client,
        r#"
        INSERT INTO transactions
        SELECT chain_id, tx_hash, block_number, transaction_index,
               from_address, to_address, value, nonce, gas, gas_price,
               max_fee_per_gas, max_priority_fee_per_gas, tx_type, method_id,
               false, now64(3)
        FROM transactions FINAL
        WHERE chain_id = ? AND block_number >= ? AND is_canonical = true
        "#,
        chain_id,
        from_block,
    )
    .await?;

    Ok(())
}

pub async fn get_block_by_number(
    client: &Client,
    chain_id: i64,
    block_number: i64,
) -> Result<Option<BlockRow>, ApplicationError> {
    let chain_id = as_u64(chain_id, "chain_id")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
    let block_number = as_u64(block_number, "block_number")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;

    let row = client
        .query(
            r#"
            SELECT chain_id, block_number, block_hash, parent_hash, timestamp,
                   gas_limit, gas_used, base_fee_per_gas, beneficiary,
                   transactions_root, receipts_root, state_root, size,
                   withdrawals_root, blob_gas_used, excess_blob_gas,
                   parent_beacon_block_root, transaction_count, is_canonical, stored_at
            FROM blocks FINAL
            WHERE chain_id = ? AND block_number = ? AND is_canonical = true
            LIMIT 1
            "#,
        )
        .bind(chain_id)
        .bind(block_number)
        .fetch_optional::<BlockRow>()
        .await
        .map_err(|err| {
            ApplicationError::ExternalService(format!("ClickHouse query failed: {err}"))
        })?;

    Ok(row)
}

pub async fn get_block_by_hash(
    client: &Client,
    chain_id: i64,
    block_hash: &str,
) -> Result<Option<BlockRow>, ApplicationError> {
    let chain_id = as_u64(chain_id, "chain_id")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
    let block_hash = normalize_hash(block_hash)?;

    let row = client
        .query(
            r#"
            SELECT chain_id, block_number, block_hash, parent_hash, timestamp,
                   gas_limit, gas_used, base_fee_per_gas, beneficiary,
                   transactions_root, receipts_root, state_root, size,
                   withdrawals_root, blob_gas_used, excess_blob_gas,
                   parent_beacon_block_root, transaction_count, is_canonical, stored_at
            FROM blocks FINAL
            WHERE chain_id = ? AND block_hash = ? AND is_canonical = true
            LIMIT 1
            "#,
        )
        .bind(chain_id)
        .bind(block_hash)
        .fetch_optional::<BlockRow>()
        .await
        .map_err(|err| {
            ApplicationError::ExternalService(format!("ClickHouse query failed: {err}"))
        })?;

    Ok(row)
}

pub async fn get_block_transactions(
    client: &Client,
    chain_id: i64,
    block_number: i64,
    limit: u64,
    cursor_index: Option<u32>,
) -> Result<Vec<TransactionRow>, ApplicationError> {
    let chain_id = as_u64(chain_id, "chain_id")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
    let block_number = as_u64(block_number, "block_number")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;

    let mut query_str = String::from(
        r#"
        SELECT chain_id, tx_hash, block_number, transaction_index,
               from_address, to_address, value, nonce, gas, gas_price,
               max_fee_per_gas, max_priority_fee_per_gas, tx_type, method_id,
               is_canonical, stored_at
        FROM transactions FINAL
        WHERE chain_id = ? AND block_number = ? AND is_canonical = true
        "#,
    );

    if cursor_index.is_some() {
        query_str.push_str(" AND transaction_index >= ?");
    }
    query_str.push_str(" ORDER BY transaction_index ASC LIMIT ?");

    let mut query = client.query(&query_str).bind(chain_id).bind(block_number);
    if let Some(idx) = cursor_index {
        query = query.bind(idx);
    }
    query = query.bind(limit);

    let rows = query.fetch_all::<TransactionRow>().await.map_err(|err| {
        ApplicationError::ExternalService(format!("ClickHouse query failed: {err}"))
    })?;

    Ok(rows)
}

pub async fn get_transaction_by_hash(
    client: &Client,
    chain_id: i64,
    tx_hash: &str,
) -> Result<Option<TransactionRow>, ApplicationError> {
    let chain_id = as_u64(chain_id, "chain_id")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
    let tx_hash = normalize_hash(tx_hash)?;

    let row = client
        .query(
            r#"
            SELECT chain_id, tx_hash, block_number, transaction_index,
               from_address, to_address, value, nonce, gas, gas_price,
               max_fee_per_gas, max_priority_fee_per_gas, tx_type, method_id,
               is_canonical, stored_at
            FROM transactions FINAL
            WHERE chain_id = ? AND tx_hash = ? AND is_canonical = true
            LIMIT 1
            "#,
        )
        .bind(chain_id)
        .bind(tx_hash)
        .fetch_optional::<TransactionRow>()
        .await
        .map_err(|err| {
            ApplicationError::ExternalService(format!("ClickHouse query failed: {err}"))
        })?;

    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn get_address_transactions(
    client: &Client,
    chain_id: i64,
    address: &str,
    direction: &str,
    from_block: Option<i64>,
    to_block: Option<i64>,
    limit: u64,
    cursor: Option<(u64, u32, String)>,
) -> Result<Vec<TransactionRow>, ApplicationError> {
    let chain_id = as_u64(chain_id, "chain_id")
        .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
    let address = normalize_address(address)?;

    let mut query_str = String::from(
        r#"
        SELECT chain_id, tx_hash, block_number, transaction_index,
               from_address, to_address, value, nonce, gas, gas_price,
               max_fee_per_gas, max_priority_fee_per_gas, tx_type, method_id,
               is_canonical, stored_at
        FROM transactions FINAL
        WHERE chain_id = ? AND is_canonical = true
        "#,
    );

    match direction {
        "from" => {
            query_str.push_str(" AND from_address = ?");
        }
        "to" => {
            query_str.push_str(" AND to_address = ?");
        }
        _ => {
            query_str.push_str(" AND (from_address = ? OR to_address = ?)");
        }
    }

    if from_block.is_some() {
        query_str.push_str(" AND block_number >= ?");
    }
    if to_block.is_some() {
        query_str.push_str(" AND block_number <= ?");
    }

    if cursor.is_some() {
        query_str.push_str(" AND ((block_number < ?) OR (block_number = ? AND transaction_index < ?) OR (block_number = ? AND transaction_index = ? AND tx_hash < ?))");
    }

    query_str.push_str(" ORDER BY block_number DESC, transaction_index DESC, tx_hash DESC LIMIT ?");

    let mut query = client.query(&query_str).bind(chain_id);

    match direction {
        "from" => {
            query = query.bind(&address);
        }
        "to" => {
            query = query.bind(&address);
        }
        _ => {
            query = query.bind(&address).bind(&address);
        }
    }

    if let Some(fb) = from_block {
        let u_fb = as_u64(fb, "from_block")
            .map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
        query = query.bind(u_fb);
    }
    if let Some(tb) = to_block {
        let u_tb =
            as_u64(tb, "to_block").map_err(|err| ApplicationError::BadRequest(err.to_string()))?;
        query = query.bind(u_tb);
    }

    if let Some((cur_block, cur_idx, cur_hash)) = cursor {
        query = query
            .bind(cur_block)
            .bind(cur_block)
            .bind(cur_idx)
            .bind(cur_block)
            .bind(cur_idx)
            .bind(cur_hash);
    }

    query = query.bind(limit);

    let rows = query.fetch_all::<TransactionRow>().await.map_err(|err| {
        ApplicationError::ExternalService(format!("ClickHouse query failed: {err}"))
    })?;

    Ok(rows)
}
