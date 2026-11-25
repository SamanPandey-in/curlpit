# Collections on disk

Curlpit keeps everything as ordinary files in one data directory. Collections are folders, and every request is its own small TOML file. You can read and edit them by hand, keep them in Git, and review changes in a pull request. This page describes the layout and file formats as implemented in [`crates/store`](../../crates/store/src/lib.rs).

## The data directory

| OS | Default location |
|---|---|
| macOS | `~/Library/Application Support/dev.api-tester.API-Tester/` |
| Windows | `%APPDATA%\api-tester\API Tester\data\` |
| Linux | `$XDG_DATA_HOME/apitester/` (usually `~/.local/share/apitester/`) |

Set the environment variable `API_TESTER_DATA_DIR` to use any other folder. The Settings page shows the folder in use.

```text
<data-dir>/
├── collections/                    one folder per collection
│   └── getting-started/
│       ├── collection.toml         collection settings (name, description, …)
│       ├── 1-get-with-query-params.toml
│       ├── 2-post-json.toml
│       └── Users/                  a folder: its name is the directory name
│           ├── folder.toml         folder settings (optional)
│           └── list-users.toml
├── environments/
│   └── httpbin.toml                one file per environment
├── globals.toml                    global variables
├── settings.toml                   app settings
├── session.toml                    open tabs, including unsaved drafts
├── history.db                      request history (SQLite)
└── cookies.json                    the cookie jar
```

To version a collection with Git, the simplest setup is to point `API_TESTER_DATA_DIR` at a folder inside your repository, or to make `collections/<name>/` a repository. Leave `history.db`, `session.toml` and `cookies.json` out of version control. They are local state, and may contain response data, unsaved drafts with literal secrets, or session cookies.

## Rules that apply to every file

- **`schema_version`** is the first key, currently `1`. A file with a newer version than the app supports is refused and never overwritten. Its request appears in the sidebar with ⚠ and an explanation.
- **Unreadable files are shown, not hidden.** A request file that doesn't parse appears with ⚠, and clicking it shows the error. An unreadable environment file is skipped, and is never overwritten unless you save an environment to the same file name.
- **Atomic writes.** Every save writes a temporary file in the same directory, flushes it to disk, and renames it over the original. A crash leaves either the old file or the new one, never half of one.
- **Hidden entries are ignored.** Anything whose name starts with `.` (`.git`, `.DS_Store`, editor swap files) is skipped.
- **New fields are additive.** Fields added after a file was written get defaults when it's loaded, so old files keep working. Re-saving an older request file adds only its `uid` line.

## Collections

A collection is a directory under `collections/`. The directory name is a slug of the collection's name (lowercase, with runs of other characters turned into `-`), and a clash gets `-2`, `-3` and so on. The **display name** comes from `collection.toml`. If that file is missing or has no name, the directory name is used.

### `collection.toml`

```toml
schema_version = 1
name = "Getting started"
description = "Sample requests against httpbin.org, a public echo service. …"
```

These optional keys can also be stored (the format comes from decision D48):

| Key | Type |
|---|---|
| `pre_request_script`, `test_script` | strings |
| `[[variables]]` | `key`, `value`, `secret`, `enabled` |
| `[[headers]]` | `key`, `value`, `description`, `enabled` |
| `[auth]` | same shape as a request's `[request.auth]`; omitted when none |

> **Stored, not applied yet.** Curlpit reads and writes these settings, but sending a request does not use the collection's or folder's variables, headers, auth or scripts yet. Until that lands, put shared values in an environment.

## Folders

Any sub-directory of a collection is a folder, at any depth. Folders show their **directory name** as is, so `User Management/` reads as "User Management". The sidebar lists folders first, then requests, each sorted by name (case-insensitive). The collection runner uses the same order.

### `folder.toml` (optional)

```toml
schema_version = 1
description = "User endpoints"
```

It takes the same optional keys as `collection.toml` except `name`. A folder's `auth` defaults to **inherit**, and is omitted from the file in that case. Like the collection settings, this is stored but not applied yet.

You can create folders in your file manager; Curlpit lists them on the next refresh.

## Requests

One `.toml` file per request. The file name is a slug of the request name, and a clash in the same directory gets `-2`, `-3` and so on. Renaming a request in the app and saving it moves the file. A request named "Collection" or "Folder" is saved as `collection-request.toml` or `folder-request.toml`, so it never collides with the settings files. Older request files that happen to have those names still load as requests.

Here is a saved request (`2-post-json.toml` from the sample collection):

```toml
schema_version = 1

[request]
uid = "43a2a6d9-dc6a-485e-a5a9-e6e038810457"
id = ""
name = "2. POST JSON"
method = "POST"
url = "https://httpbin.org/post"
params = []
headers = []

[request.body]
type = "json"
raw = """
{
  "name": "Curlpit",
  "tags": ["native", "fast"]
}"""

[request.auth]
type = "none"

[request.settings]
timeout_ms = 30000
follow_redirects = true
verify_tls = true
```

### `[request]` fields

| Key | Meaning |
|---|---|
| `uid` | A stable identity (UUID v4). It is assigned on the first save and kept when the request is renamed or moved, so tools can follow a request across file-name changes. Leave it out of hand-written files and one is added on the next save. Don't copy it when you duplicate a file by hand. |
| `id` | A legacy field, usually empty |
| `name` | Display name. If empty, the file name is shown. |
| `method` | `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `HEAD`, `TRACE`, `CONNECT` |
| `url` | As typed, `{{variables}}` included. If there is no `://` after variables are substituted, `https://` is added. |
| `pre_request_script`, `test_script` | JavaScript (see [scripting](scripting.md)). Omitted when empty, and placed above the tables so files read top to bottom. |
| `[[request.params]]`, `[[request.headers]]` | `key`, `value`, `description`, `enabled` |
| `[request.body]` | See below |
| `[request.auth]` | See below |
| `[request.settings]` | `timeout_ms` (0 means no timeout; the default is 30000), `follow_redirects`, `verify_tls` |

### Body (`[request.body]`, by `type`)

| `type` | Other keys |
|---|---|
| `none` | |
| `json`, `xml`, `html`, `text` | `raw` |
| `urlencoded` | `[[request.body.fields]]` with `key`, `value`, `description`, `enabled` |
| `formdata` | `[[request.body.fields]]` with `key`, `enabled`, and `value = { type = "text", value = "…" }` or `value = { type = "file", path = "…" }` |
| `graphql` | `query`, and `variables` (JSON text, omitted when empty) |

### Auth (`[request.auth]`, by `type`)

| `type` | Other keys | Sent? |
|---|---|---|
| `none` | | |
| `inherit` | | Sends no auth until inheritance is built |
| `basic` | `username`, `password` | Yes |
| `bearer` | `token` | Yes |
| `apikey` | `key`, `value`, `location` (`header` or `query`) | Yes |
| `jwt` | `token`, `prefix` (default `Bearer`) | Yes |
| `custom` | `[[request.auth.headers]]` | Yes |
| `oauth2` | `grant_type` (`authorization_code`, `client_credentials`, `password`, `device_code`), `auth_url`, `token_url`, `refresh_url`, `client_id`, `client_secret`, `scope`, `redirect_uri`, `state`, `pkce`, `username`, `password`, `client_auth` (`header` or `body`), `add_token_to`, `header_prefix` | Not yet: sending fails with "isn't supported yet" |
| `digest` | `username`, `password` | Not yet |

**Secrets are plain text.** Passwords, tokens and secret environment values are masked in the editor but stored as written. Prefer `{{variables}}` in request files, and keep the values in an environment file that you don't commit.

## Environments and globals

`environments/<slug>.toml`, one per environment:

```toml
schema_version = 1
name = "httpbin"

[[variables]]
key = "baseUrl"
value = "https://httpbin.org"
secret = false
enabled = true
```

`globals.toml` has the same `[[variables]]` list and no `name`. A missing `globals.toml` means no globals. A corrupt one is reported as an error rather than silently treated as empty.

## `settings.toml`

```toml
schema_version = 1
active_environment = "httpbin"
default_timeout_ms = 30000
verify_tls = true
history_cap = 1000
proxy = "http://proxy.local:3128"   # omitted when empty
```

All keys are optional, and missing keys get the defaults shown. The app always loads, changes and saves this file, so one feature never resets another's settings. A missing or unreadable file means defaults.

## The first-run sample

When Curlpit starts with a data directory that contains nothing but an empty `collections/` folder (hidden entries aside), it writes the **Getting started** collection, the `httpbin` environment, and a `settings.toml` that makes `httpbin` active. Any other content (a settings file, history, an environment, a collection) means the directory is in use, and nothing is written. Once seeded, the directory is never fresh again, so deleting the sample doesn't bring it back.
