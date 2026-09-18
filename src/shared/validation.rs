use crate::shared::error::ApplicationError;

pub fn normalize_address(value: &str) -> Result<String, ApplicationError> {
    let body = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);

    if body.len() != 40 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM address: {value}"
        )));
    }

    Ok(format!("0x{}", body.to_ascii_lowercase()))
}

pub fn normalize_topic(value: &str) -> Result<String, ApplicationError> {
    let body = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);

    if body.len() != 64 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM topic: {value}"
        )));
    }

    Ok(format!("0x{}", body.to_ascii_lowercase()))
}

pub fn normalize_hash(value: &str) -> Result<String, ApplicationError> {
    let body = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);

    if body.len() != 64 || !body.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApplicationError::BadRequest(format!(
            "invalid EVM hash: {value}"
        )));
    }

    Ok(format!("0x{}", body.to_ascii_lowercase()))
}
