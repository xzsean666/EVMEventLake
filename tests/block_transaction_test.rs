use std::collections::HashMap;

use eventlake::rpc_pool::evm_rpc_client::{
    DecodedBlock, DecodedReceipt, DecodedTransaction, RpcReceipt, attach_receipts_to_blocks,
    decode_rpc_receipt, parse_batch_receipts_response, parse_quantity_to_dec,
};
use serde_json::json;

#[test]
fn test_parse_quantity_to_dec_variations() {
    assert_eq!(parse_quantity_to_dec("0x5208").unwrap(), "21000");
    assert_eq!(parse_quantity_to_dec("0x0").unwrap(), "0");
    assert_eq!(parse_quantity_to_dec("0X10").unwrap(), "16");
    assert_eq!(parse_quantity_to_dec("21000").unwrap(), "21000");
    assert_eq!(parse_quantity_to_dec("0").unwrap(), "0");
    assert_eq!(
        parse_quantity_to_dec("0x3b9aca00").unwrap(),
        "1000000000"
    );
    assert_eq!(parse_quantity_to_dec("0x1234").unwrap(), "4660");

    assert!(parse_quantity_to_dec("").is_err());
    assert!(parse_quantity_to_dec("0x").is_err());
    assert!(parse_quantity_to_dec("0xz").is_err());
}

#[test]
fn test_decode_rpc_receipt_standard_and_op_stack() {
    // 1. Standard EVM receipt with hex status "0x1" and OP Stack l1Fee
    let rpc_json = json!({
        "transactionHash": "0x111102cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001",
        "transactionIndex": "0x2",
        "blockNumber": "0x1b4",
        "status": "0x1",
        "gasUsed": "0x5208",
        "effectiveGasPrice": "0x3b9aca00",
        "l1Fee": "0x1234"
    });

    let receipt: RpcReceipt = serde_json::from_value(rpc_json).expect("deserializes");
    let decoded = decode_rpc_receipt(receipt).expect("decodes");

    assert_eq!(
        decoded.transaction_hash,
        "0x111102cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001"
    );
    assert_eq!(decoded.transaction_index, Some(2));
    assert_eq!(decoded.block_number, Some(436));
    assert_eq!(decoded.status, Some(1));
    assert_eq!(decoded.gas_used, Some("21000".to_owned()));
    assert_eq!(decoded.effective_gas_price, Some("1000000000".to_owned()));
    assert_eq!(decoded.l1_fee, Some("4660".to_owned()));

    // 2. Receipt with integer status and numeric gasUsed / l1Fee
    let rpc_json2 = json!({
        "transactionHash": "0x222202cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6002",
        "transactionIndex": "0x0",
        "blockNumber": "0x1b4",
        "status": 0,
        "gasUsed": 50000,
        "effectiveGasPrice": "2000000000",
        "l1_fee": 1000
    });

    let receipt2: RpcReceipt = serde_json::from_value(rpc_json2).expect("deserializes");
    let decoded2 = decode_rpc_receipt(receipt2).expect("decodes");

    assert_eq!(decoded2.status, Some(0));
    assert_eq!(decoded2.gas_used, Some("50000".to_owned()));
    assert_eq!(decoded2.effective_gas_price, Some("2000000000".to_owned()));
    assert_eq!(decoded2.l1_fee, Some("1000".to_owned()));

    // 3. Receipt without status (pre-Byzantium or legacy) and without l1Fee (L1 chain)
    let rpc_json3 = json!({
        "transactionHash": "0x333302cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6003"
    });
    let receipt3: RpcReceipt = serde_json::from_value(rpc_json3).expect("deserializes");
    let decoded3 = decode_rpc_receipt(receipt3).expect("decodes");
    assert_eq!(decoded3.status, None);
    assert_eq!(decoded3.gas_used, None);
    assert_eq!(decoded3.effective_gas_price, None);
    assert_eq!(decoded3.l1_fee, None);
}

#[test]
fn test_parse_batch_receipts_response_ordering_and_missing() {
    let raw = json!([
        {
            "id": 502,
            "result": [
                {
                    "transactionHash": "0x502002cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001",
                    "transactionIndex": "0x0",
                    "status": "0x1",
                    "gasUsed": "0x5208"
                }
            ]
        },
        {
            "id": 500,
            "result": []
        },
        {
            "id": 501,
            "result": [
                {
                    "transactionHash": "0x501002cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001",
                    "transactionIndex": "0x0",
                    "status": "0x0",
                    "gasUsed": "0x186a0"
                }
            ]
        }
    ]);

    let blocks = [500, 501, 502];
    let map = parse_batch_receipts_response(&blocks, raw)
        .expect("parses batch")
        .expect("is Some");

    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&500).unwrap().len(), 0);
    assert_eq!(map.get(&501).unwrap().len(), 1);
    assert_eq!(map.get(&501).unwrap()[0].status, Some(0));
    assert_eq!(map.get(&502).unwrap().len(), 1);
    assert_eq!(map.get(&502).unwrap()[0].status, Some(1));

    // When requested block is missing from batch response
    let missing_raw = json!([
        {
            "id": 500,
            "result": []
        }
    ]);
    let err = parse_batch_receipts_response(&blocks, missing_raw);
    assert!(err.is_err());
}

#[test]
fn test_parse_batch_receipts_response_method_not_found_downgrade() {
    // 1. Single error object with code -32601
    let single_err = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32601,
            "message": "Method not found"
        }
    });
    assert_eq!(
        parse_batch_receipts_response(&[100], single_err).unwrap(),
        None
    );

    // 2. Single error with "does not exist" message
    let single_err_msg = json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": {
            "code": -32000,
            "message": "the method eth_getBlockReceipts does not exist/is not available"
        }
    });
    assert_eq!(
        parse_batch_receipts_response(&[100], single_err_msg).unwrap(),
        None
    );

    // 3. Batch array with method not found error
    let batch_err = json!([
        {
            "id": 100,
            "error": {
                "code": -32601,
                "message": "unknown method eth_getBlockReceipts"
            }
        }
    ]);
    assert_eq!(
        parse_batch_receipts_response(&[100], batch_err).unwrap(),
        None
    );
}

#[test]
fn test_attach_receipts_to_blocks_matching() {
    let mut blocks = vec![
        DecodedBlock {
            chain_id: 1,
            block_number: 10,
            block_hash: "0x0000000000000000000000000000000000000000000000000000000000000010"
                .to_owned(),
            parent_hash: "0x0000000000000000000000000000000000000000000000000000000000000009"
                .to_owned(),
            timestamp: 100,
            gas_limit: "30000000".to_owned(),
            gas_used: "42000".to_owned(),
            base_fee_per_gas: None,
            beneficiary: None,
            transactions_root: None,
            receipts_root: None,
            state_root: None,
            size: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            transaction_count: 2,
            transactions: vec![
                DecodedTransaction {
                    chain_id: 1,
                    tx_hash: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_owned(),
                    block_number: 10,
                    transaction_index: 0,
                    from_address: "0x1111111111111111111111111111111111111111".to_owned(),
                    to_address: None,
                    value: "0".to_owned(),
                    nonce: "0".to_owned(),
                    gas: "21000".to_owned(),
                    gas_price: None,
                    max_fee_per_gas: None,
                    max_priority_fee_per_gas: None,
                    tx_type: Some(2),
                    method_id: None,
                    status: None,
                    gas_used: None,
                    effective_gas_price: None,
                    l1_fee: None,
                },
                DecodedTransaction {
                    chain_id: 1,
                    tx_hash: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_owned(),
                    block_number: 10,
                    transaction_index: 1,
                    from_address: "0x2222222222222222222222222222222222222222".to_owned(),
                    to_address: None,
                    value: "100".to_owned(),
                    nonce: "1".to_owned(),
                    gas: "21000".to_owned(),
                    gas_price: None,
                    max_fee_per_gas: None,
                    max_priority_fee_per_gas: None,
                    tx_type: Some(2),
                    method_id: None,
                    status: None,
                    gas_used: None,
                    effective_gas_price: None,
                    l1_fee: None,
                },
            ],
        },
    ];

    let mut receipts_by_block = HashMap::new();
    receipts_by_block.insert(
        10,
        vec![
            DecodedReceipt {
                transaction_hash:
                    "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_owned(),
                transaction_index: Some(1),
                block_number: Some(10),
                status: Some(0),
                gas_used: Some("21000".to_owned()),
                effective_gas_price: Some("2000000000".to_owned()),
                l1_fee: Some("1500".to_owned()),
            },
            DecodedReceipt {
                transaction_hash:
                    "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_owned(),
                transaction_index: Some(0),
                block_number: Some(10),
                status: Some(1),
                gas_used: Some("21000".to_owned()),
                effective_gas_price: Some("1500000000".to_owned()),
                l1_fee: Some("1200".to_owned()),
            },
        ],
    );

    attach_receipts_to_blocks(&mut blocks, &receipts_by_block);

    let tx0 = &blocks[0].transactions[0];
    assert_eq!(tx0.status, Some(1));
    assert_eq!(tx0.gas_used, Some("21000".to_owned()));
    assert_eq!(tx0.effective_gas_price, Some("1500000000".to_owned()));
    assert_eq!(tx0.l1_fee, Some("1200".to_owned()));

    let tx1 = &blocks[0].transactions[1];
    assert_eq!(tx1.status, Some(0));
    assert_eq!(tx1.gas_used, Some("21000".to_owned()));
    assert_eq!(tx1.effective_gas_price, Some("2000000000".to_owned()));
    assert_eq!(tx1.l1_fee, Some("1500".to_owned()));
}

#[test]
fn test_transaction_row_receipt_fields() {
    use eventlake::clickhouse::TransactionRow;

    let row = TransactionRow {
        chain_id: 1,
        tx_hash: "0x111102cfba1925b6a715f1fbe148f98a28e367809930f7de3e6b22eb012a6001".to_owned(),
        block_number: 100,
        transaction_index: 0,
        from_address: "0x1111111111111111111111111111111111111111".to_owned(),
        to_address: None,
        value: "0".to_owned(),
        nonce: "0".to_owned(),
        gas: "21000".to_owned(),
        gas_price: None,
        max_fee_per_gas: None,
        max_priority_fee_per_gas: None,
        tx_type: Some(2),
        method_id: None,
        status: Some(1),
        gas_used: Some("21000".to_owned()),
        effective_gas_price: Some("1000000000".to_owned()),
        l1_fee: Some("4660".to_owned()),
        is_canonical: true,
        stored_at: time::OffsetDateTime::now_utc(),
    };

    assert_eq!(row.status, Some(1));
    assert_eq!(row.gas_used.as_deref(), Some("21000"));
    assert_eq!(row.effective_gas_price.as_deref(), Some("1000000000"));
    assert_eq!(row.l1_fee.as_deref(), Some("4660"));
}

#[test]
fn test_partition_block_range_into_slices_historical_and_tip() {
    use eventlake::block_transaction::collector::partition_block_range_into_slices;
    use eventlake::rpc_pool::RpcEndpointRecord;

    fn mock_endpoint(id_u8: u8, max_batch_size: Option<i32>) -> RpcEndpointRecord {
        RpcEndpointRecord {
            id: uuid::Uuid::from_bytes([id_u8; 16]),
            chain_id: 1868,
            url: format!("https://rpc-{id_u8}.soneium.org"),
            status: "healthy".to_owned(),
            weight: 100,
            latency_ms: Some(50),
            last_check_at: None,
            failure_count: 0,
            last_error: None,
            is_archive: true,
            max_block_range: Some(10000),
            max_batch_size,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            cooldown_until: None,
            cooldown_remaining_seconds: None,
        }
    }

    let endpoints = vec![
        mock_endpoint(1, Some(50)), // Official high-throughput
        mock_endpoint(2, Some(20)), // Thirdweb
        mock_endpoint(3, Some(20)), // Sequence
        mock_endpoint(4, Some(20)), // NodeFlare (Full/Pruned)
        mock_endpoint(5, Some(10)), // dRPC
    ];

    // 1. from_block > safe_head -> empty
    let empty = partition_block_range_into_slices(100, 90, &endpoints, 20, false);
    assert!(empty.is_empty());

    // 2. is_tip = true -> always 1 slice with the first endpoint's capacity (50)
    let tip_slices = partition_block_range_into_slices(100, 500, &endpoints, 20, true);
    assert_eq!(tip_slices.len(), 1);
    assert_eq!(tip_slices[0].0.id, endpoints[0].id);
    assert_eq!(tip_slices[0].1.len(), 50);
    assert_eq!(tip_slices[0].1[0], 100);
    assert_eq!(tip_slices[0].1[49], 149);

    // 3. Historical mode with large remaining -> partitions according to each endpoint's capacity
    // 50 + 20 + 20 + 20 + 10 = 120 blocks total
    let large_hist = partition_block_range_into_slices(100, 1000, &endpoints, 20, false);
    assert_eq!(large_hist.len(), 5);

    // Slice 0: Node 1 (cap: 50) -> [100..=149]
    assert_eq!(large_hist[0].0.id, endpoints[0].id);
    assert_eq!(large_hist[0].1.len(), 50);
    assert_eq!(large_hist[0].1[0], 100);
    assert_eq!(large_hist[0].1[49], 149);

    // Slice 1: Node 2 (cap: 20) -> [150..=169]
    assert_eq!(large_hist[1].0.id, endpoints[1].id);
    assert_eq!(large_hist[1].1.len(), 20);
    assert_eq!(large_hist[1].1[0], 150);
    assert_eq!(large_hist[1].1[19], 169);

    // Slice 2: Node 3 (cap: 20) -> [170..=189]
    assert_eq!(large_hist[2].0.id, endpoints[2].id);
    assert_eq!(large_hist[2].1.len(), 20);
    assert_eq!(large_hist[2].1[0], 170);
    assert_eq!(large_hist[2].1[19], 189);

    // Slice 3: Node 4 (cap: 20) -> [190..=209]
    assert_eq!(large_hist[3].0.id, endpoints[3].id);
    assert_eq!(large_hist[3].1.len(), 20);
    assert_eq!(large_hist[3].1[0], 190);
    assert_eq!(large_hist[3].1[19], 209);

    // Slice 4: Node 5 (cap: 10) -> [210..=219]
    assert_eq!(large_hist[4].0.id, endpoints[4].id);
    assert_eq!(large_hist[4].1.len(), 10);
    assert_eq!(large_hist[4].1[0], 210);
    assert_eq!(large_hist[4].1[9], 219);

    // 4. Historical mode when safe_head is reached early (e.g. safe_head = 160)
    // Node 1 gets 50 (100..=149), Node 2 gets remaining 11 (150..=160), Nodes 3-5 omitted
    let early_stop = partition_block_range_into_slices(100, 160, &endpoints, 20, false);
    assert_eq!(early_stop.len(), 2);
    assert_eq!(early_stop[0].1.len(), 50);
    assert_eq!(early_stop[1].1.len(), 11);
    assert_eq!(early_stop[1].1[0], 150);
    assert_eq!(early_stop[1].1[10], 160);
}

#[test]
fn test_validate_block_sequence_multi_slice_continuity_and_gap_detection() {
    use eventlake::rpc_pool::evm_rpc_client::validate_block_sequence;

    fn mock_block(chain_id: i64, block_number: i64, hash: &str, parent_hash: &str) -> DecodedBlock {
        DecodedBlock {
            chain_id,
            block_number,
            block_hash: hash.to_owned(),
            parent_hash: parent_hash.to_owned(),
            timestamp: 1700000000 + block_number,
            gas_limit: "30000000".to_owned(),
            gas_used: "21000".to_owned(),
            base_fee_per_gas: None,
            beneficiary: None,
            transactions_root: None,
            receipts_root: None,
            state_root: None,
            size: None,
            withdrawals_root: None,
            blob_gas_used: None,
            excess_blob_gas: None,
            parent_beacon_block_root: None,
            transaction_count: 0,
            transactions: vec![],
        }
    }

    // Slice 1: blocks 10, 11
    let slice1 = vec![
        mock_block(1, 10, "0x0000000000000000000000000000000000000000000000000000000000000010", "0x0000000000000000000000000000000000000000000000000000000000000009"),
        mock_block(1, 11, "0x0000000000000000000000000000000000000000000000000000000000000011", "0x0000000000000000000000000000000000000000000000000000000000000010"),
    ];

    // Slice 2: blocks 12, 13
    let slice2 = vec![
        mock_block(1, 12, "0x0000000000000000000000000000000000000000000000000000000000000012", "0x0000000000000000000000000000000000000000000000000000000000000011"),
        mock_block(1, 13, "0x0000000000000000000000000000000000000000000000000000000000000013", "0x0000000000000000000000000000000000000000000000000000000000000012"),
    ];

    // Concatenated: seamless sequence
    let mut all_blocks = slice1.clone();
    all_blocks.extend(slice2);
    assert!(validate_block_sequence(&all_blocks).is_ok());

    // Gap detection: missing block 12 (10, 11, 13)
    let gapped_blocks = vec![
        slice1[0].clone(),
        slice1[1].clone(),
        mock_block(1, 13, "0x0000000000000000000000000000000000000000000000000000000000000013", "0x0000000000000000000000000000000000000000000000000000000000000012"),
    ];
    let gap_err = validate_block_sequence(&gapped_blocks);
    assert!(gap_err.is_err());
    assert!(gap_err.unwrap_err().to_string().contains("block height gap detected"));

    // Hash mismatch detection: block 12 parent_hash does not match block 11 block_hash
    let fork_blocks = vec![
        slice1[0].clone(),
        slice1[1].clone(),
        mock_block(1, 12, "0x0000000000000000000000000000000000000000000000000000000000000012", "0x0000000000000000000000000000000000000000000000000000000000000099"),
    ];
    let fork_err = validate_block_sequence(&fork_blocks);
    assert!(fork_err.is_err());
    assert!(fork_err.unwrap_err().to_string().contains("parent hash mismatch"));
}

#[test]
fn test_block_time_range_and_by_time_models() {
    use eventlake::block_transaction::api::{
        AddressProfileResponse, BlockByTimeQuery, BlockTimeRangeResponse, BlocksTimeRangeQuery,
        TransactionStatusResponse,
    };

    let range = BlockTimeRangeResponse {
        chain_id: 1,
        start_time: 1700000000,
        end_time: 1700003600,
        start_block: Some(100),
        end_block: Some(200),
        block_count: 101,
    };
    assert_eq!(range.block_count, 101);

    let status = TransactionStatusResponse {
        chain_id: 1,
        tx_hash: "0x123".to_owned(),
        block_number: 100,
        status: Some(1),
        gas_used: Some("21000".to_owned()),
        current_head: 150,
        confirmations: 51,
        is_canonical: true,
    };
    assert_eq!(status.confirmations, 51);

    let profile = AddressProfileResponse {
        chain_id: 1,
        address: "0x0000000000000000000000000000000000000001".to_owned(),
        first_block: Some(10),
        last_block: Some(100),
        sent_tx_count: 5,
        received_tx_count: 10,
        last_nonce: Some("4".to_owned()),
    };
    assert_eq!(profile.sent_tx_count, 5);

    let q1: BlockByTimeQuery = serde_json::from_str(r#"{"timestamp": 1700000000}"#).unwrap();
    assert_eq!(q1.timestamp, 1700000000);
    assert_eq!(q1.closest, None);

    let q2: BlocksTimeRangeQuery =
        serde_json::from_str(r#"{"start_time": 100, "end_time": 200}"#).unwrap();
    assert_eq!(q2.start_time, 100);
    assert_eq!(q2.end_time, 200);
}

#[test]
fn test_advanced_block_transaction_models() {
    use eventlake::block_transaction::api::{
        BlockGasConsumerResponse, BlockGasConsumersQuery, GasEstimate, GasOracleResponse,
        NetworkStatsResponse, TopContractResponse, TopContractsQuery, WhaleTransfersQuery,
    };

    let consumer = BlockGasConsumerResponse {
        from_address: "0x1111111111111111111111111111111111111111".to_owned(),
        total_gas_used: "150000".to_owned(),
        tx_count: 2,
    };
    assert_eq!(consumer.tx_count, 2);

    let oracle = GasOracleResponse {
        chain_id: 1,
        base_fee: Some("100".to_owned()),
        slow: GasEstimate {
            max_priority_fee_per_gas: "10".to_owned(),
            max_fee_per_gas: "110".to_owned(),
        },
        normal: GasEstimate {
            max_priority_fee_per_gas: "20".to_owned(),
            max_fee_per_gas: "132".to_owned(),
        },
        fast: GasEstimate {
            max_priority_fee_per_gas: "30".to_owned(),
            max_fee_per_gas: "155".to_owned(),
        },
    };
    assert_eq!(oracle.normal.max_priority_fee_per_gas, "20");

    let stats = NetworkStatsResponse {
        chain_id: 1,
        latest_block: 1000,
        latest_timestamp: 1700000000,
        tps_last_1h: 15.5,
        avg_gas_utilization_percent: 55.2,
        avg_block_time_seconds: 2.0,
    };
    assert_eq!(stats.latest_block, 1000);

    let top = TopContractResponse {
        contract_address: "0x2222222222222222222222222222222222222222".to_owned(),
        tx_count: 50,
        user_count: 10,
        total_gas_used: "2500000".to_owned(),
    };
    assert_eq!(top.tx_count, 50);

    let q_gas: BlockGasConsumersQuery = serde_json::from_str(r#"{"limit": 25}"#).unwrap();
    assert_eq!(q_gas.limit, Some(25));

    let q_top: TopContractsQuery =
        serde_json::from_str(r#"{"window_blocks": 500, "limit": 10}"#).unwrap();
    assert_eq!(q_top.window_blocks, Some(500));
    assert_eq!(q_top.limit, Some(10));

    let q_whale: WhaleTransfersQuery =
        serde_json::from_str(r#"{"min_value": "5000000000000000000", "limit": 50}"#).unwrap();
    assert_eq!(q_whale.min_value, Some("5000000000000000000".to_owned()));
    assert_eq!(q_whale.limit, Some(50));
}


