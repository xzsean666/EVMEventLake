use alloy_primitives::U256;

use crate::shared::error::ApplicationError;

pub fn parse_hex_u64(value: &str) -> Result<i64, ApplicationError> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    i64::from_str_radix(trimmed, 16)
        .map_err(|_| ApplicationError::BadRequest(format!("invalid hex number: {value}")))
}

pub fn parse_hex_u64_quantity(value: &str) -> Result<u64, ApplicationError> {
    let trimmed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if trimmed.is_empty() {
        return Err(ApplicationError::BadRequest(format!(
            "invalid empty hex quantity: {value}"
        )));
    }
    u64::from_str_radix(trimmed, 16)
        .map_err(|_| ApplicationError::BadRequest(format!("invalid hex quantity: {value}")))
}

pub fn parse_hex_u256_to_dec(value: &str) -> Result<String, ApplicationError> {
    let trimmed = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if trimmed.is_empty() {
        return Err(ApplicationError::BadRequest(format!(
            "invalid empty hex u256: {value}"
        )));
    }
    let parsed = U256::from_str_radix(trimmed, 16)
        .map_err(|_| ApplicationError::BadRequest(format!("invalid hex u256: {value}")))?;
    Ok(parsed.to_string())
}

pub fn extract_method_id(input: &str) -> Option<String> {
    let trimmed = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if trimmed.len() < 8 {
        return None;
    }
    let method_slice = &trimmed[..8];
    if method_slice.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("0x{}", method_slice.to_ascii_lowercase()))
    } else {
        None
    }
}

pub fn normalize_hex(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("0x") {
        lower
    } else {
        format!("0x{lower}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_u256_preserves_large_integers() {
        let max_u256_hex = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let dec = parse_hex_u256_to_dec(max_u256_hex).expect("parses max u256");
        assert_eq!(
            dec,
            "115792089237316195423570985008687907853269984665640564039457584007913129639935"
        );

        let zero = parse_hex_u256_to_dec("0x0").expect("parses zero");
        assert_eq!(zero, "0");

        assert!(parse_hex_u256_to_dec("").is_err());
        assert!(parse_hex_u256_to_dec("0x").is_err());
        assert!(parse_hex_u256_to_dec("0xnothex").is_err());
    }

    #[test]
    fn extract_method_id_handles_various_calldata() {
        assert_eq!(
            extract_method_id("0xa9059cbb000000000000000000000000"),
            Some("0xa9059cbb".to_owned())
        );
        assert_eq!(
            extract_method_id("A9059CBB12345678"),
            Some("0xa9059cbb".to_owned())
        );
        assert_eq!(extract_method_id("0x"), None);
        assert_eq!(extract_method_id("0x1234"), None);
        assert_eq!(extract_method_id(""), None);
    }
}
