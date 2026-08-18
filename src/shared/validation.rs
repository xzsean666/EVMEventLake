use crate::shared::{error::ApplicationError, hex::normalize_hex};

pub fn normalize_address(value: &str) -> Result<String, ApplicationError> {
    let normalized = normalize_hex(value);
    let body = normalized.trim_start_matches("0x");

    if body.len() != 40 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM address: {value}"
        )));
    }

    Ok(normalized)
}

pub fn normalize_topic(value: &str) -> Result<String, ApplicationError> {
    let normalized = normalize_hex(value);
    let body = normalized.trim_start_matches("0x");

    if body.len() != 64 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM topic: {value}"
        )));
    }

    Ok(normalized)
}

pub fn normalize_hash(value: &str) -> Result<String, ApplicationError> {
    let normalized = normalize_hex(value);
    let body = normalized.trim_start_matches("0x");

    if body.len() != 64 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM hash: {value}"
        )));
    }

    Ok(normalized)
}
