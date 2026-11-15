use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use serde_json::json;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    pub fn not_found(what: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", format!("{what} not found"))
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "validation_failed", message)
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    /// Logs the cause and answers without it: internal details are not for clients.
    pub fn internal(cause: impl std::fmt::Display) -> Self {
        eprintln!("jmk-registry: internal error: {cause}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal server error")
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "code": self.code, "message": self.message } }));
        let mut response = (self.status, body).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

/// Reads a JSON request body. The body is taken as bytes rather than through
/// axum's `Json` extractor so that every failure is reported in the API's own
/// error format.
pub fn parse_json<T: DeserializeOwned>(body: Result<Bytes, BytesRejection>) -> Result<T, ApiError> {
    let bytes = body.map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
                format!("request body exceeds {} KiB", crate::MAX_BODY_BYTES / 1024),
            )
        } else {
            ApiError::bad_request("invalid_body", "request body could not be read")
        }
    })?;
    serde_json::from_slice(&bytes).map_err(|error| match error.classify() {
        serde_json::error::Category::Data => ApiError::validation(error.to_string()),
        _ => ApiError::bad_request("invalid_json", format!("request body is not valid JSON: {error}")),
    })
}
