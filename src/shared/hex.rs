use crate::shared::error::ApplicationError;

pub fn parse_hex_u64(value: &str) -> Result<i64, ApplicationError> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    i64::from_str_radix(trimmed, 16)
        .map_err(|_| ApplicationError::BadRequest(format!("invalid hex number: {value}")))
}

pub fn normalize_hex(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("0x") {
        lower
    } else {
        format!("0x{lower}")
    }
}
