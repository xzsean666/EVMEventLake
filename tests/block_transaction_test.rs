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

#[cfg(feature = "clickhouse")]
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
