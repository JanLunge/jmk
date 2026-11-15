# JMK registry server

Stores the keyboard definitions and keymaps that people share from the JMK
configurator, and lists downloadable firmware images. A builder publishes a
keyboard once; everyone with the same board picks it from the catalogue
instead of running the wiring wizard.

One binary, one SQLite file. There are no user accounts: publishing returns a
secret edit token, and that token is what allows later changes.

This crate is not part of the root workspace. It has its own `Cargo.lock`, so
the firmware and tools build without its dependencies.

## Run

```sh
cd server
cargo run --release
```

The server listens on `127.0.0.1:8787` and creates `./jmk-registry.db`. The
schema is created and upgraded at start-up; `PRAGMA user_version` holds its
version, and a database written by a newer server is refused.

| Variable | Default | Meaning |
| --- | --- | --- |
| `JMK_BIND` | `127.0.0.1:8787` | Address and port to listen on. |
| `JMK_DB` | `./jmk-registry.db` | SQLite database file. |
| `JMK_ADMIN_TOKEN` | unset | Enables the moderation endpoints. At least 16 characters. Without it they answer 403. |
| `JMK_FIRMWARE_DIR` | unset | Directory of firmware images to list and serve. |
| `JMK_TRUST_FORWARDED_FOR` | `false` | Set to `true` only behind a reverse proxy that appends the client address to `X-Forwarded-For`. The rate limit then uses the last entry of that header instead of the connecting address, which would otherwise be the proxy for everyone. |

### Docker

```sh
docker build -t jmk-registry server/
docker run --rm -p 8787:8787 -v jmk-registry-data:/data \
    -e JMK_ADMIN_TOKEN="$(openssl rand -hex 24)" \
    -v "$PWD/firmware-images:/firmware:ro" -e JMK_FIRMWARE_DIR=/firmware \
    jmk-registry
```

The image listens on `0.0.0.0:8787`, keeps the database in the `/data` volume
and runs as a non-root user.

### Test

```sh
cd server
cargo test
```

The integration tests in `tests/api.rs` drive the router in-process against a
temporary database; nothing listens on a port. `scripts/check.sh` runs them
with the rest of the project's checks.

## API

Everything is JSON. Errors have one shape, with a matching status code:

```json
{ "error": { "code": "validation_failed", "message": "name must be 1 to 64 characters" } }
```

| Status | `code` | When |
| --- | --- | --- |
| 400 | `invalid_json`, `invalid_query`, `invalid_name`, `invalid_body` | The request could not be understood. |
| 401 | `missing_token` | A token is needed and none was sent. |
| 403 | `wrong_token`, `admin_disabled` | The token does not fit, or moderation is off. |
| 404 | `not_found` | No such route, keyboard, version, keymap, report or image. |
| 405 | `method_not_allowed` | |
| 413 | `body_too_large` | The request body is over 256 KiB. |
| 422 | `validation_failed` | Valid JSON that breaks a rule below. |
| 429 | `rate_limited` | Too many writes; see `Retry-After`. |
| 500 | `internal` | |

**Tokens.** Creating a keyboard or a keymap returns an `edit_token` once. The
server keeps only its SHA-256 hash, so a lost token cannot be recovered. Send
it as `Authorization: Bearer <edit_token>` to change or delete that one item.
The admin token is sent the same way; it can hide, unhide and delete, but not
edit.

**Rate limit.** Each client address gets 30 write requests (anything but
`GET`, `HEAD` and `OPTIONS`) per 10 minutes, counted whether or not they
succeed. IPv6 clients are counted per /64. The counters live in memory and
reset when the server restarts.

**CORS.** Any origin may call the API, with `GET`, `POST`, `PUT`, `DELETE`
and the `Authorization` and `Content-Type` headers.

**Definitions.** `definition` is the configurator's own format and is stored
and returned as sent, key order included. The server checks only that it is a
JSON object with an integer `schema`.

The examples use `curl` against a local server:

```sh
API=http://127.0.0.1:8787/api/v1
```

### Health

```sh
curl http://127.0.0.1:8787/healthz
# 200 {"status":"ok"}
```

### Keyboards

#### Publish: `POST /keyboards`

| Field | Required | Rule |
| --- | --- | --- |
| `name` | yes | 1 to 64 characters |
| `description` | no | at most 2000 characters |
| `author` | no | at most 64 characters |
| `tags` | no | at most 8, each 1 to 24 characters of `a-z`, `0-9`, `-` |
| `mcu` | yes | `rp2040`, `rp2350` or `nrf52840` |
| `key_count` | yes | integer, 1 to 1024 |
| `split` | yes | boolean |
| `definition` | yes | object with an integer `schema` |

```sh
curl -X POST $API/keyboards -H 'Content-Type: application/json' -d '{
  "name": "Plank 48",
  "description": "Hand-wired 4x12 ortholinear",
  "author": "ada",
  "tags": ["ortho", "hand-wired"],
  "mcu": "rp2040",
  "key_count": 48,
  "split": false,
  "definition": { "schema": 1, "matrix": { "rows": 4, "cols": 12 } }
}'
# 201 {"slug":"plank-48","version":1,"edit_token":"2cd9a294…"}
```

The slug is the name in lowercase with everything but letters and digits
turned into hyphens. If it is taken, a number is appended: `plank-48-2`. It
never changes afterwards, even when the keyboard is renamed.

#### Browse: `GET /keyboards`

| Parameter | Meaning |
| --- | --- |
| `q` | Words to search for in name, description, author and tags. Every word must appear; case is ignored. |
| `tag` | Only keyboards carrying exactly this tag. |
| `mcu` | `rp2040`, `rp2350` or `nrf52840`. |
| `split` | `true` or `false`. |
| `sort` | `recent` (default, last updated first), `popular` (most downloads first) or `name`. |
| `page`, `per_page` | From 1; `per_page` defaults to 20 and is at most 100. |

```sh
curl "$API/keyboards?q=ortho&mcu=rp2040&sort=popular&per_page=10"
```

```json
{
  "items": [
    {
      "slug": "plank-48",
      "name": "Plank 48",
      "description": "Hand-wired 4x12 ortholinear",
      "author": "ada",
      "tags": ["ortho", "hand-wired"],
      "mcu": "rp2040",
      "key_count": 48,
      "split": false,
      "version": 1,
      "downloads": 0,
      "updated_at": "2026-11-15T04:25:48Z"
    }
  ],
  "page": 1,
  "per_page": 10,
  "total": 1
}
```

#### Fetch: `GET /keyboards/{slug}`

Returns the summary fields plus the latest `definition`, and counts one
download.

```sh
curl $API/keyboards/plank-48
```

#### Versions: `GET /keyboards/{slug}/versions` and `/versions/{n}`

```sh
curl $API/keyboards/plank-48/versions
# {"items":[{"version":2,"created_at":"…"},{"version":1,"created_at":"…"}]}

curl $API/keyboards/plank-48/versions/1
# {"slug":"plank-48","version":1,"created_at":"…","definition":{…}}
```

These do not count as downloads.

#### Update: `PUT /keyboards/{slug}`

Needs the edit token. Send any of the fields from publishing; fields left out
keep their value. Sending a `definition` stores it as the next version, and
earlier versions stay readable. Changing only metadata does not create a
version. Returns the updated summary.

```sh
curl -X PUT $API/keyboards/plank-48 \
  -H "Authorization: Bearer $EDIT_TOKEN" -H 'Content-Type: application/json' \
  -d '{ "description": "Now with a rotary encoder",
        "definition": { "schema": 1, "matrix": { "rows": 4, "cols": 12 }, "encoders": 1 } }'
```

#### Delete: `DELETE /keyboards/{slug}`

Needs the edit token or the admin token. Removes the keyboard with all its
versions, keymaps and reports. Answers 204. The slug becomes free again.

```sh
curl -X DELETE $API/keyboards/plank-48 -H "Authorization: Bearer $EDIT_TOKEN"
```

### Keymaps

Alternative keymaps that anyone can attach to a published keyboard. They are
not versioned: an update replaces the keymap.

```sh
# Share one. name is required; description and author are optional.
curl -X POST $API/keyboards/plank-48/keymaps -H 'Content-Type: application/json' -d '{
  "name": "Colemak-DH",
  "description": "Home row mods",
  "author": "lin",
  "definition": { "schema": 1, "layers": [["Q", "W", "F", "P"]] }
}'
# 201 {"id":1,"edit_token":"9f1c…"}

# List a keyboard's keymaps (summaries, no definitions).
curl $API/keyboards/plank-48/keymaps
# {"items":[{"id":1,"keyboard":"plank-48","name":"Colemak-DH","description":"Home row mods",
#            "author":"lin","created_at":"…","updated_at":"…"}]}

# Fetch one with its definition.
curl $API/keymaps/1

# Change it (any of name, description, author, definition), with its own token.
curl -X PUT $API/keymaps/1 -H "Authorization: Bearer $KEYMAP_TOKEN" \
  -H 'Content-Type: application/json' -d '{ "name": "Colemak-DH v2" }'

# Delete it: its own token or the admin token. 204.
curl -X DELETE $API/keymaps/1 -H "Authorization: Bearer $KEYMAP_TOKEN"
```

A keyboard's edit token does not open the keymaps attached to it.

### Reports and moderation

Anyone can report a keyboard. `reason` is 1 to 1000 characters.

```sh
curl -X POST $API/keyboards/plank-48/report -H 'Content-Type: application/json' \
  -d '{ "reason": "This is an advert, not a keyboard" }'
# 202 {"status":"received"}
```

The rest needs `JMK_ADMIN_TOKEN` to be set on the server and sent as the
bearer token.

```sh
# Reports, newest first. Takes page and per_page.
curl $API/admin/reports -H "Authorization: Bearer $JMK_ADMIN_TOKEN"
# {"items":[{"id":1,"keyboard":"plank-48","keyboard_name":"Plank 48","keyboard_hidden":false,
#            "reason":"This is an advert, not a keyboard","created_at":"…"}],
#  "page":1,"per_page":20,"total":1}

# Hide a keyboard, and bring it back.
curl -X POST $API/admin/keyboards/plank-48/hide   -H "Authorization: Bearer $JMK_ADMIN_TOKEN"
curl -X POST $API/admin/keyboards/plank-48/unhide -H "Authorization: Bearer $JMK_ADMIN_TOKEN"
# {"slug":"plank-48","hidden":true}

# Dismiss a report that has been dealt with. 204.
curl -X DELETE $API/admin/reports/1 -H "Authorization: Bearer $JMK_ADMIN_TOKEN"
```

A hidden keyboard answers 404 everywhere outside `/admin`: the list, the
keyboard itself, its versions, its keymaps, and edits with its own token. Its
slug stays taken. Only the admin token can delete it.

### Firmware

Lists the files in `JMK_FIRMWARE_DIR` named `jmk-<mcu>-<version>.<ext>`, where
`<mcu>` is a supported microcontroller and `<ext>` is `uf2`, `hex`, `bin` or
`zip`. Other files, directories and symbolic links are ignored. `url` is
relative to the server's address.

```sh
curl $API/firmware
```

```json
{
  "items": [
    {
      "name": "jmk-rp2040-0.1.0.uf2",
      "mcu": "rp2040",
      "version": "0.1.0",
      "size": 71680,
      "sha256": "6105d6cc76af400325e94d588ce511be5bfdbb73b437dc51eca43917d7a43e3d",
      "url": "/api/v1/firmware/jmk-rp2040-0.1.0.uf2"
    }
  ]
}
```

```sh
curl -O $API/firmware/jmk-rp2040-0.1.0.uf2
```

A name of any other shape, including one with a path separator, is answered
with 400 `invalid_name`.
