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
