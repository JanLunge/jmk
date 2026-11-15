use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::Credentials;
use crate::error::{parse_json, ApiError};
use crate::pagination::{Page, PageRequest};
use crate::{keyboards, validate, SharedState};

#[derive(Debug, Deserialize)]
struct NewReport {
    reason: String,
}

pub async fn report(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let input: NewReport = parse_json(body)?;
    let reason = validate::reason(&input.reason)?;
    state
        .db(move |connection| {
            let keyboard = keyboards::find_visible(connection, &slug)?;
            connection.execute(
                "INSERT INTO reports (keyboard_id, reason) VALUES (?1, ?2)",
                params![keyboard.id, reason],
            )?;
            Ok(())
        })
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "received" }))))
}

fn require_admin(state: &SharedState, headers: &HeaderMap) -> Result<(), ApiError> {
    Credentials::from_headers(headers).require_admin(state.admin_token_hash.as_deref())
}

#[derive(Debug, Deserialize)]
pub struct ReportsQuery {
    page: Option<String>,
    per_page: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    id: i64,
    keyboard: String,
    keyboard_name: String,
    keyboard_hidden: bool,
    reason: String,
    created_at: String,
}

pub async fn list_reports(
    State(state): State<SharedState>,
    headers: HeaderMap,
    query: Result<Query<ReportsQuery>, QueryRejection>,
) -> Result<Json<Page<Report>>, ApiError> {
    require_admin(&state, &headers)?;
    let Query(query) = query
        .map_err(|_| ApiError::bad_request("invalid_query", "the query string could not be read"))?;
    let page = PageRequest::parse(query.page.as_deref(), query.per_page.as_deref())?;
    let reports = state
        .db(move |connection| {
            let total: u64 =
                connection.query_row("SELECT COUNT(*) FROM reports", [], |row| row.get(0))?;
            let mut statement = connection.prepare(
                "SELECT r.id, k.slug, k.name, k.hidden, r.reason, r.created_at \
                 FROM reports r JOIN keyboards k ON k.id = r.keyboard_id \
                 ORDER BY r.id DESC LIMIT ?1 OFFSET ?2",
            )?;
            let items = statement
                .query_map([page.limit(), page.offset()], |row| {
                    Ok(Report {
                        id: row.get(0)?,
                        keyboard: row.get(1)?,
                        keyboard_name: row.get(2)?,
                        keyboard_hidden: row.get(3)?,
                        reason: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(page.page_of(items, total))
        })
        .await?;
    Ok(Json(reports))
}

pub async fn remove_report(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    require_admin(&state, &headers)?;
    let id: i64 = id.parse().map_err(|_| ApiError::not_found("report"))?;
    state
        .db(move |connection| {
            match connection.execute("DELETE FROM reports WHERE id = ?1", [id])? {
                0 => Err(ApiError::not_found("report")),
                _ => Ok(()),
            }
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn hide(
    state: State<SharedState>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    set_hidden(state, slug, headers, true).await
}

pub async fn unhide(
    state: State<SharedState>,
    slug: Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    set_hidden(state, slug, headers, false).await
}

async fn set_hidden(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    hidden: bool,
) -> Result<Json<Value>, ApiError> {
    require_admin(&state, &headers)?;
    let body = json!({ "slug": slug, "hidden": hidden });
    state
        .db(move |connection| {
            match connection
                .execute("UPDATE keyboards SET hidden = ?1 WHERE slug = ?2", params![hidden, slug])?
            {
                0 => Err(ApiError::not_found("keyboard")),
                _ => Ok(()),
            }
        })
        .await?;
    Ok(Json(body))
}
