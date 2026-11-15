//! Registry of shared JMK keyboard definitions, keymaps and firmware images.

mod auth;
mod config;
mod db;
mod error;
mod firmware;
mod keyboards;
mod keymaps;
mod moderation;
mod pagination;
mod rate_limit;
mod validate;

use std::sync::{Arc, Mutex};

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, Method, StatusCode};
use axum::routing::{delete, get, post};
use axum::{middleware, Json, Router};
use rusqlite::Connection;
use serde_json::{json, Value};
use tower_http::cors::{Any, CorsLayer};

pub use config::Config;
use error::ApiError;
use rate_limit::RateLimiter;

pub const MAX_BODY_BYTES: usize = 256 * 1024;

pub struct AppState {
    config: Config,
    connection: Mutex<Connection>,
    admin_token_hash: Option<String>,
    rate_limiter: RateLimiter,
    firmware_hashes: firmware::HashCache,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    /// Opens the database, bringing its schema up to date.
    pub fn open(config: Config) -> Result<SharedState, String> {
        let connection = db::open(&config.db_path)?;
        Ok(Arc::new(Self {
            connection: Mutex::new(connection),
            admin_token_hash: config.admin_token.as_deref().map(auth::hash_token),
            rate_limiter: RateLimiter::new(config.write_limit, config.write_window),
            firmware_hashes: firmware::HashCache::default(),
            config,
        }))
    }

    /// Runs database work on the blocking pool so it cannot stall the
    /// async workers.
    async fn db<T, F>(self: &Arc<Self>, work: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, ApiError> + Send + 'static,
    {
        let state = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            // A panic in earlier work rolled its transaction back when it
            // unwound, so the connection behind a poisoned lock is still sound.
            let mut connection =
                state.connection.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            work(&mut connection)
        })
        .await
        .map_err(ApiError::internal)?
    }
}

pub fn app(state: SharedState) -> Router {
    let api = Router::new()
        .route("/keyboards", get(keyboards::list).post(keyboards::create))
        .route(
            "/keyboards/{slug}",
            get(keyboards::show).put(keyboards::update).delete(keyboards::remove),
        )
        .route("/keyboards/{slug}/versions", get(keyboards::versions))
        .route("/keyboards/{slug}/versions/{version}", get(keyboards::version))
        .route("/keyboards/{slug}/keymaps", get(keymaps::list).post(keymaps::create))
        .route("/keyboards/{slug}/report", post(moderation::report))
        .route(
            "/keymaps/{id}",
            get(keymaps::show).put(keymaps::update).delete(keymaps::remove),
        )
        .route("/admin/reports", get(moderation::list_reports))
        .route("/admin/reports/{id}", delete(moderation::remove_report))
        .route("/admin/keyboards/{slug}/hide", post(moderation::hide))
        .route("/admin/keyboards/{slug}/unhide", post(moderation::unhide))
        .route("/firmware", get(firmware::list))
        .route("/firmware/{name}", get(firmware::download));

    // The configurator is a desktop app and nothing here relies on cookies,
    // so any origin may call the API.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api/v1", api)
        .fallback(route_not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(Arc::clone(&state), rate_limit::limit_writes))
        .layer(cors)
        .with_state(state)
}

async fn healthz(State(state): State<SharedState>) -> Result<Json<Value>, ApiError> {
    state
        .db(|connection| Ok(connection.query_row("SELECT 1", [], |_| Ok(()))?))
        .await?;
    Ok(Json(json!({ "status": "ok" })))
}

async fn route_not_found() -> ApiError {
    ApiError::not_found("route")
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "this route does not support that method",
    )
}
