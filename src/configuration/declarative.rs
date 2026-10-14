use std::path::Path;
use serde::Deserialize;
use sqlx::SqlitePool;

use crate::rpc_pool::RpcEndpointSeed;

#[derive(Debug, Clone, Deserialize)]
pub struct DeclarativeConfigFile {
    pub version: Option<String>,
    #[serde(default)]
    pub chains: Vec<DeclarativeChainSeed>,
    #[serde(default)]
    pub rpc_endpoints: Vec<RpcEndpointSeed>,
    #[serde(default)]
    pub subscriptions: Vec<DeclarativeSubscriptionSeed>,
    #[serde(default)]
    pub block_transaction_sync: Vec<DeclarativeBlockSyncSeed>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeclarativeChainSeed {
    pub chain_id: i64,
    pub name: String,
    pub native_token_symbol: Option<String>,
    pub safe_confirmation_depth: Option<i64>,
    pub default_min_block_window: Option<i64>,
    pub default_max_block_window: Option<i64>,
    pub rpc_notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeclarativeSubscriptionSeed {
    pub chain_id: i64,
    pub collection_scope: Option<String>,
    pub contract_address: Option<String>,
    pub start_block: i64,
    pub realtime_enabled: Option<bool>,
    pub min_block_window: Option<i64>,
    pub max_block_window: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeclarativeBlockSyncSeed {
    pub chain_id: i64,
    pub enabled: Option<bool>,
    pub start_block: Option<i64>,
    pub batch_size: Option<i32>,
    pub reorg_window: Option<i32>,
    pub realtime_enabled: Option<bool>,
}

pub async fn seed_declarative_config_from_file(
    pool: &SqlitePool,
    path: &str,
) -> anyhow::Result<()> {
    if !Path::new(path).is_file() {
        tracing::info!(path = %path, "declarative config file not found, skipping");
        return Ok(());
    }

    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| anyhow::anyhow!("failed to read declarative config at {path}: {e}"))?;

    seed_declarative_config_from_json(pool, &content).await
}

pub async fn seed_declarative_config_from_json(
    pool: &SqlitePool,
    json_str: &str,
) -> anyhow::Result<()> {
    let config: DeclarativeConfigFile = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(err) => {
            anyhow::bail!("failed to parse declarative config JSON: {err}");
        }
    };

    // 1. Chains
    for chain in &config.chains {
        let symbol = chain.native_token_symbol.as_deref().unwrap_or("ETH");
        let safe_depth = chain.safe_confirmation_depth.unwrap_or(12);
        let min_w = chain.default_min_block_window.unwrap_or(1);
        let max_w = chain.default_max_block_window.unwrap_or(1000);

        sqlx::query(
            r#"
            INSERT INTO eventlake_chains (
                chain_id, name, native_token_symbol, safe_confirmation_depth,
                default_min_block_window, default_max_block_window, rpc_notes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (chain_id) DO UPDATE SET
                name = EXCLUDED.name,
                native_token_symbol = EXCLUDED.native_token_symbol,
                safe_confirmation_depth = EXCLUDED.safe_confirmation_depth,
                default_min_block_window = EXCLUDED.default_min_block_window,
                default_max_block_window = EXCLUDED.default_max_block_window,
                rpc_notes = EXCLUDED.rpc_notes,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(chain.chain_id)
        .bind(&chain.name)
        .bind(symbol)
        .bind(safe_depth)
        .bind(min_w)
        .bind(max_w)
        .bind(&chain.rpc_notes)
        .execute(pool)
        .await?;
    }

    // 2. RPC Endpoints
    if !config.rpc_endpoints.is_empty() {
        let rpcs_json = serde_json::to_string(&config.rpc_endpoints)?;
        crate::rpc_pool::seed_rpc_endpoints_from_json(pool, &rpcs_json).await?;
    }

    // 3. Subscriptions (checkpoints preserved!)
    for sub in &config.subscriptions {
        let scope = sub.collection_scope.as_deref().unwrap_or("contract");
        let contract_addr = sub.contract_address.as_deref().map(|a| a.to_lowercase());
        let realtime = sub.realtime_enabled.unwrap_or(true);
        let min_w = sub.min_block_window.unwrap_or(1);
        let max_w = sub.max_block_window.unwrap_or(500);

        let existing = sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT id FROM eventlake_subscriptions WHERE chain_id = $1 AND contract_address = $2 AND active = 1",
        )
        .bind(sub.chain_id)
        .bind(&contract_addr)
        .fetch_optional(pool)
        .await?;

        if let Some(existing_id) = existing {
            sqlx::query(
                r#"
                UPDATE eventlake_subscriptions
                SET min_block_window = $2,
                    max_block_window = $3,
                    current_block_window = MIN(current_block_window, $3),
                    realtime_enabled = $4,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = $1
                "#,
            )
            .bind(existing_id)
            .bind(min_w)
            .bind(max_w)
            .bind(realtime)
            .execute(pool)
            .await?;
        } else {
            let id = uuid::Uuid::new_v4();
            sqlx::query(
                r#"
                INSERT INTO eventlake_subscriptions (
                    id, chain_id, contract_address, collection_scope, start_block, current_block,
                    min_block_window, max_block_window, current_block_window, realtime_enabled
                )
                VALUES ($1, $2, $3, $4, $5, $5, $6, $7, $7, $8)
                "#,
            )
            .bind(id)
            .bind(sub.chain_id)
            .bind(&contract_addr)
            .bind(scope)
            .bind(sub.start_block)
            .bind(min_w)
            .bind(max_w)
            .bind(realtime)
            .execute(pool)
            .await?;
        }
    }

    // 4. Block & Transaction Sync
    for bsync in &config.block_transaction_sync {
        let is_enabled = bsync.enabled.unwrap_or(true);
        let start_block = bsync.start_block.unwrap_or(0);
        let batch_size = bsync.batch_size.unwrap_or(10);
        let reorg_window = bsync.reorg_window.unwrap_or(32);
        let realtime = bsync.realtime_enabled.unwrap_or(true);

        if is_enabled {
            sqlx::query(
                r#"
                INSERT INTO eventlake_block_transaction_sync_state (
                    chain_id, next_block, start_block, status, realtime_enabled,
                    batch_size, max_concurrency, reorg_window, updated_at
                )
                VALUES ($1, $2, $2, 'pending', $3, $4, 2, $5, CURRENT_TIMESTAMP)
                ON CONFLICT (chain_id) DO UPDATE SET
                    status = CASE WHEN eventlake_block_transaction_sync_state.status = 'paused' THEN 'pending' ELSE eventlake_block_transaction_sync_state.status END,
                    realtime_enabled = $3,
                    batch_size = $4,
                    reorg_window = $5,
                    updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(bsync.chain_id)
            .bind(start_block)
            .bind(realtime)
            .bind(batch_size)
            .bind(reorg_window)
            .execute(pool)
            .await?;
            tracing::info!(chain_id = bsync.chain_id, "block & transaction sync configured (enabled)");
        } else {
            sqlx::query(
                r#"
                INSERT INTO eventlake_block_transaction_sync_state (
                    chain_id, next_block, start_block, status, realtime_enabled,
                    batch_size, max_concurrency, reorg_window, updated_at
                )
                VALUES ($1, $2, $2, 'paused', $3, $4, 2, $5, CURRENT_TIMESTAMP)
                ON CONFLICT (chain_id) DO UPDATE SET
                    status = 'paused',
                    realtime_enabled = $3,
                    updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(bsync.chain_id)
            .bind(start_block)
            .bind(realtime)
            .bind(batch_size)
            .bind(reorg_window)
            .execute(pool)
            .await?;
            tracing::info!(chain_id = bsync.chain_id, "block & transaction sync configured (disabled/paused)");
        }
    }

    tracing::info!(
        chains = config.chains.len(),
        rpc_endpoints = config.rpc_endpoints.len(),
        subscriptions = config.subscriptions.len(),
        block_sync = config.block_transaction_sync.len(),
        "declarative configuration applied successfully"
    );

    Ok(())
}
