//! Drives the whole router in-process against a temporary database.

use std::fs;
use std::path::Path;
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use http_body_util::BodyExt;
use jmk_registry::{app, AppState, Config, MAX_BODY_BYTES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token-0123456789";

struct TestServer {
    router: Router,
    directory: TempDir,
}

impl TestServer {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(adjust: impl FnOnce(&mut Config)) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut config = Config {
            db_path: directory.path().join("registry.db"),
            admin_token: Some(ADMIN_TOKEN.to_string()),
            // High enough that only the rate limit test reaches it.
            write_limit: 10_000,
            ..Config::default()
        };
        adjust(&mut config);
        let router = app(AppState::open(config).unwrap());
        Self { router, directory }
    }

    async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let body = match body {
            Some(json) => Body::from(json.to_string()),
            None => Body::empty(),
        };
        let response = self.send(request.body(body).unwrap()).await;
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
        (status, json)
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        self.call(Method::GET, path, None, None).await
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(Method::POST, path, None, Some(body)).await
    }

    /// Publishes a keyboard and returns its slug and edit token.
    async fn publish(&self, keyboard: Value) -> (String, String) {
        let (status, body) = self.post("/api/v1/keyboards", keyboard).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        (text(&body["slug"]), text(&body["edit_token"]))
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_else(|| panic!("expected a string, got {value}")).to_string()
}

fn keyboard(name: &str) -> Value {
    json!({
        "name": name,
        "description": "A hand-wired board",
        "author": "ada",
        "tags": ["ortho", "hand-wired"],
        "mcu": "rp2040",
        "key_count": 48,
        "split": false,
        "definition": { "schema": 1, "matrix": { "rows": 4, "cols": 12 } }
    })
}

fn error_code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or_else(|| panic!("not an error body: {body}"))
}

fn slugs(listing: &Value) -> Vec<&str> {
    listing["items"].as_array().unwrap().iter().map(|item| item["slug"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn healthz_answers() {
    let server = TestServer::new();
    let (status, body) = server.get("/healthz").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn unknown_routes_and_methods_answer_in_the_error_format() {
    let server = TestServer::new();
    let (status, body) = server.get("/api/v1/nothing-here").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");

    let (status, body) = server.call(Method::DELETE, "/api/v1/keyboards", None, None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(error_code(&body), "method_not_allowed");
}

#[tokio::test]
async fn create_then_get_returns_the_definition_and_counts_downloads() {
    let server = TestServer::new();
    let (status, created) = server.post("/api/v1/keyboards", keyboard("Plank 48")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["slug"], "plank-48");
    assert_eq!(created["version"], 1);
    assert_eq!(text(&created["edit_token"]).len(), 64);

    let (status, first) = server.get("/api/v1/keyboards/plank-48").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["name"], "Plank 48");
    assert_eq!(first["description"], "A hand-wired board");
    assert_eq!(first["author"], "ada");
    assert_eq!(first["tags"], json!(["ortho", "hand-wired"]));
    assert_eq!(first["mcu"], "rp2040");
    assert_eq!(first["key_count"], 48);
    assert_eq!(first["split"], false);
    assert_eq!(first["version"], 1);
    assert_eq!(first["downloads"], 1);
    assert_eq!(first["definition"], json!({ "schema": 1, "matrix": { "rows": 4, "cols": 12 } }));
    assert!(first.get("edit_token").is_none() && first.get("edit_token_hash").is_none());

    let (_, second) = server.get("/api/v1/keyboards/plank-48").await;
    assert_eq!(second["downloads"], 2);

    let (status, body) = server.get("/api/v1/keyboards/no-such-board").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
}

#[tokio::test]
async fn definition_keeps_its_key_order() {
    let server = TestServer::new();
    let mut board = keyboard("Ordered");
    board["definition"] = json!({ "zeta": 1, "schema": 3, "alpha": [3, 1, 2] });
    let request = Request::post("/api/v1/keyboards").body(Body::from(board.to_string())).unwrap();
    assert_eq!(server.send(request).await.status(), StatusCode::CREATED);

    let response = server.send(Request::get("/api/v1/keyboards/ordered").body(Body::empty()).unwrap()).await;
    let raw = response.into_body().collect().await.unwrap().to_bytes();
    let raw = String::from_utf8(raw.to_vec()).unwrap();
    assert!(raw.contains(r#""definition":{"zeta":1,"schema":3,"alpha":[3,1,2]}"#), "{raw}");
}

#[tokio::test]
async fn slugs_are_made_unique_with_a_numeric_suffix() {
    let server = TestServer::new();
    assert_eq!(server.publish(keyboard("My Board!")).await.0, "my-board");
    assert_eq!(server.publish(keyboard("my board")).await.0, "my-board-2");
    assert_eq!(server.publish(keyboard("MY  BOARD")).await.0, "my-board-3");
    assert_eq!(server.publish(keyboard("键盘")).await.0, "keyboard");
}

#[tokio::test]
async fn list_filters_sorts_and_paginates() {
    let server = TestServer::new();
    let mut alpha = keyboard("Alpha Split");
    alpha["split"] = json!(true);
    alpha["mcu"] = json!("nrf52840");
    alpha["tags"] = json!(["split", "wireless"]);
    alpha["author"] = json!("grace");
    let mut charlie = keyboard("charlie pad");
    charlie["mcu"] = json!("rp2350");
    charlie["tags"] = json!(["macro-pad"]);
    charlie["description"] = json!("Nine keys and 100% fun");
    server.publish(alpha).await;
    server.publish(keyboard("Bravo")).await;
    server.publish(charlie).await;
    // Bravo becomes the most downloaded.
    server.get("/api/v1/keyboards/bravo").await;
    server.get("/api/v1/keyboards/bravo").await;
    server.get("/api/v1/keyboards/charlie-pad").await;

    let (status, all) = server.get("/api/v1/keyboards").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["total"], 3);
    assert_eq!(all["page"], 1);
    assert_eq!(all["per_page"], 20);
    let summary = &all["items"][0];
    for field in [
        "slug", "name", "description", "author", "tags", "mcu", "key_count", "split", "version",
        "downloads", "updated_at",
    ] {
        assert!(summary.get(field).is_some(), "summary lacks {field}");
    }
    assert!(summary.get("definition").is_none());

    let (_, by_name) = server.get("/api/v1/keyboards?sort=name").await;
    assert_eq!(slugs(&by_name), ["alpha-split", "bravo", "charlie-pad"]);
    let (_, popular) = server.get("/api/v1/keyboards?sort=popular").await;
    assert_eq!(slugs(&popular), ["bravo", "charlie-pad", "alpha-split"]);
    let (_, recent) = server.get("/api/v1/keyboards?sort=recent").await;
    assert_eq!(recent["total"], 3);

    let (_, split) = server.get("/api/v1/keyboards?split=true").await;
    assert_eq!(slugs(&split), ["alpha-split"]);
    let (_, unsplit) = server.get("/api/v1/keyboards?split=false&sort=name").await;
    assert_eq!(slugs(&unsplit), ["bravo", "charlie-pad"]);
    let (_, by_mcu) = server.get("/api/v1/keyboards?mcu=rp2350").await;
    assert_eq!(slugs(&by_mcu), ["charlie-pad"]);
    let (_, by_tag) = server.get("/api/v1/keyboards?tag=wireless").await;
    assert_eq!(slugs(&by_tag), ["alpha-split"]);
    // A tag filter matches whole tags, not parts of them.
    let (_, partial_tag) = server.get("/api/v1/keyboards?tag=wire").await;
    assert_eq!(partial_tag["total"], 0);

    // Empty parameters, as a form would send them, are ignored.
    let (status, empty) = server.get("/api/v1/keyboards?q=&tag=&mcu=&split=&sort=&page=&per_page=").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["total"], 3);

    let (_, first_page) = server.get("/api/v1/keyboards?sort=name&per_page=2").await;
    assert_eq!(slugs(&first_page), ["alpha-split", "bravo"]);
    assert_eq!(first_page["total"], 3);
    let (_, second_page) = server.get("/api/v1/keyboards?sort=name&per_page=2&page=2").await;
    assert_eq!(slugs(&second_page), ["charlie-pad"]);
    assert_eq!(second_page["page"], 2);

    for query in ["sort=newest", "mcu=avr", "split=maybe", "page=0", "per_page=101", "page=x"] {
        let (status, body) = server.get(&format!("/api/v1/keyboards?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(error_code(&body), "invalid_query", "{query}");
    }
}

#[tokio::test]
async fn search_covers_name_description_author_and_tags() {
    let server = TestServer::new();
    let mut alpha = keyboard("Alpha Split");
    alpha["author"] = json!("grace");
    alpha["tags"] = json!(["wireless"]);
    let mut charlie = keyboard("charlie pad");
    charlie["description"] = json!("Nine keys and 100% fun");
    charlie["tags"] = json!(["macro-pad"]);
    server.publish(alpha).await;
    server.publish(charlie).await;

    let found = |query: &'static str| {
        let server = &server;
        async move {
            let (status, body) = server.get(&format!("/api/v1/keyboards?sort=name&q={query}")).await;
            assert_eq!(status, StatusCode::OK, "{query}");
            slugs(&body).into_iter().map(str::to_string).collect::<Vec<_>>()
        }
    };
    assert_eq!(found("ALPHA").await, ["alpha-split"]); // name, any case
    assert_eq!(found("nine").await, ["charlie-pad"]); // description
    assert_eq!(found("grace").await, ["alpha-split"]); // author
    assert_eq!(found("macro").await, ["charlie-pad"]); // tag
    assert_eq!(found("keys").await, ["charlie-pad"]);
    assert_eq!(found("a").await, ["alpha-split", "charlie-pad"]);
    assert_eq!(found("alpha%20grace").await, ["alpha-split"]); // all terms must match
    assert_eq!(found("alpha%20nine").await, Vec::<String>::new());
    // LIKE wildcards in the query are taken literally.
    assert_eq!(found("100%25").await, ["charlie-pad"]);
    assert_eq!(found("%25").await, ["charlie-pad"]);
    assert_eq!(found("_").await, Vec::<String>::new());
}

#[tokio::test]
async fn put_stores_a_new_version_and_keeps_old_ones_readable() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Versioned")).await;
    let path = format!("/api/v1/keyboards/{slug}");

    let change = json!({
        "description": "Now with a number row",
        "key_count": 60,
        "tags": ["ortho"],
        "definition": { "schema": 2, "matrix": { "rows": 5, "cols": 12 } }
    });
    let (status, updated) = server.call(Method::PUT, &path, Some(&token), Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["version"], 2);
    assert_eq!(updated["key_count"], 60);
    assert_eq!(updated["name"], "Versioned");
    assert_eq!(updated["slug"], slug);

    let (_, latest) = server.get(&path).await;
    assert_eq!(latest["version"], 2);
    assert_eq!(latest["description"], "Now with a number row");
    assert_eq!(latest["tags"], json!(["ortho"]));
    assert_eq!(latest["definition"]["schema"], 2);

    let (status, versions) = server.get(&format!("{path}/versions")).await;
    assert_eq!(status, StatusCode::OK);
    let numbers: Vec<_> = versions["items"].as_array().unwrap().iter().map(|v| v["version"].clone()).collect();
    assert_eq!(numbers, [json!(2), json!(1)]);
    assert!(versions["items"][0]["created_at"].is_string());

    let (status, first) = server.get(&format!("{path}/versions/1")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["version"], 1);
    assert_eq!(first["definition"], json!({ "schema": 1, "matrix": { "rows": 4, "cols": 12 } }));
    let (_, second) = server.get(&format!("{path}/versions/2")).await;
    assert_eq!(second["definition"]["matrix"]["rows"], 5);

    for missing in ["3", "0", "latest", "-1"] {
        let (status, _) = server.get(&format!("{path}/versions/{missing}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
    }

    // Changing only metadata does not create a version; the slug stays put.
    let rename = json!({ "name": "Renamed" });
    let (status, renamed) = server.call(Method::PUT, &path, Some(&token), Some(rename)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "Renamed");
    assert_eq!(renamed["version"], 2);
    assert_eq!(renamed["slug"], slug);

    // Reading versions does not count as a download; the two gets above do.
    assert_eq!(renamed["downloads"], 1);
}

#[tokio::test]
async fn writes_need_the_right_token() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Guarded")).await;
    let (_, other_token) = server.publish(keyboard("Other")).await;
    let path = format!("/api/v1/keyboards/{slug}");
    let change = json!({ "description": "taken over" });

    let (status, body) = server.call(Method::PUT, &path, None, Some(change.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_token");
    let (status, body) = server.call(Method::PUT, &path, Some("not-the-token"), Some(change.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "wrong_token");
    // Another keyboard's token is no better.
    let (status, _) = server.call(Method::PUT, &path, Some(&other_token), Some(change.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The admin token moderates; it does not edit.
    let (status, _) = server.call(Method::PUT, &path, Some(ADMIN_TOKEN), Some(change)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = server.call(Method::DELETE, &path, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_token");
    let (status, _) = server.call(Method::DELETE, &path, Some(&other_token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // A request without a token is told how to authenticate.
    let request = Request::delete(&path).body(Body::empty()).unwrap();
    let response = server.send(request).await;
    assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");

    let (_, untouched) = server.get(&path).await;
    assert_eq!(untouched["description"], "A hand-wired board");
    assert_eq!(untouched["version"], 1);

    let (status, _) = server.call(Method::DELETE, &path, Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = server.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.get(&format!("{path}/versions/1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_can_delete_any_keyboard() {
    let server = TestServer::new();
    let (slug, _) = server.publish(keyboard("Spam")).await;
    let path = format!("/api/v1/keyboards/{slug}");
    let (status, _) = server.call(Method::DELETE, &path, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = server.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invalid_keyboards_are_rejected() {
    let server = TestServer::new();
    type Break = Box<dyn Fn(&mut Value)>;
    let invalid: Vec<(&str, Break)> = vec![
        ("empty name", Box::new(|k| k["name"] = json!("   "))),
        ("long name", Box::new(|k| k["name"] = json!("n".repeat(65)))),
        ("missing name", Box::new(|k| { k.as_object_mut().unwrap().remove("name"); })),
        ("name of the wrong type", Box::new(|k| k["name"] = json!(7))),
        ("long description", Box::new(|k| k["description"] = json!("d".repeat(2001)))),
        ("long author", Box::new(|k| k["author"] = json!("a".repeat(65)))),
        ("too many tags", Box::new(|k| k["tags"] = json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"]))),
        ("long tag", Box::new(|k| k["tags"] = json!(["t".repeat(25)]))),
        ("uppercase tag", Box::new(|k| k["tags"] = json!(["Ortho"]))),
        ("tag with a space", Box::new(|k| k["tags"] = json!(["hand wired"]))),
        ("empty tag", Box::new(|k| k["tags"] = json!([""]))),
        ("unknown mcu", Box::new(|k| k["mcu"] = json!("atmega32u4"))),
        ("missing mcu", Box::new(|k| { k.as_object_mut().unwrap().remove("mcu"); })),
        ("fractional key_count", Box::new(|k| k["key_count"] = json!(4.5))),
        ("zero key_count", Box::new(|k| k["key_count"] = json!(0))),
        ("split of the wrong type", Box::new(|k| k["split"] = json!("yes"))),
        ("missing definition", Box::new(|k| { k.as_object_mut().unwrap().remove("definition"); })),
        ("definition that is an array", Box::new(|k| k["definition"] = json!([1, 2]))),
        ("definition without schema", Box::new(|k| k["definition"] = json!({ "matrix": {} }))),
        ("text schema", Box::new(|k| k["definition"] = json!({ "schema": "1" }))),
        ("fractional schema", Box::new(|k| k["definition"] = json!({ "schema": 1.5 }))),
    ];
    for (case, break_it) in invalid {
        let mut board = keyboard("Valid Name");
        break_it(&mut board);
        let (status, body) = server.post("/api/v1/keyboards", board).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{case}: {body}");
        assert_eq!(error_code(&body), "validation_failed", "{case}");
        assert!(body["error"]["message"].is_string(), "{case}");
    }
    let (_, listing) = server.get("/api/v1/keyboards").await;
    assert_eq!(listing["total"], 0);

    // The limits themselves are allowed.
    let mut board = keyboard(&"n".repeat(64));
    board["description"] = json!("d".repeat(2000));
    board["author"] = json!("é".repeat(64));
    board["tags"] = json!(["a", "b", "c", "d", "e", "f", "g", &"t".repeat(24)]);
    let (status, body) = server.post("/api/v1/keyboards", board).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // Only name, mcu, key_count, split and definition are required.
    let minimal = json!({ "name": "Bare", "mcu": "rp2040", "key_count": 1, "split": false, "definition": { "schema": 1 } });
    let (status, _) = server.post("/api/v1/keyboards", minimal).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, bare) = server.get("/api/v1/keyboards/bare").await;
    assert_eq!(bare["description"], "");
    assert_eq!(bare["tags"], json!([]));
}

#[tokio::test]
async fn malformed_json_is_a_bad_request() {
    let server = TestServer::new();
    for body in ["{ \"name\": ", "", "not json"] {
        let request = Request::post("/api/v1/keyboards").body(Body::from(body)).unwrap();
        let response = server.send(request).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body:?}");
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error_code(&json), "invalid_json");
    }
}

#[tokio::test]
async fn invalid_updates_are_rejected_and_change_nothing() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Stable")).await;
    let path = format!("/api/v1/keyboards/{slug}");
    for change in [
        json!({}),
        json!({ "name": "" }),
        json!({ "mcu": "z80" }),
        json!({ "tags": ["Bad Tag"] }),
        json!({ "definition": { "no": "schema" } }),
        json!({ "description": "fine", "definition": "text" }),
    ] {
        let (status, body) = server.call(Method::PUT, &path, Some(&token), Some(change.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{change}");
        assert_eq!(error_code(&body), "validation_failed", "{change}");
    }
    let (_, unchanged) = server.get(&path).await;
    assert_eq!(unchanged["version"], 1);
    assert_eq!(unchanged["description"], "A hand-wired board");
}

#[tokio::test]
async fn bodies_over_256_kib_are_refused() {
    let server = TestServer::new();
    let mut board = keyboard("Huge");
    board["definition"]["padding"] = json!("x".repeat(MAX_BODY_BYTES));
    let oversized = board.to_string();

    // Once with the length announced up front, once streamed without it.
    let announced = Request::post("/api/v1/keyboards")
        .header(header::CONTENT_LENGTH, oversized.len())
        .body(Body::from(oversized.clone()))
        .unwrap();
    let streamed = Request::post("/api/v1/keyboards")
        .body(Body::new(ChunkedBody(oversized.into_bytes().chunks(16 * 1024).map(<[u8]>::to_vec).collect())))
        .unwrap();
    for request in [announced, streamed] {
        let response = server.send(request).await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(error_code(&json), "body_too_large");
    }

    // A large definition under the limit is fine.
    let mut board = keyboard("Large");
    board["definition"]["padding"] = json!("x".repeat(MAX_BODY_BYTES - 1024));
    assert!(board.to_string().len() <= MAX_BODY_BYTES);
    let (status, _) = server.post("/api/v1/keyboards", board).await;
    assert_eq!(status, StatusCode::CREATED);
}

/// A body that arrives in pieces and does not announce its length.
struct ChunkedBody(std::collections::VecDeque<Vec<u8>>);

impl http_body::Body for ChunkedBody {
    type Data = axum::body::Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let chunk = self.0.pop_front();
        std::task::Poll::Ready(chunk.map(|chunk| Ok(http_body::Frame::data(chunk.into()))))
    }
}

#[tokio::test]
async fn keymaps_can_be_shared_edited_and_deleted() {
    let server = TestServer::new();
    let (slug, keyboard_token) = server.publish(keyboard("Host Board")).await;
    let list_path = format!("/api/v1/keyboards/{slug}/keymaps");

    let (status, empty) = server.get(&list_path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(empty["items"], json!([]));

    let keymap = json!({
        "name": "Colemak-DH",
        "description": "Home row mods",
        "author": "lin",
        "definition": { "schema": 1, "layers": [["Q", "W", "F"]] }
    });
    let (status, created) = server.post(&list_path, keymap).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let id = created["id"].as_i64().unwrap();
    let token = text(&created["edit_token"]);
    let path = format!("/api/v1/keymaps/{id}");

    let (_, listed) = server.get(&list_path).await;
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], id);
    assert_eq!(listed["items"][0]["name"], "Colemak-DH");
    assert_eq!(listed["items"][0]["keyboard"], slug);
    assert!(listed["items"][0].get("definition").is_none());

    let (status, fetched) = server.get(&path).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["author"], "lin");
    assert_eq!(fetched["definition"]["layers"][0][2], "F");

    let change = json!({ "name": "Colemak-DH v2", "definition": { "schema": 1, "layers": [["Q", "W", "P"]] } });
    let (status, body) = server.call(Method::PUT, &path, None, Some(change.clone())).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_token");
    // The keyboard's token does not open its keymaps.
    let (status, body) = server.call(Method::PUT, &path, Some(&keyboard_token), Some(change.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "wrong_token");
    let (status, updated) = server.call(Method::PUT, &path, Some(&token), Some(change)).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["name"], "Colemak-DH v2");
    assert_eq!(updated["description"], "Home row mods");
    assert_eq!(updated["definition"]["layers"][0][2], "P");

    for invalid in [json!({}), json!({ "name": "" }), json!({ "definition": [] })] {
        let (status, _) = server.call(Method::PUT, &path, Some(&token), Some(invalid)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let (status, _) = server.post(&list_path, json!({ "name": "No definition" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = server.post("/api/v1/keyboards/absent/keymaps", json!({ "name": "x", "definition": { "schema": 1 } })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = server.call(Method::DELETE, &path, Some("wrong"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = server.call(Method::DELETE, &path, Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = server.get(&path).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.get("/api/v1/keymaps/not-a-number").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_keyboard_removes_its_keymaps_and_admin_can_delete_keymaps() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Parent")).await;
    let list_path = format!("/api/v1/keyboards/{slug}/keymaps");
    let keymap = json!({ "name": "One", "definition": { "schema": 1 } });
    let (_, first) = server.post(&list_path, keymap.clone()).await;
    let (_, second) = server.post(&list_path, keymap).await;

    let first_path = format!("/api/v1/keymaps/{}", first["id"]);
    let (status, _) = server.call(Method::DELETE, &first_path, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = server.call(Method::DELETE, &format!("/api/v1/keyboards/{slug}"), Some(&token), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = server.get(&format!("/api/v1/keymaps/{}", second["id"])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reports_reach_the_admin_and_hidden_keyboards_disappear() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Dubious")).await;
    server.publish(keyboard("Fine")).await;
    let path = format!("/api/v1/keyboards/{slug}");
    let (_, keymap) = server.post(&format!("{path}/keymaps"), json!({ "name": "km", "definition": { "schema": 1 } })).await;
    let keymap_path = format!("/api/v1/keymaps/{}", keymap["id"]);

    let (status, _) = server.post(&format!("{path}/report"), json!({ "reason": "Not a keyboard, an advert" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    for invalid in [json!({ "reason": "  " }), json!({}), json!({ "reason": "r".repeat(1001) })] {
        let (status, _) = server.post(&format!("{path}/report"), invalid).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let (status, _) = server.post("/api/v1/keyboards/absent/report", json!({ "reason": "x" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = server.get("/api/v1/admin/reports").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "missing_token");
    let (status, _) = server.call(Method::GET, "/api/v1/admin/reports", Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, reports) = server.call(Method::GET, "/api/v1/admin/reports", Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reports["total"], 1);
    assert_eq!(reports["items"][0]["keyboard"], slug);
    assert_eq!(reports["items"][0]["reason"], "Not a keyboard, an advert");
    assert_eq!(reports["items"][0]["keyboard_hidden"], false);

    let hide = format!("/api/v1/admin/keyboards/{slug}/hide");
    let (status, _) = server.call(Method::POST, &hide, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = server.call(Method::POST, &hide, Some(&token), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = server.call(Method::POST, "/api/v1/admin/keyboards/absent/hide", Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, hidden) = server.call(Method::POST, &hide, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(hidden["hidden"], true);

    let (_, listing) = server.get("/api/v1/keyboards").await;
    assert_eq!(slugs(&listing), ["fine"]);
    for hidden_path in [
        path.clone(),
        format!("{path}/versions"),
        format!("{path}/versions/1"),
        format!("{path}/keymaps"),
        keymap_path.clone(),
    ] {
        let (status, _) = server.get(&hidden_path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{hidden_path}");
    }
    // Its owner cannot bring it back by editing, and the slug stays taken.
    let (status, _) = server.call(Method::PUT, &path, Some(&token), Some(json!({ "name": "Honest" }))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(server.publish(keyboard("Dubious")).await.0, "dubious-2");

    let (_, reports) = server.call(Method::GET, "/api/v1/admin/reports", Some(ADMIN_TOKEN), None).await;
    assert_eq!(reports["items"][0]["keyboard_hidden"], true);

    let unhide = format!("/api/v1/admin/keyboards/{slug}/unhide");
    let (status, _) = server.call(Method::POST, &unhide, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, restored) = server.get(&path).await;
    assert_eq!(status, StatusCode::OK);
    // Requests refused while hidden were not counted as downloads.
    assert_eq!(restored["downloads"], 1);
    let (status, _) = server.get(&keymap_path).await;
    assert_eq!(status, StatusCode::OK);

    let report_path = format!("/api/v1/admin/reports/{}", reports["items"][0]["id"]);
    let (status, _) = server.call(Method::DELETE, &report_path, None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = server.call(Method::DELETE, &report_path, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, reports) = server.call(Method::GET, "/api/v1/admin/reports", Some(ADMIN_TOKEN), None).await;
    assert_eq!(reports["total"], 0);
}

#[tokio::test]
async fn admin_can_delete_a_hidden_keyboard() {
    let server = TestServer::new();
    let (slug, token) = server.publish(keyboard("Gone")).await;
    let path = format!("/api/v1/keyboards/{slug}");
    server.call(Method::POST, &format!("/api/v1/admin/keyboards/{slug}/hide"), Some(ADMIN_TOKEN), None).await;
    let (status, _) = server.call(Method::DELETE, &path, Some(&token), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.call(Method::DELETE, &path, Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(server.publish(keyboard("Gone")).await.0, "gone");
}

#[tokio::test]
async fn moderation_is_off_without_an_admin_token() {
    let server = TestServer::with(|config| config.admin_token = None);
    let (slug, token) = server.publish(keyboard("Board")).await;
    for candidate in [None, Some(""), Some(ADMIN_TOKEN), Some(token.as_str())] {
        let (status, body) = server.call(Method::GET, "/api/v1/admin/reports", candidate, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error_code(&body), "admin_disabled");
        let hide = format!("/api/v1/admin/keyboards/{slug}/hide");
        let (status, _) = server.call(Method::POST, &hide, candidate, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    let (status, _) = server.call(Method::DELETE, &format!("/api/v1/keyboards/{slug}"), Some(ADMIN_TOKEN), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

fn write(path: &Path, contents: &[u8]) {
    fs::write(path, contents).unwrap();
}

#[tokio::test]
async fn firmware_is_listed_and_served() {
    let mut firmware = None;
    let server = TestServer::with(|config| {
        let directory = config.db_path.parent().unwrap().join("firmware");
        fs::create_dir(&directory).unwrap();
        config.firmware_dir = Some(directory.clone());
        firmware = Some(directory);
    });
    let firmware = firmware.unwrap();
    write(&firmware.join("jmk-rp2040-0.1.0.uf2"), b"rp2040 image");
    write(&firmware.join("jmk-nrf52840-0.2.0-rc1.uf2"), b"nrf image, longer");
    write(&firmware.join("README.txt"), b"not firmware");
    write(&firmware.join("jmk-z80-0.1.0.uf2"), b"unknown chip");
    fs::create_dir(firmware.join("jmk-rp2350-9.9.9.uf2")).unwrap();
    let secret = server.directory.path().join("jmk-rp2040-6.6.6.uf2");
    write(&secret, b"outside the firmware directory");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&secret, firmware.join("jmk-rp2040-6.6.6.uf2")).unwrap();

    let (status, listing) = server.get("/api/v1/firmware").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        listing["items"],
        json!([
            {
                "name": "jmk-nrf52840-0.2.0-rc1.uf2",
                "mcu": "nrf52840",
                "version": "0.2.0-rc1",
                "size": 17,
                "sha256": format!("{:x}", Sha256::digest(b"nrf image, longer")),
                "url": "/api/v1/firmware/jmk-nrf52840-0.2.0-rc1.uf2"
            },
            {
                "name": "jmk-rp2040-0.1.0.uf2",
                "mcu": "rp2040",
                "version": "0.1.0",
                "size": 12,
                "sha256": format!("{:x}", Sha256::digest(b"rp2040 image")),
                "url": "/api/v1/firmware/jmk-rp2040-0.1.0.uf2"
            }
        ])
    );

    let request = Request::get("/api/v1/firmware/jmk-rp2040-0.1.0.uf2").body(Body::empty()).unwrap();
    let response = server.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/octet-stream");
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "12");
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"rp2040 image");

    // A replaced image is hashed again rather than answered from the cache.
    write(&firmware.join("jmk-rp2040-0.1.0.uf2"), b"rp2040 image, rebuilt");
    let (_, listing) = server.get("/api/v1/firmware").await;
    assert_eq!(listing["items"][1]["sha256"], format!("{:x}", Sha256::digest(b"rp2040 image, rebuilt")));

    let (status, _) = server.get("/api/v1/firmware/jmk-rp2040-7.7.7.uf2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Neither a directory nor a link out of the directory is served.
    let (status, _) = server.get("/api/v1/firmware/jmk-rp2350-9.9.9.uf2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = server.get("/api/v1/firmware/jmk-rp2040-6.6.6.uf2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn firmware_download_rejects_path_traversal() {
    let server = TestServer::with(|config| {
        let directory = config.db_path.parent().unwrap().join("firmware");
        fs::create_dir(&directory).unwrap();
        config.firmware_dir = Some(directory);
    });
    write(&server.directory.path().join("jmk-rp2040-6.6.6.uf2"), b"outside");
    write(&server.directory.path().join("firmware/jmk-rp2040-0.1.0.uf2"), b"inside");

    for name in [
        "..%2Fjmk-rp2040-6.6.6.uf2",
        "..%2Fregistry.db",
        "%2Fetc%2Fpasswd",
        "..%5C..%5Cregistry.db",
        "jmk-rp2040-0.1.0.uf2%2F..%2F..%2Fregistry.db",
        "jmk-rp2040-..%2Fjmk-rp2040-6.6.6.uf2",
        "jmk-rp2040-0.1.0.uf2%00",
        "..",
        "README.txt",
    ] {
        let (status, body) = server.get(&format!("/api/v1/firmware/{name}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(error_code(&body), "invalid_name", "{name}");
    }
    // Unencoded separators never reach the handler: no such route.
    for path in ["/api/v1/firmware/../registry.db", "/api/v1/firmware/a/b"] {
        let (status, _) = server.get(path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn firmware_list_is_empty_without_a_directory() {
    let server = TestServer::new();
    let (status, listing) = server.get("/api/v1/firmware").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["items"], json!([]));
    let (status, _) = server.get("/api/v1/firmware/jmk-rp2040-0.1.0.uf2").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let missing = TestServer::with(|config| {
        config.firmware_dir = Some(config.db_path.parent().unwrap().join("does-not-exist"));
    });
    let (status, listing) = missing.get("/api/v1/firmware").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["items"], json!([]));
}

#[tokio::test]
async fn writes_are_rate_limited_per_client() {
    let server = TestServer::with(|config| {
        config.write_limit = 3;
        config.write_window = Duration::from_secs(600);
        config.trust_forwarded_for = true;
    });
    let write_from = |client: &'static str| {
        let server = &server;
        async move {
            let request = Request::post("/api/v1/keyboards")
                .header("x-forwarded-for", client)
                .body(Body::from(keyboard("Board").to_string()))
                .unwrap();
            server.send(request).await
        }
    };
    for _ in 0..3 {
        assert_eq!(write_from("203.0.113.7").await.status(), StatusCode::CREATED);
    }
    let refused = write_from("203.0.113.7").await;
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = refused.headers()[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
    assert!((1..=600).contains(&retry_after));
    let bytes = refused.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(error_code(&json), "rate_limited");

    // Rejected writes count too: a failed token guess is still a write.
    let request = Request::delete("/api/v1/keyboards/board")
        .header("x-forwarded-for", "203.0.113.7")
        .body(Body::empty())
        .unwrap();
    assert_eq!(server.send(request).await.status(), StatusCode::TOO_MANY_REQUESTS);

    // Reads are not limited, and another client has its own allowance. The
    // client cannot pick its bucket by sending its own forwarding entries.
    let request = Request::get("/api/v1/keyboards")
        .header("x-forwarded-for", "203.0.113.7")
        .body(Body::empty())
        .unwrap();
    assert_eq!(server.send(request).await.status(), StatusCode::OK);
    assert_eq!(write_from("198.51.100.2").await.status(), StatusCode::CREATED);
    assert_eq!(write_from("198.51.100.2, 203.0.113.7").await.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(write_from("203.0.113.7, 198.51.100.2").await.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn cors_allows_any_origin_and_authorized_writes() {
    let server = TestServer::new();
    let request = Request::get("/api/v1/keyboards")
        .header(header::ORIGIN, "app://jmk")
        .body(Body::empty())
        .unwrap();
    let response = server.send(request).await;
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri("/api/v1/keyboards/some-board")
        .header(header::ORIGIN, "app://jmk")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization, content-type")
        .body(Body::empty())
        .unwrap();
    let response = server.send(preflight).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    let methods = response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS].to_str().unwrap();
    for method in ["GET", "POST", "PUT", "DELETE"] {
        assert!(methods.contains(method), "{methods}");
    }
    let headers = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS].to_str().unwrap().to_lowercase();
    assert!(headers.contains("authorization") && headers.contains("content-type"), "{headers}");

    // Errors carry the header as well, or a browser would hide them from the app.
    let request = Request::get("/api/v1/keyboards/absent")
        .header(header::ORIGIN, "app://jmk")
        .body(Body::empty())
        .unwrap();
    let response = server.send(request).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
}

#[tokio::test]
async fn data_and_schema_survive_a_restart() {
    let directory = tempfile::tempdir().unwrap();
    let config = Config { db_path: directory.path().join("registry.db"), ..Config::default() };

    let first = app(AppState::open(config.clone()).unwrap());
    let request = Request::post("/api/v1/keyboards").body(Body::from(keyboard("Kept").to_string())).unwrap();
    assert_eq!(first.oneshot(request).await.unwrap().status(), StatusCode::CREATED);

    // Opening again runs the migrations against an up-to-date schema.
    let second = app(AppState::open(config.clone()).unwrap());
    let request = Request::get("/api/v1/keyboards/kept").body(Body::empty()).unwrap();
    assert_eq!(second.oneshot(request).await.unwrap().status(), StatusCode::OK);

    let connection = rusqlite::Connection::open(&config.db_path).unwrap();
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    assert_eq!(version, 1);
    // Only the hash of an edit token is stored.
    let stored: String = connection
        .query_row("SELECT edit_token_hash FROM keyboards WHERE slug = 'kept'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored.len(), 64);

    // A database from a newer server is refused rather than guessed at.
    connection.pragma_update(None, "user_version", 99).unwrap();
    drop(connection);
    let error = AppState::open(config).err().unwrap();
    assert!(error.contains("newer"), "{error}");
}
