use crate::{
    app::application_state::ApplicationState,
    block_transaction::state::{self, BlockTransactionSyncStateRecord},
    shared::error::ApplicationError,
};
use crate::{chains, rpc_pool};

pub async fn collect_once(state: &ApplicationState) -> Result<(), ApplicationError> {
    let limit = (state.configuration.block_transaction.max_concurrency.max(1) * 2) as i64;
    let sync_states = state::runnable_sync_states(&state.pool, limit.max(10)).await?;
    if sync_states.is_empty() {
        return Ok(());
    }

    let concurrency = state.configuration.block_transaction.max_concurrency.max(1) as usize;
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = tokio::task::JoinSet::new();

    for sync_state in sync_states {
        let permit = semaphore.clone().acquire_owned().await.map_err(|_| {
            ApplicationError::Internal("failed to acquire concurrency permit".to_owned())
        })?;
        let state = state.clone();

        tasks.spawn(async move {
            let _permit = permit;
            if let Err(error) = collect_chain(&state, &sync_state).await {
                tracing::warn!(
                    chain_id = sync_state.chain_id,
                    error = %error,
                    "block-transaction collection tick failed for chain"
                );
                let _ = state::mark_sync_error(
                    &state.pool,
                    sync_state.chain_id,
                    &error.public_message(),
                )
                .await;
            }
        });
    }

    while let Some(res) = tasks.join_next().await {
        if let Err(join_err) = res {
            tracing::error!(error = %join_err, "block-transaction collector task panicked");
        }
    }

    Ok(())
}

pub fn partition_block_range_into_slices(
    from_block: i64,
    safe_head: i64,
    endpoints: &[rpc_pool::RpcEndpointRecord],
    default_batch_size: usize,
    is_tip: bool,
) -> Vec<(rpc_pool::RpcEndpointRecord, Vec<i64>)> {
    if from_block > safe_head || endpoints.is_empty() {
        return Vec::new();
    }

    let default_batch = (default_batch_size as i64).max(1);

    if is_tip {
        let ep = &endpoints[0];
        let cap = ep
            .max_batch_size
            .filter(|&s| s > 0)
            .map(|s| s as i64)
            .unwrap_or(default_batch);
        let curr_end = (from_block + cap - 1).min(safe_head);
        return vec![(ep.clone(), (from_block..=curr_end).collect())];
    }

    let mut slices = Vec::with_capacity(endpoints.len());
    let mut curr_start = from_block;
    for ep in endpoints {
        if curr_start > safe_head {
            break;
        }
        let cap = ep
            .max_batch_size
            .filter(|&s| s > 0)
            .map(|s| s as i64)
            .unwrap_or(default_batch);
        let curr_end = (curr_start + cap - 1).min(safe_head);
        slices.push((ep.clone(), (curr_start..=curr_end).collect()));
        curr_start = curr_end + 1;
    }

    slices
}

async fn fetch_blocks_and_receipts_for_endpoint(
    client: &reqwest::Client,
    endpoint: &rpc_pool::RpcEndpointRecord,
    chain_id: i64,
    block_numbers: &[i64],
) -> Result<Vec<rpc_pool::evm_rpc_client::DecodedBlock>, ApplicationError> {
    let chunk_size = match endpoint.max_batch_size {
        Some(limit) if limit > 0 => (limit as usize).min(block_numbers.len()),
        _ => block_numbers.len(),
    };

    let mut all_blocks = Vec::with_capacity(block_numbers.len());
    for chunk in block_numbers.chunks(chunk_size.max(1)) {
        let mut blocks = rpc_pool::evm_rpc_client::eth_get_blocks_by_number_batch(
            client,
            &endpoint.url,
            chain_id,
            chunk,
        )
        .await?;

        let total_txs: usize = blocks.iter().map(|b| b.transactions.len()).sum();
        if total_txs > 0 {
            match rpc_pool::evm_rpc_client::eth_get_block_receipts_batch(
                client,
                &endpoint.url,
                chunk,
            )
            .await
            {
                Ok(Some(receipts_by_block)) => {
                    rpc_pool::evm_rpc_client::attach_receipts_to_blocks(
                        &mut blocks,
                        &receipts_by_block,
                    );
                }
                Ok(None) => {
                    tracing::warn!(
                        chain_id,
                        endpoint_id = %endpoint.id,
                        "RPC endpoint does not support eth_getBlockReceipts; proceeding without transaction receipts"
                    );
                }
                Err(error) => {
                    return Err(error);
                }
            }
        }
        all_blocks.extend(blocks);
    }

    Ok(all_blocks)
}

async fn fetch_slice_with_failover(
    state: &ApplicationState,
    chain_id: i64,
    initial_endpoint: &rpc_pool::RpcEndpointRecord,
    block_numbers: &[i64],
) -> Result<Vec<rpc_pool::evm_rpc_client::DecodedBlock>, ApplicationError> {
    if block_numbers.is_empty() {
        return Ok(Vec::new());
    }

    const MAX_RETRIES: usize = 5;
    let mut last_error = None;
    let mut current_endpoint = initial_endpoint.clone();

    for attempt in 1..=MAX_RETRIES {
        match fetch_blocks_and_receipts_for_endpoint(
            &state.http_client,
            &current_endpoint,
            chain_id,
            block_numbers,
        )
        .await
        {
            Ok(blocks) => {
                let _ = rpc_pool::mark_rpc_success(&state.pool, current_endpoint.id).await;
                return Ok(blocks);
            }
            Err(err) => {
                let err_msg = err.public_message();
                tracing::warn!(
                    chain_id,
                    endpoint_id = %current_endpoint.id,
                    url = %current_endpoint.url,
                    attempt,
                    error = %err_msg,
                    "RPC endpoint failed during slice fetch; triggering failover to another endpoint"
                );
                let _ = rpc_pool::mark_rpc_failure(&state.pool, current_endpoint.id, &err_msg).await;
                last_error = Some(err);

                let req = rpc_pool::EndpointRequirements {
                    needs_archive: false,
                    preferred_block_range: Some(block_numbers.len() as i64),
                };
                match rpc_pool::select_rpc_endpoint_with_requirements(&state.pool, chain_id, &req).await {
                    Ok(next_ep) => {
                        current_endpoint = next_ep;
                    }
                    Err(sel_err) => {
                        tracing::warn!(
                            chain_id,
                            error = %sel_err,
                            "failover could not select new RPC endpoint"
                        );
                    }
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ApplicationError::ExternalService(format!(
            "failed to fetch block slice [{}, {}] after {MAX_RETRIES} attempts",
            block_numbers[0],
            block_numbers[block_numbers.len() - 1]
        ))
    }))
}

async fn get_chain_head_with_failover(
    state: &ApplicationState,
    chain_id: i64,
    req: &rpc_pool::EndpointRequirements,
) -> Result<i64, ApplicationError> {
    const MAX_HEAD_RETRIES: usize = 3;
    let mut last_error = None;
    for attempt in 1..=MAX_HEAD_RETRIES {
        let endpoint = match rpc_pool::select_rpc_endpoint_with_requirements(
            &state.pool,
            chain_id,
            req,
        )
        .await
        {
            Ok(ep) => ep,
            Err(err) => return Err(err),
        };
        match rpc_pool::evm_rpc_client::eth_block_number(&state.http_client, &endpoint.url).await {
            Ok(head) => {
                let _ = rpc_pool::mark_rpc_success(&state.pool, endpoint.id).await;
                return Ok(head);
            }
            Err(error) => {
                let error_message = error.public_message();
                tracing::warn!(
                    chain_id,
                    endpoint_id = %endpoint.id,
                    url = %endpoint.url,
                    attempt,
                    error = %error_message,
                    "failed to fetch eth_blockNumber; failing over"
                );
                let _ = rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await;
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        ApplicationError::ExternalService(
            "failed to query chain head from any RPC endpoint".to_owned(),
        )
    }))
}

async fn get_block_by_number_with_failover(
    state: &ApplicationState,
    chain_id: i64,
    block_number: i64,
    req: &rpc_pool::EndpointRequirements,
) -> Result<Option<rpc_pool::evm_rpc_client::DecodedBlock>, ApplicationError> {
    const MAX_RETRIES: usize = 3;
    let mut last_error = None;
    for attempt in 1..=MAX_RETRIES {
        let endpoint = match rpc_pool::select_rpc_endpoint_with_requirements(
            &state.pool,
            chain_id,
            req,
        )
        .await
        {
            Ok(ep) => ep,
            Err(err) => return Err(err),
        };
        match rpc_pool::evm_rpc_client::eth_get_block_by_number(
            &state.http_client,
            &endpoint.url,
            chain_id,
            block_number,
        )
        .await
        {
            Ok(b) => {
                let _ = rpc_pool::mark_rpc_success(&state.pool, endpoint.id).await;
                return Ok(b);
            }
            Err(error) => {
                let error_message = error.public_message();
                tracing::warn!(
                    chain_id,
                    endpoint_id = %endpoint.id,
                    url = %endpoint.url,
                    attempt,
                    block_number,
                    error = %error_message,
                    "failed to fetch block by number for reorg check; failing over"
                );
                let _ = rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await;
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        ApplicationError::ExternalService(format!(
            "failed to get block {block_number} from any RPC endpoint after {MAX_RETRIES} attempts"
        ))
    }))
}

async fn collect_chain(
    state: &ApplicationState,
    sync_state: &BlockTransactionSyncStateRecord,
) -> Result<(), ApplicationError> {
    if !state.configuration.clickhouse.enabled {
        return Err(ApplicationError::ExternalService(
            "block/transaction collection requires EVENTLAKE_CLICKHOUSE_ENABLED=true".to_owned(),
        ));
    }

    if sync_state.status == "reorg_retrying" {
        let client = match crate::clickhouse::active_client(state).await? {
            Some(c) => c,
            None => {
                return Err(ApplicationError::ExternalService(
                    "ClickHouse client unavailable while retrying reorg".to_owned(),
                ));
            }
        };

        match crate::clickhouse::invalidate_blocks_and_transactions_from_block(
            &client,
            sync_state.chain_id,
            sync_state.next_block,
        )
        .await
        {
            Ok(()) => {
                state::advance_checkpoint(
                    &state.pool,
                    sync_state.chain_id,
                    sync_state.next_block,
                    sync_state.safe_head,
                    sync_state.latest_seen_block,
                    "syncing",
                )
                .await?;
                tracing::info!(
                    chain_id = sync_state.chain_id,
                    from_block = sync_state.next_block,
                    "ClickHouse block-transaction reorg tombstones applied; resuming sync"
                );
            }
            Err(error) => {
                tracing::error!(
                    chain_id = sync_state.chain_id,
                    from_block = sync_state.next_block,
                    error = %error,
                    "ClickHouse block-transaction reorg tombstone retry failed"
                );
                return Ok(());
            }
        }
        return Ok(());
    }

    let policy = chains::get_collection_policy(&state.pool, sync_state.chain_id).await?;
    let req = rpc_pool::EndpointRequirements {
        needs_archive: false,
        preferred_block_range: Some(sync_state.batch_size as i64),
    };

    let chain_head = get_chain_head_with_failover(state, sync_state.chain_id, &req).await?;
    let safe_head = chain_head.saturating_sub(policy.safe_confirmation_depth);

    if sync_state.next_block > safe_head {
        let status = if sync_state.realtime_enabled {
            "caught_up"
        } else {
            "syncing"
        };
        state::advance_checkpoint(
            &state.pool,
            sync_state.chain_id,
            sync_state.next_block,
            Some(safe_head),
            Some(chain_head),
            status,
        )
        .await?;
        return Ok(());
    }

    let client = match crate::clickhouse::active_client(state).await? {
        Some(c) => c,
        None => {
            return Err(ApplicationError::ExternalService(
                "ClickHouse client unavailable for block-transaction collection".to_owned(),
            ));
        }
    };

    let from_block = sync_state.next_block;
    if sync_state.reorg_window > 0 && from_block > sync_state.start_block {
        let check_height = from_block - 1;
        if let Ok(Some(existing_prev)) =
            crate::clickhouse::get_block_by_number(&client, sync_state.chain_id, check_height)
                .await
        {
            let rpc_prev = get_block_by_number_with_failover(
                state,
                sync_state.chain_id,
                check_height,
                &req,
            )
            .await?;

            if let Some(rpc_prev) = rpc_prev {
                if rpc_prev.block_hash != existing_prev.block_hash {
                    tracing::warn!(
                        chain_id = sync_state.chain_id,
                        height = check_height,
                        stored_hash = %existing_prev.block_hash,
                        rpc_hash = %rpc_prev.block_hash,
                        "block reorg detected; invalidating stale blocks/transactions"
                    );

                    let tombstone_res =
                        crate::clickhouse::invalidate_blocks_and_transactions_from_block(
                            &client,
                            sync_state.chain_id,
                            check_height,
                        )
                        .await;

                    if let Err(error) = tombstone_res {
                        state::rewind_checkpoint_for_reorg(
                            &state.pool,
                            sync_state.chain_id,
                            check_height,
                            &error.to_string(),
                        )
                        .await?;
                        return Ok(());
                    }

                    state::rewind_checkpoint_for_reorg(
                        &state.pool,
                        sync_state.chain_id,
                        check_height,
                        "reorg detected",
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    let endpoints = rpc_pool::get_available_rpc_endpoints_with_requirements(
        &state.pool,
        sync_state.chain_id,
        &req,
    )
    .await?;

    let is_tip = sync_state.status == "caught_up"
        || safe_head.saturating_sub(from_block).saturating_add(1) <= (sync_state.batch_size as i64);

    let slices = partition_block_range_into_slices(
        from_block,
        safe_head,
        &endpoints,
        sync_state.batch_size as usize,
        is_tip,
    );

    if slices.is_empty() {
        return Ok(());
    }

    let actual_slices_count = slices.len();
    let target_from = slices[0].1[0];
    let target_to = slices[actual_slices_count - 1]
        .1
        .last()
        .copied()
        .unwrap_or(target_from);

    let mut slice_results: Vec<Option<Vec<rpc_pool::evm_rpc_client::DecodedBlock>>> =
        vec![None; actual_slices_count];
    let mut first_error: Option<ApplicationError> = None;

    let mut join_set = tokio::task::JoinSet::new();
    for (idx, (endpoint, slice_blocks)) in slices.into_iter().enumerate() {
        let state = state.clone();
        let chain_id = sync_state.chain_id;
        join_set.spawn(async move {
            let blocks =
                fetch_slice_with_failover(&state, chain_id, &endpoint, &slice_blocks).await?;
            Ok::<(usize, Vec<rpc_pool::evm_rpc_client::DecodedBlock>), ApplicationError>((
                idx, blocks,
            ))
        });
    }

    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Ok((idx, blocks))) => {
                slice_results[idx] = Some(blocks);
            }
            Ok(Err(err)) => {
                if first_error.is_none() {
                    first_error = Some(err);
                }
            }
            Err(join_err) => {
                if first_error.is_none() {
                    first_error = Some(ApplicationError::Internal(format!(
                        "slice task panicked: {join_err}"
                    )));
                }
            }
        }
    }

    if let Some(err) = first_error {
        return Err(err);
    }

    let mut all_blocks = Vec::new();
    for (idx, res) in slice_results.into_iter().enumerate() {
        match res {
            Some(blocks) => all_blocks.extend(blocks),
            None => {
                return Err(ApplicationError::Internal(format!(
                    "slice {idx} result is missing after JoinSet completion"
                )));
            }
        }
    }

    if all_blocks.is_empty() {
        return Ok(());
    }

    // Sequence integrity verification across all slices
    rpc_pool::evm_rpc_client::validate_block_sequence(&all_blocks)?;

    let total_txs: usize = all_blocks.iter().map(|b| b.transactions.len()).sum();

    if let Err(error) =
        crate::clickhouse::write_blocks_and_transactions(&client, &all_blocks).await
    {
        state.clear_clickhouse_client();
        return Err(ApplicationError::ExternalService(format!(
            "ClickHouse block-transaction write failed: {error}"
        )));
    }

    let next_block = target_to + 1;
    let status = if next_block > safe_head {
        "caught_up"
    } else {
        "syncing"
    };

    state::advance_checkpoint(
        &state.pool,
        sync_state.chain_id,
        next_block,
        Some(safe_head),
        Some(chain_head),
        status,
    )
    .await?;

    tracing::info!(
        chain_id = sync_state.chain_id,
        from_block = target_from,
        to_block = target_to,
        slices_count = actual_slices_count,
        block_count = all_blocks.len(),
        tx_count = total_txs,
        "collected block and transaction batch"
    );

    Ok(())
}
