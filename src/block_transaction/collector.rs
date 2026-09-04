use crate::{
    app::application_state::ApplicationState,
    block_transaction::state::{self, BlockTransactionSyncStateRecord},
    shared::error::ApplicationError,
};
#[cfg(feature = "clickhouse")]
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

async fn collect_chain(
    state: &ApplicationState,
    sync_state: &BlockTransactionSyncStateRecord,
) -> Result<(), ApplicationError> {
    #[cfg(not(feature = "clickhouse"))]
    {
        let _ = state;
        let _ = sync_state;
        return Err(ApplicationError::ExternalService(
            "block/transaction collection requires binary built with --features clickhouse"
                .to_owned(),
        ));
    }

    #[cfg(feature = "clickhouse")]
    {
        if !state.configuration.clickhouse.enabled {
            return Err(ApplicationError::ExternalService(
                "block/transaction collection requires EVENTLAKE_CLICKHOUSE_ENABLED=true"
                    .to_owned(),
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
        let endpoint = rpc_pool::select_rpc_endpoint(&state.pool, sync_state.chain_id).await?;
        let chain_head_res =
            rpc_pool::evm_rpc_client::eth_block_number(&state.http_client, &endpoint.url).await;

        let chain_head = match chain_head_res {
            Ok(h) => h,
            Err(error) => {
                let error_message = error.public_message();
                let _ = rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await;
                return Err(error);
            }
        };

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

        let from_block = sync_state.next_block;
        let batch_size = sync_state.batch_size as i64;
        let to_block = from_block
            .saturating_add(batch_size)
            .saturating_sub(1)
            .min(safe_head);
        let block_numbers: Vec<i64> = (from_block..=to_block).collect();

        let client = match crate::clickhouse::active_client(state).await? {
            Some(c) => c,
            None => {
                return Err(ApplicationError::ExternalService(
                    "ClickHouse client unavailable for block-transaction collection".to_owned(),
                ));
            }
        };

        if sync_state.reorg_window > 0 && from_block > sync_state.start_block {
            let check_height = from_block - 1;
            if let Ok(Some(existing_prev)) =
                crate::clickhouse::get_block_by_number(&client, sync_state.chain_id, check_height)
                    .await
            {
                let rpc_prev_res = rpc_pool::evm_rpc_client::eth_get_block_by_number(
                    &state.http_client,
                    &endpoint.url,
                    sync_state.chain_id,
                    check_height,
                )
                .await;

                match rpc_prev_res {
                    Ok(Some(rpc_prev)) => {
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
                    Ok(None) => {}
                    Err(error) => {
                        let error_message = error.public_message();
                        let _ =
                            rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message)
                                .await;
                        return Err(error);
                    }
                }
            }
        }

        let blocks_res = rpc_pool::evm_rpc_client::eth_get_blocks_by_number_batch(
            &state.http_client,
            &endpoint.url,
            sync_state.chain_id,
            &block_numbers,
        )
        .await;

        let blocks = match blocks_res {
            Ok(b) => b,
            Err(error) => {
                let error_message = error.public_message();
                let _ = rpc_pool::mark_rpc_failure(&state.pool, endpoint.id, &error_message).await;
                return Err(error);
            }
        };

        if let Err(error) = crate::clickhouse::write_blocks_and_transactions(&client, &blocks).await
        {
            state.clear_clickhouse_client();
            return Err(ApplicationError::ExternalService(format!(
                "ClickHouse block-transaction write failed: {error}"
            )));
        }

        let next_block = to_block + 1;
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
            from_block,
            to_block,
            block_count = blocks.len(),
            tx_count = blocks.iter().map(|b| b.transactions.len()).sum::<usize>(),
            endpoint_id = %endpoint.id,
            "collected block and transaction batch"
        );

        Ok(())
    }
}
