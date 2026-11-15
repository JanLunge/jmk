use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rusqlite::types::{Type, Value as SqlValue};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{self, Credentials};
use crate::db::{self, NOW};
use crate::error::{parse_json, ApiError};
use crate::pagination::{Items, Page, PageRequest};
use crate::{validate, SharedState};

const COLUMNS: &str = "id, slug, name, description, author, tags, mcu, key_count, split, \
                       version, downloads, updated_at, hidden, edit_token_hash";

const MAX_SLUG_CHARS: usize = 48;
const MAX_SEARCH_TERMS: usize = 8;

#[derive(Debug, Serialize)]
pub struct KeyboardSummary {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    pub mcu: String,
    pub key_count: u32,
    pub split: bool,
    pub version: u32,
    pub downloads: u64,
    pub updated_at: String,
}

pub struct Keyboard {
    pub id: i64,
    pub hidden: bool,
    pub edit_token_hash: String,
    pub summary: KeyboardSummary,
}

fn from_row(row: &Row) -> rusqlite::Result<Keyboard> {
    let tags: String = row.get("tags")?;
    let tags = serde_json::from_str(&tags)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(error)))?;
    Ok(Keyboard {
        id: row.get("id")?,
        hidden: row.get("hidden")?,
        edit_token_hash: row.get("edit_token_hash")?,
        summary: KeyboardSummary {
            slug: row.get("slug")?,
            name: row.get("name")?,
            description: row.get("description")?,
            author: row.get("author")?,
            tags,
            mcu: row.get("mcu")?,
            key_count: row.get("key_count")?,
            split: row.get("split")?,
            version: row.get("version")?,
            downloads: row.get("downloads")?,
            updated_at: row.get("updated_at")?,
        },
    })
}

/// Finds a keyboard whether or not it is hidden.
pub fn find(connection: &Connection, slug: &str) -> Result<Keyboard, ApiError> {
    connection
        .query_row(&format!("SELECT {COLUMNS} FROM keyboards WHERE slug = ?1"), [slug], from_row)
        .optional()?
        .ok_or_else(|| ApiError::not_found("keyboard"))
}

/// Finds a keyboard as the public sees it: a hidden one does not exist.
pub fn find_visible(connection: &Connection, slug: &str) -> Result<Keyboard, ApiError> {
    let keyboard = find(connection, slug)?;
    if keyboard.hidden {
        return Err(ApiError::not_found("keyboard"));
    }
    Ok(keyboard)
}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.is_empty())
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    q: Option<String>,
    tag: Option<String>,
    mcu: Option<String>,
    split: Option<String>,
    sort: Option<String>,
    page: Option<String>,
    per_page: Option<String>,
}

pub async fn list(
    State(state): State<SharedState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Page<KeyboardSummary>>, ApiError> {
    let invalid = |message: &str| ApiError::bad_request("invalid_query", message);
    let Query(query) = query.map_err(|_| invalid("the query string could not be read"))?;
    let page = PageRequest::parse(present(&query.page), present(&query.per_page))?;

    let mut conditions = vec!["hidden = 0".to_string()];
    let mut values: Vec<SqlValue> = Vec::new();
    if let Some(q) = present(&query.q) {
        // Every term must appear somewhere in the searchable text.
        for term in q.split_whitespace().take(MAX_SEARCH_TERMS) {
            conditions.push(
                "(name LIKE ? ESCAPE '\\' OR description LIKE ? ESCAPE '\\' \
                 OR author LIKE ? ESCAPE '\\' OR tags LIKE ? ESCAPE '\\')"
                    .to_string(),
            );
            let pattern = db::like_pattern(term);
            values.extend(std::iter::repeat_n(SqlValue::Text(pattern), 4));
        }
    }
    if let Some(tag) = present(&query.tag) {
        conditions
            .push("EXISTS (SELECT 1 FROM json_each(keyboards.tags) WHERE value = ?)".to_string());
        values.push(SqlValue::Text(tag.to_string()));
    }
    if let Some(mcu) = present(&query.mcu) {
        let mcu = validate::mcu(mcu).map_err(|error| invalid(&error.message))?;
        conditions.push("mcu = ?".to_string());
        values.push(SqlValue::Text(mcu.to_string()));
    }
    if let Some(split) = present(&query.split) {
        let split = match split {
            "true" => true,
            "false" => false,
            _ => return Err(invalid("split must be true or false")),
        };
        conditions.push("split = ?".to_string());
        values.push(SqlValue::Integer(split.into()));
    }
    let order = match present(&query.sort).unwrap_or("recent") {
        "recent" => "updated_at DESC, id DESC",
        "popular" => "downloads DESC, id DESC",
        "name" => "name COLLATE NOCASE ASC, id ASC",
        _ => return Err(invalid("sort must be recent, popular or name")),
    };

    let listing = state
        .db(move |connection| {
            let filter = conditions.join(" AND ");
            let total: u64 = connection.query_row(
                &format!("SELECT COUNT(*) FROM keyboards WHERE {filter}"),
                params_from_iter(values.iter()),
                |row| row.get(0),
            )?;
            values.push(SqlValue::Integer(page.limit()));
            values.push(SqlValue::Integer(page.offset()));
            let mut statement = connection.prepare(&format!(
                "SELECT {COLUMNS} FROM keyboards WHERE {filter} ORDER BY {order} LIMIT ? OFFSET ?"
            ))?;
            let items = statement
                .query_map(params_from_iter(values.iter()), from_row)?
                .map(|keyboard| keyboard.map(|keyboard| keyboard.summary))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(page.page_of(items, total))
        })
        .await?;
    Ok(Json(listing))
}

#[derive(Debug, Deserialize)]
struct CreateKeyboard {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    tags: Vec<String>,
    mcu: String,
    key_count: i64,
    split: bool,
    definition: Value,
}

#[derive(Debug, Serialize)]
pub struct Created {
    slug: String,
    version: u32,
    edit_token: String,
}

pub async fn create(
    State(state): State<SharedState>,
    body: Result<Bytes, BytesRejection>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let input: CreateKeyboard = parse_json(body)?;
    let name = validate::name(&input.name)?;
    let description = validate::description(&input.description)?;
    let author = validate::author(&input.author)?;
    let tags = validate::tags(input.tags)?;
    let mcu = validate::mcu(&input.mcu)?;
    let key_count = validate::key_count(input.key_count)?;
    validate::definition(&input.definition)?;

    let edit_token = auth::new_token()?;
    let edit_token_hash = auth::hash_token(&edit_token);
    let slug = state
        .db(move |connection| {
            let transaction = connection.transaction()?;
            let slug = unused_slug(&transaction, &slug_base(&name))?;
            transaction.execute(
                "INSERT INTO keyboards (slug, name, description, author, tags, mcu, key_count, \
                 split, edit_token_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    slug,
                    name,
                    description,
                    author,
                    Value::from(tags).to_string(),
                    mcu,
                    key_count,
                    input.split,
                    edit_token_hash
                ],
            )?;
            transaction.execute(
                "INSERT INTO keyboard_versions (keyboard_id, version, definition) VALUES (?1, 1, ?2)",
                params![transaction.last_insert_rowid(), input.definition.to_string()],
            )?;
            transaction.commit()?;
            Ok(slug)
        })
        .await?;
    Ok((StatusCode::CREATED, Json(Created { slug, version: 1, edit_token })))
}

/// The URL form of a name: lowercase ASCII letters and digits, with every
/// run of anything else collapsed into one hyphen.
fn slug_base(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() == MAX_SLUG_CHARS {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    // A name written entirely in another script leaves nothing to build on.
    if slug.is_empty() { "keyboard".to_string() } else { slug.to_string() }
}

fn unused_slug(connection: &Connection, base: &str) -> Result<String, ApiError> {
    let mut candidate = base.to_string();
    let mut suffix = 1u32;
    loop {
        let taken: bool = connection.query_row(
            "SELECT EXISTS (SELECT 1 FROM keyboards WHERE slug = ?1)",
            [&candidate],
            |row| row.get(0),
        )?;
        if !taken {
            return Ok(candidate);
        }
        suffix += 1;
        candidate = format!("{base}-{suffix}");
    }
}

#[derive(Debug, Serialize)]
pub struct KeyboardWithDefinition {
    #[serde(flatten)]
    summary: KeyboardSummary,
    definition: Value,
}

pub async fn show(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Json<KeyboardWithDefinition>, ApiError> {
    let keyboard = state
        .db(move |connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE keyboards SET downloads = downloads + 1 WHERE slug = ?1 AND hidden = 0",
                [&slug],
            )?;
            let keyboard = find_visible(&transaction, &slug)?;
            let definition = stored_definition(&transaction, &keyboard, keyboard.summary.version)?;
            transaction.commit()?;
            Ok(KeyboardWithDefinition { summary: keyboard.summary, definition: definition.definition })
        })
        .await?;
    Ok(Json(keyboard))
}

#[derive(Debug, Serialize)]
pub struct VersionSummary {
    version: u32,
    created_at: String,
}

pub async fn versions(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
) -> Result<Json<Items<VersionSummary>>, ApiError> {
    let items = state
        .db(move |connection| {
            let keyboard = find_visible(connection, &slug)?;
            let mut statement = connection.prepare(
                "SELECT version, created_at FROM keyboard_versions WHERE keyboard_id = ?1 \
                 ORDER BY version DESC",
            )?;
            let items = statement
                .query_map([keyboard.id], |row| {
                    Ok(VersionSummary { version: row.get(0)?, created_at: row.get(1)? })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(items)
        })
        .await?;
    Ok(Json(Items { items }))
}

#[derive(Debug, Serialize)]
pub struct VersionWithDefinition {
    slug: String,
    version: u32,
    created_at: String,
    definition: Value,
}

pub async fn version(
    State(state): State<SharedState>,
    Path((slug, version)): Path<(String, String)>,
) -> Result<Json<VersionWithDefinition>, ApiError> {
    let version: u32 = version.parse().map_err(|_| ApiError::not_found("version"))?;
    let found = state
        .db(move |connection| {
            let keyboard = find_visible(connection, &slug)?;
            stored_definition(connection, &keyboard, version)
        })
        .await?;
    Ok(Json(found))
}

fn stored_definition(
    connection: &Connection,
    keyboard: &Keyboard,
    version: u32,
) -> Result<VersionWithDefinition, ApiError> {
    let (definition, created_at): (String, String) = connection
        .query_row(
            "SELECT definition, created_at FROM keyboard_versions \
             WHERE keyboard_id = ?1 AND version = ?2",
            params![keyboard.id, version],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found("version"))?;
    Ok(VersionWithDefinition {
        slug: keyboard.summary.slug.clone(),
        version,
        created_at,
        definition: db::parse_definition(&definition)?,
    })
}

/// Every field is optional: what is absent stays as it is. A new version is
/// stored only when a definition is sent.
#[derive(Debug, Deserialize)]
struct UpdateKeyboard {
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    tags: Option<Vec<String>>,
    mcu: Option<String>,
    key_count: Option<i64>,
    split: Option<bool>,
    definition: Option<Value>,
}

pub async fn update(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<Json<KeyboardSummary>, ApiError> {
    let credentials = Credentials::from_headers(&headers);
    let input: UpdateKeyboard = parse_json(body)?;
    let name = input.name.as_deref().map(validate::name).transpose()?;
    let description = input.description.as_deref().map(validate::description).transpose()?;
    let author = input.author.as_deref().map(validate::author).transpose()?;
    let tags = input.tags.map(validate::tags).transpose()?;
    let mcu = input.mcu.as_deref().map(validate::mcu).transpose()?;
    let key_count = input.key_count.map(validate::key_count).transpose()?;
    if let Some(definition) = &input.definition {
        validate::definition(definition)?;
    }
    let nothing_to_change = name.is_none()
        && description.is_none()
        && author.is_none()
        && tags.is_none()
        && mcu.is_none()
        && key_count.is_none()
        && input.split.is_none()
        && input.definition.is_none();
    if nothing_to_change {
        return Err(ApiError::validation("the request changes nothing"));
    }

    let summary = state
        .db(move |connection| {
            let transaction = connection.transaction()?;
            let keyboard = find_visible(&transaction, &slug)?;
            credentials.require_edit_token(&keyboard.edit_token_hash)?;
            let current = keyboard.summary;
            let mut version = current.version;
            if let Some(definition) = &input.definition {
                version += 1;
                transaction.execute(
                    "INSERT INTO keyboard_versions (keyboard_id, version, definition) \
                     VALUES (?1, ?2, ?3)",
                    params![keyboard.id, version, definition.to_string()],
                )?;
            }
            transaction.execute(
                &format!(
                    "UPDATE keyboards SET name = ?1, description = ?2, author = ?3, tags = ?4, \
                     mcu = ?5, key_count = ?6, split = ?7, version = ?8, updated_at = {NOW} \
                     WHERE id = ?9"
                ),
                params![
                    name.unwrap_or(current.name),
                    description.unwrap_or(current.description),
                    author.unwrap_or(current.author),
                    Value::from(tags.unwrap_or(current.tags)).to_string(),
                    mcu.map(str::to_string).unwrap_or(current.mcu),
                    key_count.unwrap_or(current.key_count),
                    input.split.unwrap_or(current.split),
                    version,
                    keyboard.id
                ],
            )?;
            let updated = find(&transaction, &slug)?.summary;
            transaction.commit()?;
            Ok(updated)
        })
        .await?;
    Ok(Json(summary))
}

pub async fn remove(
    State(state): State<SharedState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let credentials = Credentials::from_headers(&headers);
    let is_admin = credentials.is_admin(state.admin_token_hash.as_deref());
    state
        .db(move |connection| {
            let keyboard = if is_admin {
                find(connection, &slug)?
            } else {
                let keyboard = find_visible(connection, &slug)?;
                credentials.require_edit_token(&keyboard.edit_token_hash)?;
                keyboard
            };
            // Versions, keymaps and reports go with it through ON DELETE CASCADE.
            connection.execute("DELETE FROM keyboards WHERE id = ?1", [keyboard.id])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::slug_base;

    #[test]
    fn slugs_are_lowercase_and_hyphenated() {
        assert_eq!(slug_base("  Lily58 Pro (v2)! "), "lily58-pro-v2");
        assert_eq!(slug_base("a--b__c"), "a-b-c");
    }

    #[test]
    fn slug_falls_back_when_nothing_is_usable() {
        assert_eq!(slug_base("键盘"), "keyboard");
        assert_eq!(slug_base("---"), "keyboard");
    }

    #[test]
    fn slug_is_cut_without_a_trailing_hyphen() {
        let slug = slug_base(&format!("{} tail", "x".repeat(47)));
        assert_eq!(slug, "x".repeat(47));
    }
}
