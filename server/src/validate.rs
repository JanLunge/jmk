use serde_json::Value;

use crate::error::ApiError;

pub const MCUS: [&str; 3] = ["rp2040", "rp2350", "nrf52840"];

const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 2000;
const MAX_AUTHOR_CHARS: usize = 64;
const MAX_TAGS: usize = 8;
const MAX_TAG_CHARS: usize = 24;
const MAX_KEY_COUNT: i64 = 1024;
const MAX_REASON_CHARS: usize = 1000;

pub fn name(value: &str) -> Result<String, ApiError> {
    let name = value.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_NAME_CHARS {
        return Err(ApiError::validation(format!("name must be 1 to {MAX_NAME_CHARS} characters")));
    }
    Ok(name.to_string())
}

pub fn description(value: &str) -> Result<String, ApiError> {
    at_most("description", value.trim(), MAX_DESCRIPTION_CHARS)
}

pub fn author(value: &str) -> Result<String, ApiError> {
    at_most("author", value.trim(), MAX_AUTHOR_CHARS)
}

pub fn reason(value: &str) -> Result<String, ApiError> {
    let reason = value.trim();
    if reason.is_empty() {
        return Err(ApiError::validation("reason must not be empty"));
    }
    at_most("reason", reason, MAX_REASON_CHARS)
}

fn at_most(field: &str, value: &str, max_chars: usize) -> Result<String, ApiError> {
    if value.chars().count() > max_chars {
        return Err(ApiError::validation(format!("{field} must be at most {max_chars} characters")));
    }
    Ok(value.to_string())
}

pub fn tag(value: &str) -> Result<(), ApiError> {
    let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-';
    if value.is_empty() || value.len() > MAX_TAG_CHARS || !value.bytes().all(allowed) {
        return Err(ApiError::validation(format!(
            "a tag must be 1 to {MAX_TAG_CHARS} characters of a-z, 0-9 and -"
        )));
    }
    Ok(())
}

/// Checks every tag and drops repeats, keeping the first occurrence.
pub fn tags(values: Vec<String>) -> Result<Vec<String>, ApiError> {
    let mut unique: Vec<String> = Vec::new();
    for value in values {
        tag(&value)?;
        if !unique.contains(&value) {
            unique.push(value);
        }
    }
    if unique.len() > MAX_TAGS {
        return Err(ApiError::validation(format!("at most {MAX_TAGS} tags are allowed")));
    }
    Ok(unique)
}

pub fn mcu(value: &str) -> Result<&'static str, ApiError> {
    MCUS.into_iter()
        .find(|known| *known == value)
        .ok_or_else(|| ApiError::validation(format!("mcu must be one of {}", MCUS.join(", "))))
}

pub fn key_count(value: i64) -> Result<u32, ApiError> {
    if !(1..=MAX_KEY_COUNT).contains(&value) {
        return Err(ApiError::validation(format!("key_count must be from 1 to {MAX_KEY_COUNT}")));
    }
    Ok(value as u32)
}

/// The definition's contents belong to the configurator and are stored as
/// given. Only the envelope is checked: an object carrying its schema number.
pub fn definition(value: &Value) -> Result<(), ApiError> {
    let Some(object) = value.as_object() else {
        return Err(ApiError::validation("definition must be a JSON object"));
    };
    match object.get("schema") {
        Some(schema) if schema.is_i64() || schema.is_u64() => Ok(()),
        _ => Err(ApiError::validation("definition.schema must be an integer")),
    }
}
