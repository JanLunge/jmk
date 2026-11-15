use axum::http::{header, HeaderMap, StatusCode};
use sha2::{Digest, Sha256};

use crate::error::ApiError;

/// A fresh edit token: 256 random bits as hex. Only its hash is stored.
pub fn new_token() -> Result<String, ApiError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(ApiError::internal)?;
    Ok(hex(&bytes))
}

pub fn hash_token(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The bearer token a request presented, already hashed.
pub struct Credentials {
    token_hash: Option<String>,
}

impl Credentials {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let token = headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                let (scheme, token) = value.split_once(' ')?;
                scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
            })
            .filter(|token| !token.is_empty());
        Self { token_hash: token.map(hash_token) }
    }

    pub fn is_admin(&self, admin_token_hash: Option<&str>) -> bool {
        matches!((&self.token_hash, admin_token_hash), (Some(given), Some(admin)) if same(given, admin))
    }

    pub fn require_admin(&self, admin_token_hash: Option<&str>) -> Result<(), ApiError> {
        let Some(admin) = admin_token_hash else {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "admin_disabled",
                "moderation is not enabled on this server",
            ));
        };
        self.require(admin)
    }

    pub fn require_edit_token(&self, edit_token_hash: &str) -> Result<(), ApiError> {
        self.require(edit_token_hash)
    }

    fn require(&self, expected_hash: &str) -> Result<(), ApiError> {
        match &self.token_hash {
            None => Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "missing_token",
                "this request needs an Authorization: Bearer token",
            )),
            Some(given) if same(given, expected_hash) => Ok(()),
            Some(_) => Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "wrong_token",
                "the token does not grant access to this resource",
            )),
        }
    }
}

/// Compares without stopping at the first difference, so response time does
/// not reveal how much of a hash matched.
fn same(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left.bytes().zip(right.bytes()).fold(0, |difference, (a, b)| difference | (a ^ b)) == 0
}
