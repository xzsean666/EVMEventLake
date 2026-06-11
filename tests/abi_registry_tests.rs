use eventlake::abi_registry::parse_abi_from_value;
use serde_json::json;

#[test]
fn parses_erc20_transfer_event_from_uploaded_abi() {
    let abi_json = json!([
        {
            "anonymous": false,
            "inputs": [
                {"indexed": true, "internalType": "address", "name": "from", "type": "address"},
                {"indexed": true, "internalType": "address", "name": "to", "type": "address"},
                {"indexed": false, "internalType": "uint256", "name": "value", "type": "uint256"}
            ],
            "name": "Transfer",
            "type": "event"
        }
    ]);

    let parsed = parse_abi_from_value(&abi_json).expect("ABI parses");
    let transfer_events = parsed
        .events
        .get("Transfer")
        .expect("Transfer event exists");

    assert_eq!(transfer_events.len(), 1);
    assert_eq!(
        transfer_events[0].signature(),
        "Transfer(address,address,uint256)"
    );
}
