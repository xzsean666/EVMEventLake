use eventlake::shared::validation::{normalize_address, normalize_topic};

#[test]
fn normalizes_valid_evm_address() {
    let address =
        normalize_address("742d35Cc6634C0532925a3b844Bc454e4438f44e").expect("address is valid");

    assert_eq!(address, "0x742d35cc6634c0532925a3b844bc454e4438f44e");
}

#[test]
fn rejects_short_topic() {
    assert!(normalize_topic("0x1234").is_err());
}

#[test]
fn rejects_duplicate_0x_prefix() {
    assert!(normalize_address("0x0x742d35cc6634c0532925a3b844bc454e4438f44e").is_err());
    assert!(
        normalize_topic("0x0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef")
            .is_err()
    );
    assert!(
        eventlake::shared::validation::normalize_hash(
            "0x0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"
        )
        .is_err()
    );
}

