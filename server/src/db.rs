use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

/// SQL fragment for the current time in the format every timestamp column uses.
pub const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ', 'now')";

/// One entry per schema version, applied in order. Never edit an entry that
/// has shipped; append a new one. `PRAGMA user_version` records how many ran.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE keyboards (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        slug            TEXT NOT NULL UNIQUE,
        name            TEXT NOT NULL,
        description     TEXT NOT NULL,
        author          TEXT NOT NULL,
        tags            TEXT NOT NULL,
        mcu             TEXT NOT NULL,
        key_count       INTEGER NOT NULL,
        split           INTEGER NOT NULL,
        version         INTEGER NOT NULL DEFAULT 1,
        downloads       INTEGER NOT NULL DEFAULT 0,
        hidden          INTEGER NOT NULL DEFAULT 0,
        edit_token_hash TEXT NOT NULL,
        created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE TABLE keyboard_versions (
        keyboard_id INTEGER NOT NULL REFERENCES keyboards(id) ON DELETE CASCADE,
        version     INTEGER NOT NULL,
        definition  TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        PRIMARY KEY (keyboard_id, version)
    );
    CREATE TABLE keymaps (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        keyboard_id     INTEGER NOT NULL REFERENCES keyboards(id) ON DELETE CASCADE,
        name            TEXT NOT NULL,
        description     TEXT NOT NULL,
        author          TEXT NOT NULL,
        definition      TEXT NOT NULL,
        edit_token_hash TEXT NOT NULL,
        created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX keymaps_by_keyboard ON keymaps(keyboard_id);
    CREATE TABLE reports (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        keyboard_id INTEGER NOT NULL REFERENCES keyboards(id) ON DELETE CASCADE,
        reason      TEXT NOT NULL,
        created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX reports_by_keyboard ON reports(keyboard_id);
"];

pub fn open(path: &Path) -> Result<Connection, String> {
    let describe = |error: rusqlite::Error| format!("database {}: {error}", path.display());
    let mut connection = Connection::open(path).map_err(describe)?;
    connection.busy_timeout(Duration::from_secs(5)).map_err(describe)?;
    connection.pragma_update(None, "journal_mode", "WAL").map_err(describe)?;
    // With WAL this cannot corrupt the database; a power cut may lose the
    // last few commits, which is acceptable here and avoids a sync per write.
    connection.pragma_update(None, "synchronous", "NORMAL").map_err(describe)?;
    connection.pragma_update(None, "foreign_keys", true).map_err(describe)?;
    migrate(&mut connection).map_err(|error| format!("database {}: {error}", path.display()))?;
    Ok(connection)
}

fn migrate(connection: &mut Connection) -> Result<(), String> {
    let applied: usize = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if applied > MIGRATIONS.len() {
        return Err(format!(
            "schema version {applied} is newer than this server understands ({})",
            MIGRATIONS.len()
        ));
    }
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(applied) {
        let version = index + 1;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        transaction
            .execute_batch(migration)
            .and_then(|()| transaction.pragma_update(None, "user_version", version))
            .and_then(|()| transaction.commit())
            .map_err(|error| format!("migration to schema version {version} failed: {error}"))?;
    }
    Ok(())
}

/// Turns user text into a `LIKE` pattern that matches it literally anywhere.
/// Use with `ESCAPE '\'`.
pub fn like_pattern(text: &str) -> String {
    let mut pattern = String::with_capacity(text.len() + 2);
    pattern.push('%');
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern.push('%');
    pattern
}

/// Definitions are validated before they are stored, so a stored one that
/// does not parse means the database was damaged.
pub fn parse_definition(text: &str) -> Result<serde_json::Value, crate::error::ApiError> {
    serde_json::from_str(text).map_err(crate::error::ApiError::internal)
}
