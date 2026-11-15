use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{self, Credentials};
use crate::db::{self, NOW};
use crate::error::{parse_json, ApiError};
use crate::pagination::Items;
use crate::{keyboards, validate, SharedState};

const SELECT: &str = "SELECT m.id, k.slug, m.name, m.description, m.author, m.created_at, \
                      m.updated_at, m.definition, m.edit_token_hash, k.hidden \
                      FROM keymaps m JOIN keyboards k ON k.id = m.keyboard_id";

#[derive(Debug, Serialize)]
pub struct KeymapSummary {
    id: i64,
    keyboard: String,
    name: String,
    description: String,
    author: String,
    created_at: String,
    updated_at: String,
}

struct Keymap {
    summary: KeymapSummary,
    definition: String,
    edit_token_hash: String,
    keyboard_hidden: bool,
}

fn from_row(row: &Row) -> rusqlite::Result<Keymap> {
    Ok(Keymap {
        summary: KeymapSummary {
            id: row.get(0)?,
            keyboard: row.get(1)?,
            name: row.get(2)?,
            description: row.get(3)?,
            author: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        },
        definition: row.get(7)?,
        edit_token_hash: row.get(8)?,
        keyboard_hidden: row.get(9)?,
    })
}

/// Finds a keymap, including one whose keyboard is hidden.
fn find(connection: &Connection, id: i64) -> Result<Keymap, ApiError> {
    connection
        .query_row(&format!("{SELECT} WHERE m.id = ?1"), [id], from_row)
        .optional()?
        .ok_or_else(|| ApiError::not_found("keymap"))
}

/// A keymap of a hidden keyboard is hidden with it.
fn find_visible(connection: &Connection, id: i64) -> Result<Keymap, ApiError> {
    let keymap = find(connection, id)?;
    if keymap.keyboard_hidden {
        return Err(ApiError::not_found("keymap"));
    }
    Ok(keymap)
}

fn parse_id(id: &str) -> Result<i64, ApiError> {
    id.parse().map_err(|_| ApiError::not_found("keymap"))
}

pub async fn list(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Json<Items<KeymapSummary>>, ApiError> {
    let items = state
        .db(move |connection| {
            let keyboard = keyboards::find_visible(connection, &slug)?;
            let mut statement = connection
                .prepare(&format!("{SELECT} WHERE m.keyboard_id = ?1 ORDER BY m.updated_at DESC, m.id DESC"))?;
            let items = statement
                .query_map([keyboard.id], from_row)?
                .map(|keymap| keymap.map(|keymap| keymap.summary))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(items)
        })
        .await?;
    Ok(Json(Items { items }))
}

#[derive(Debug, Deserialize)]
struct CreateKeymap {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    definition: Value,
}

#[derive(Debug, Serialize)]
pub struct Created {
    id: i64,
    edit_token: String,
}

pub async fn create(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    body: Result<Bytes, BytesRejection>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let input: CreateKeymap = parse_json(body)?;
    let name = validate::name(&input.name)?;
    let description = validate::description(&input.description)?;
    let author = validate::author(&input.author)?;
    validate::definition(&input.definition)?;

    let edit_token = auth::new_token()?;
    let edit_token_hash = auth::hash_token(&edit_token);
    let id = state
        .db(move |connection| {
            let keyboard = keyboards::find_visible(connection, &slug)?;
            connection.execute(
                "INSERT INTO keymaps (keyboard_id, name, description, author, definition, \
                 edit_token_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    keyboard.id,
                    name,
                    description,
                    author,
                    input.definition.to_string(),
                    edit_token_hash
                ],
            )?;
            Ok(connection.last_insert_rowid())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(Created { id, edit_token })))
}

#[derive(Debug, Serialize)]
pub struct KeymapWithDefinition {
    #[serde(flatten)]
    summary: KeymapSummary,
    definition: Value,
}

impl TryFrom<Keymap> for KeymapWithDefinition {
    type Error = ApiError;

    fn try_from(keymap: Keymap) -> Result<Self, ApiError> {
        Ok(Self {
            definition: db::parse_definition(&keymap.definition)?,
            summary: keymap.summary,
        })
    }
}

pub async fn show(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> Result<Json<KeymapWithDefinition>, ApiError> {
    let id = parse_id(&id)?;
    let keymap = state
        .db(move |connection| find_visible(connection, id)?.try_into())
        .await?;
    Ok(Json(keymap))
}

/// Every field is optional: what is absent stays as it is.
#[derive(Debug, Deserialize)]
struct UpdateKeymap {
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    definition: Option<Value>,
}

pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<KeymapWithDefinition>, ApiError> {
    let id = parse_id(&id)?;
    let credentials = Credentials::from_headers(&headers);
    let input: UpdateKeymap = parse_json(body)?;
    let name = input.name.as_deref().map(validate::name).transpose()?;
    let description = input.description.as_deref().map(validate::description).transpose()?;
    let author = input.author.as_deref().map(validate::author).transpose()?;
    if let Some(definition) = &input.definition {
        validate::definition(definition)?;
    }
    if name.is_none() && description.is_none() && author.is_none() && input.definition.is_none() {
        return Err(ApiError::validation("the request changes nothing"));
    }

    let keymap = state
        .db(move |connection| {
            let transaction = connection.transaction()?;
            let keymap = find_visible(&transaction, id)?;
            credentials.require_edit_token(&keymap.edit_token_hash)?;
            transaction.execute(
                &format!(
                    "UPDATE keymaps SET name = ?1, description = ?2, author = ?3, \
                     definition = ?4, updated_at = {NOW} WHERE id = ?5"
                ),
                params![
                    name.unwrap_or(keymap.summary.name),
                    description.unwrap_or(keymap.summary.description),
                    author.unwrap_or(keymap.summary.author),
                    input.definition.map(|definition| definition.to_string()).unwrap_or(keymap.definition),
                    id
                ],
            )?;
            let updated = find(&transaction, id)?;
            transaction.commit()?;
            updated.try_into()
        })
        .await?;
    Ok(Json(keymap))
}

pub async fn remove(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let credentials = Credentials::from_headers(&headers);
    let is_admin = credentials.is_admin(state.admin_token_hash.as_deref());
    state
        .db(move |connection| {
            if is_admin {
                find(connection, id)?;
            } else {
                let keymap = find_visible(connection, id)?;
                credentials.require_edit_token(&keymap.edit_token_hash)?;
            }
            connection.execute("DELETE FROM keymaps WHERE id = ?1", [id])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
