use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path as FilePath;
use std::sync::Mutex;
use std::time::SystemTime;

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::auth::hex;
use crate::error::ApiError;
use crate::pagination::Items;
use crate::{validate, SharedState};

const EXTENSIONS: [&str; 4] = ["uf2", "hex", "bin", "zip"];

/// Digests of the files seen at the last listing, so an unchanged image is
/// not read and hashed again on every request.
#[derive(Default)]
pub struct HashCache(Mutex<HashMap<String, CachedHash>>);

struct CachedHash {
    size: u64,
    modified: Option<SystemTime>,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub struct FirmwareImage {
    name: String,
    mcu: &'static str,
    version: String,
    size: u64,
    sha256: String,
    url: String,
}

/// Splits a name like `jmk-rp2040-0.1.0.uf2` into microcontroller and
/// version. Only names of this shape are listed or served, which also keeps
/// path separators and other files in the directory out of reach.
fn parse_name(name: &str) -> Option<(&'static str, &str)> {
    let plain = |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+');
    if !name.bytes().all(plain) {
        return None;
    }
    let (mcu, rest) = name.strip_prefix("jmk-")?.split_once('-')?;
    let mcu = validate::MCUS.into_iter().find(|known| *known == mcu)?;
    let (version, extension) = rest.rsplit_once('.')?;
    (!version.is_empty() && EXTENSIONS.contains(&extension)).then_some((mcu, version))
}

pub async fn list(State(state): State<SharedState>) -> Result<Json<Items<FirmwareImage>>, ApiError> {
    let items = tokio::task::spawn_blocking(move || match &state.config.firmware_dir {
        Some(directory) => list_directory(directory, &state.firmware_hashes),
        None => Ok(Vec::new()),
    })
    .await
    .map_err(ApiError::internal)??;
    Ok(Json(Items { items }))
}

fn list_directory(directory: &FilePath, cache: &HashCache) -> Result<Vec<FirmwareImage>, ApiError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(ApiError::internal(error)),
    };
    let mut cache = cache.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut still_present = HashMap::new();
    let mut images = Vec::new();
    for entry in entries {
        let entry = entry.map_err(ApiError::internal)?;
        let Ok(name) = entry.file_name().into_string() else { continue };
        let Some((mcu, version)) = parse_name(&name) else { continue };
        let path = entry.path();
        // symlink_metadata: a link is not a regular file, so links are skipped
        // and cannot point the server at files outside the directory.
        let metadata = fs::symlink_metadata(&path).map_err(ApiError::internal)?;
        if !metadata.is_file() {
            continue;
        }
        let (size, modified) = (metadata.len(), metadata.modified().ok());
        let cached = cache
            .remove(&name)
            .filter(|cached| cached.size == size && cached.modified == modified && modified.is_some());
        let cached = match cached {
            Some(cached) => cached,
            None => {
                let contents = fs::read(&path).map_err(ApiError::internal)?;
                CachedHash { size, modified, sha256: hex(&Sha256::digest(&contents)) }
            }
        };
        images.push(FirmwareImage {
            url: format!("/api/v1/firmware/{name}"),
            mcu,
            version: version.to_string(),
            size,
            sha256: cached.sha256.clone(),
            name: name.clone(),
        });
        still_present.insert(name, cached);
    }
    *cache = still_present;
    images.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(images)
}

pub async fn download(
    State(state): State<SharedState>,
    Path(name): Path<String>,
) -> Result<Response, ApiError> {
    if parse_name(&name).is_none() {
        return Err(ApiError::bad_request(
            "invalid_name",
            "firmware names look like jmk-rp2040-0.1.0.uf2",
        ));
    }
    let Some(directory) = state.config.firmware_dir.clone() else {
        return Err(ApiError::not_found("firmware image"));
    };
    let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
        .map_err(ApiError::internal)?;
    let contents = tokio::task::spawn_blocking(move || {
        let path = directory.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => fs::read(&path).map_err(ApiError::internal),
            Ok(_) => Err(ApiError::not_found("firmware image")),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Err(ApiError::not_found("firmware image"))
            }
            Err(error) => Err(ApiError::internal(error)),
        }
    })
    .await
    .map_err(ApiError::internal)??;
    let headers = [
        (header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream")),
        (header::CONTENT_DISPOSITION, disposition),
    ];
    Ok((headers, contents).into_response())
}

#[cfg(test)]
mod tests {
    use super::parse_name;

    #[test]
    fn parses_microcontroller_and_version() {
        assert_eq!(parse_name("jmk-rp2040-0.1.0.uf2"), Some(("rp2040", "0.1.0")));
        assert_eq!(parse_name("jmk-nrf52840-1.2.3-rc1.zip"), Some(("nrf52840", "1.2.3-rc1")));
    }

    #[test]
    fn refuses_other_names() {
        for name in [
            "notes.txt",
            "jmk-avr-0.1.0.uf2",
            "jmk-rp2040-.uf2",
            "jmk-rp2040-0.1.0.exe",
            "../jmk-rp2040-0.1.0.uf2",
            "jmk-rp2040-0.1.0/../../secret.uf2",
            "jmk-rp2040-0.1.0\\x.uf2",
        ] {
            assert_eq!(parse_name(name), None, "{name}");
        }
    }
}
