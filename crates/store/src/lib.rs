//! Local-first storage (FR-COL-06, FR-PER-01/03/04/05).
//!
//! Layout (human readable, one file per request, friendly to git):
//! ```text
//! <data-dir>/collections/<collection>/collection.toml
//!                                    /<folder>/...
//!                                    /<request>.toml
//! ```
//! Every file carries `schema_version`. Writes are atomic (temp file + fsync + rename), so a crash
//! mid-save leaves either the old or the new file, never a torn one.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use at_core::{CollectionMeta, EnvVar, Environment, FolderMeta, Request};
use serde::{Deserialize, Serialize};

mod sample;
mod secrets;
pub use sample::{sample_requests, SAMPLE_BASE_URL, SAMPLE_COLLECTION, SAMPLE_ENVIRONMENT};
pub use secrets::{backup_path, ensure_gitignore, local_path, ENV_GITIGNORE};

pub const SCHEMA_VERSION: u32 = 1;
/// Collection settings ([`CollectionMeta`]) at a collection root.
pub const COLLECTION_FILE: &str = "collection.toml";
/// Folder settings ([`FolderMeta`]) inside a folder.
pub const FOLDER_FILE: &str = "folder.toml";
/// Lines the data directory's `.gitignore` gets once the globals hold a secret (G-14).
pub const GLOBALS_GITIGNORE: &[&str] = &["globals.local.toml", "globals.toml.bak"];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read {path}: {msg}")]
    Parse { path: PathBuf, msg: String },
    #[error("cannot write {path}: {msg}")]
    Serialize { path: PathBuf, msg: String },
    #[error("{path} was saved by a newer version of API Tester (file schema {found}, this version supports up to {supported})")]
    TooNew {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    #[error("no data directory is available on this system")]
    NoDataDir,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Serialize)]
struct RequestFileRef<'a> {
    schema_version: u32,
    request: &'a Request,
}

#[derive(Deserialize)]
struct RequestFile {
    #[allow(dead_code)]
    schema_version: u32,
    request: Request,
}

#[derive(Deserialize)]
struct Versioned {
    #[serde(default)]
    schema_version: u32,
}

#[derive(Serialize)]
struct EnvFileRef<'a> {
    schema_version: u32,
    name: &'a str,
    variables: &'a [EnvVar],
}

#[derive(Deserialize)]
struct EnvFile {
    #[allow(dead_code)]
    schema_version: u32,
    name: String,
    #[serde(default)]
    variables: Vec<EnvVar>,
}

#[derive(Serialize)]
struct GlobalsFileRef<'a> {
    schema_version: u32,
    variables: &'a [EnvVar],
}

#[derive(Deserialize)]
struct GlobalsFile {
    #[allow(dead_code)]
    schema_version: u32,
    #[serde(default)]
    variables: Vec<EnvVar>,
}

/// Small app settings kept in `<data-dir>/settings.toml` (more fields arrive with the Settings screen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "current_schema")]
    pub schema_version: u32,
    /// Name of the active environment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_environment: Option<String>,
    /// Default per-request timeout, in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub default_timeout_ms: u64,
    /// Whether TLS certificates are verified on outgoing requests.
    #[serde(default = "default_verify_tls")]
    pub verify_tls: bool,
    /// Maximum number of entries kept in request history.
    #[serde(default = "default_history_cap")]
    pub history_cap: usize,
    /// Proxy URL to route requests through (empty string = no proxy).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy: String,
    /// PEM file of extra trusted root certificates, added to the built-in ones (G-17).
    /// Empty = none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ca_bundle: String,
    /// Lowest TLS version allowed: `"1.2"`, `"1.3"`, or empty for the library default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub min_tls: String,
    /// Client certificates, each presented to one host (G-17). Kept last: TOML writes
    /// arrays of tables after the plain keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub client_certs: Vec<ClientCertSetting>,
}

/// One client certificate in [`Settings::client_certs`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCertSetting {
    /// Host it is presented to, matched exactly (no port, no wildcards).
    #[serde(default)]
    pub host: String,
    /// The certificate file. The app supports PEM with the certificate and an unencrypted
    /// key in one file.
    #[serde(default)]
    pub path: String,
    /// Reserved for PKCS#12 files, which the current TLS backend cannot read; kept so a
    /// load-modify-save never drops it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

fn current_schema() -> u32 {
    SCHEMA_VERSION
}

fn default_timeout_ms() -> u64 {
    30_000
}

fn default_verify_tls() -> bool {
    true
}

fn default_history_cap() -> usize {
    1000
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_environment: None,
            default_timeout_ms: 30_000,
            verify_tls: true,
            history_cap: 1000,
            proxy: String::new(),
            ca_bundle: String::new(),
            min_tls: String::new(),
            client_certs: Vec::new(),
        }
    }
}

/// The open tabs, kept in `<data-dir>/session.toml` so they come back after a restart,
/// unsaved edits included ("hot exit").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default = "current_schema")]
    pub schema_version: u32,
    /// Index of the active tab.
    #[serde(default)]
    pub active: usize,
    #[serde(default)]
    pub tabs: Vec<SessionTab>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active: 0,
            tabs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionTab {
    /// The request file this tab is linked to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Folder the next Save goes to when the tab has no file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<PathBuf>,
    /// The editor contents had changes that weren't saved.
    #[serde(default)]
    pub unsaved: bool,
    /// What the editor showed.
    pub draft: Request,
}

/// An environment together with the file it lives in.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvEntry {
    pub env: Environment,
    pub path: PathBuf,
}

/// `collection.toml` / `folder.toml` on disk: the meta fields at the top level, next to
/// `schema_version` (older `collection.toml` files hold just `name` and `description`).
#[derive(Serialize)]
struct MetaFileRef<'a, T> {
    schema_version: u32,
    #[serde(flatten)]
    meta: &'a T,
}

/// A fresh stable id for [`Request::uid`].
pub fn new_uid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `collection.toml` and `folder.toml` are settings files, but a request named "Collection" or
/// "Folder" was once saved under those names; such a file (one with a `[request]` table) is
/// still a request.
fn is_meta_file_name(name: &str) -> bool {
    name == COLLECTION_FILE || name == FOLDER_FILE
}

fn holds_request(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
        .is_some_and(|t| t.contains_key("request"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder {
        name: String,
        path: PathBuf,
        children: Vec<Node>,
    },
    Request {
        name: String,
        path: PathBuf,
    },
    /// A file we could not read; shown to the user instead of silently hiding it.
    Broken {
        name: String,
        path: PathBuf,
        error: String,
    },
}

impl Node {
    pub fn name(&self) -> &str {
        match self {
            Node::Folder { name, .. } | Node::Request { name, .. } | Node::Broken { name, .. } => {
                name
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionTree {
    /// Directory name (stable id).
    pub dir_name: String,
    /// Display name from collection.toml.
    pub name: String,
    pub dir: PathBuf,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Store {
    /// `<data-dir>/collections`
    root: PathBuf,
    data_dir: PathBuf,
}

/// File-system friendly name: lowercase, letters/digits kept, everything else becomes `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in name.trim().chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        return "untitled".into();
    }
    // Windows reserved device names
    let reserved = matches!(out.as_str(), "con" | "prn" | "aux" | "nul")
        || ((out.starts_with("com") || out.starts_with("lpt"))
            && out.len() == 4
            && out.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.push_str("-file");
    }
    out
}

/// A folder name that is safe on every OS but still reads like the original: characters that
/// are forbidden in file names become `-`, leading dots (hidden files) and trailing dots/spaces
/// (rejected on Windows) are dropped, and Windows device names get a suffix.
pub fn folder_name(name: &str) -> String {
    let mut out: String = name
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '-'
            } else {
                ch
            }
        })
        .collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        return "Untitled folder".into();
    }
    let lower = out.to_lowercase();
    let reserved = matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || ((lower.starts_with("com") || lower.starts_with("lpt"))
            && lower.len() == 4
            && lower.as_bytes()[3].is_ascii_digit());
    if reserved {
        return format!("{out} folder");
    }
    out
}

fn unique_path(dir: &Path, slug: &str, ext: &str, replace: Option<&Path>) -> PathBuf {
    let mut n = 1u32;
    loop {
        let file = if n == 1 {
            format!("{slug}{ext}")
        } else {
            format!("{slug}-{n}{ext}")
        };
        let p = dir.join(file);
        if !p.exists() || Some(p.as_path()) == replace {
            return p;
        }
        n += 1;
    }
}

fn atomic_write(path: &Path, contents: &str) -> Result<(), StoreError> {
    let dir = path.parent().ok_or(StoreError::NoDataDir)?;
    fs::create_dir_all(dir).map_err(io(dir))?;
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    {
        let mut f = fs::File::create(&tmp).map_err(io(&tmp))?;
        f.write_all(contents.as_bytes()).map_err(io(&tmp))?;
        f.sync_all().map_err(io(&tmp))?;
    }
    fs::rename(&tmp, path).map_err(io(path))
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

impl Store {
    /// `API_TESTER_DATA_DIR` overrides the OS location (handy for testing without touching real data).
    pub fn open_default() -> Result<Store, StoreError> {
        if let Ok(p) = std::env::var("API_TESTER_DATA_DIR") {
            return Store::open_at(p);
        }
        let dirs = directories::ProjectDirs::from("dev", "api-tester", "API Tester")
            .ok_or(StoreError::NoDataDir)?;
        Store::open_at(dirs.data_dir())
    }

    pub fn open_at(data_dir: impl Into<PathBuf>) -> Result<Store, StoreError> {
        let data_dir: PathBuf = data_dir.into();
        let root = data_dir.join("collections");
        fs::create_dir_all(&root).map_err(io(&root))?;
        Ok(Store { root, data_dir })
    }

    /// `<data-dir>/collections`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<data-dir>` itself (environments, settings, history live here).
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Create a sub-folder of a collection or folder. The display name is kept readable (case and
    /// spaces preserved; only characters that are unsafe in file names are replaced). Collisions
    /// get ` 2`, ` 3`, ...
    pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, StoreError> {
        let base = folder_name(name);
        let mut n = 1u32;
        let dir = loop {
            let cand = if n == 1 {
                parent.join(&base)
            } else {
                parent.join(format!("{base} {n}"))
            };
            if !cand.exists() {
                break cand;
            }
            n += 1;
        };
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        Ok(dir)
    }

    pub fn collection_dir(&self, dir_name: &str) -> PathBuf {
        self.root.join(dir_name)
    }

    pub fn create_collection(&self, name: &str) -> Result<CollectionTree, StoreError> {
        let name = if name.trim().is_empty() {
            "Untitled collection"
        } else {
            name.trim()
        };
        let dir = unique_path(&self.root, &slug(name), "", None);
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        Store::save_collection_meta(
            &dir,
            &CollectionMeta {
                name: name.to_string(),
                ..Default::default()
            },
        )?;
        Ok(CollectionTree {
            dir_name: dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            name: name.to_string(),
            dir,
            children: Vec::new(),
        })
    }

    pub fn load_all(&self) -> Result<Vec<CollectionTree>, StoreError> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.root).map_err(io(&self.root))? {
            let entry = entry.map_err(io(&self.root))?;
            let dir_name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden(&dir_name) || !entry.path().is_dir() {
                continue;
            }
            let dir = entry.path();
            // A collection whose settings can't be read still lists its requests.
            let name = Store::load_collection_meta(&dir)
                .ok()
                .map(|c| c.name)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| dir_name.clone());
            let children = Store::load_tree(&dir)?;
            out.push(CollectionTree {
                dir_name,
                name,
                dir,
                children,
            });
        }
        out.sort_by_key(|c| c.name.to_lowercase());
        Ok(out)
    }

    pub fn load_tree(dir: &Path) -> Result<Vec<Node>, StoreError> {
        let mut nodes = Vec::new();
        for entry in fs::read_dir(dir).map_err(io(dir))? {
            let entry = entry.map_err(io(dir))?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if is_hidden(&file_name) {
                continue;
            }
            if path.is_dir() {
                let children = Store::load_tree(&path)?;
                nodes.push(Node::Folder {
                    name: file_name,
                    path,
                    children,
                });
            } else if file_name.ends_with(".toml")
                && (!is_meta_file_name(&file_name) || holds_request(&path))
            {
                let stem = file_name.trim_end_matches(".toml").to_string();
                match Store::load_request(&path) {
                    Ok(req) => {
                        let name = if req.name.trim().is_empty() {
                            stem
                        } else {
                            req.name
                        };
                        nodes.push(Node::Request { name, path });
                    }
                    Err(e) => nodes.push(Node::Broken {
                        name: stem,
                        path,
                        error: e.to_string(),
                    }),
                }
            }
        }
        nodes.sort_by_key(|n| (!matches!(n, Node::Folder { .. }), n.name().to_lowercase()));
        Ok(nodes)
    }

    pub fn load_request(path: &Path) -> Result<Request, StoreError> {
        let text = fs::read_to_string(path).map_err(io(path))?;
        let v: Versioned = toml::from_str(&text).map_err(|e| StoreError::Parse {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
        if v.schema_version > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                path: path.to_path_buf(),
                found: v.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        let f: RequestFile = toml::from_str(&text).map_err(|e| StoreError::Parse {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
        Ok(f.request)
    }

    /// Save into `dir` (a collection or folder directory). The file name comes from the request name.
    /// `replace` is the file this request was opened from: it is overwritten in place, or removed
    /// if the new name maps to a different file.
    ///
    /// A request without a [`Request::uid`] keeps the uid of the `replace` file (so renames and
    /// moves keep it even if the caller never saw it), or gets a fresh one.
    pub fn save_request(
        dir: &Path,
        req: &Request,
        replace: Option<&Path>,
    ) -> Result<PathBuf, StoreError> {
        let with_uid;
        let req = if req.uid.is_empty() {
            let uid = replace
                .and_then(|p| Store::load_request(p).ok())
                .map(|r| r.uid)
                .filter(|u| !u.is_empty())
                .unwrap_or_else(new_uid);
            with_uid = Request { uid, ..req.clone() };
            &with_uid
        } else {
            req
        };
        // `collection.toml` / `folder.toml` are settings files, not requests
        let mut name = slug(&req.name);
        if is_meta_file_name(&format!("{name}.toml")) {
            name.push_str("-request");
        }
        let target = unique_path(dir, &name, ".toml", replace);
        let text = toml::to_string_pretty(&RequestFileRef {
            schema_version: SCHEMA_VERSION,
            request: req,
        })
        .map_err(|e| StoreError::Serialize {
            path: target.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&target, &text)?;
        if let Some(old) = replace {
            if old != target && old.exists() {
                fs::remove_file(old).map_err(io(old))?;
            }
        }
        Ok(target)
    }

    pub fn delete_request(path: &Path) -> Result<(), StoreError> {
        fs::remove_file(path).map_err(io(path))
    }

    /// Move a request file into another collection or folder directory, keeping its uid (one
    /// is assigned if it had none). Returns the new path.
    pub fn move_request(path: &Path, to_dir: &Path) -> Result<PathBuf, StoreError> {
        let req = Store::load_request(path)?;
        Store::save_request(to_dir, &req, Some(path))
    }

    // ---- collection and folder settings (G-03; stored, not applied yet) ----

    /// Read a settings file: `None` if it doesn't exist (or is a request saved under that
    /// name), an error if it is unreadable or from a newer version.
    fn read_meta<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, StoreError> {
        if !path.exists() || holds_request(path) {
            return Ok(None);
        }
        let text = Store::read_versioned(path)?;
        toml::from_str::<T>(&text)
            .map(Some)
            .map_err(|e| StoreError::Parse {
                path: path.to_path_buf(),
                msg: e.to_string(),
            })
    }

    fn write_meta<T: Serialize>(path: &Path, meta: &T) -> Result<(), StoreError> {
        if holds_request(path) {
            return Err(StoreError::Serialize {
                path: path.to_path_buf(),
                msg: "a saved request has this file name; rename that request first".into(),
            });
        }
        let text = toml::to_string_pretty(&MetaFileRef {
            schema_version: SCHEMA_VERSION,
            meta,
        })
        .map_err(|e| StoreError::Serialize {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
        atomic_write(path, &text)
    }

    /// The `collection.toml` of the collection at `dir`. A missing file gives defaults, with
    /// the directory name as the display name.
    pub fn load_collection_meta(dir: &Path) -> Result<CollectionMeta, StoreError> {
        let mut meta =
            Store::read_meta::<CollectionMeta>(&dir.join(COLLECTION_FILE))?.unwrap_or_default();
        if meta.name.trim().is_empty() {
            meta.name = dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Ok(meta)
    }

    /// Write `collection.toml` at the collection root `dir` (atomically).
    pub fn save_collection_meta(dir: &Path, meta: &CollectionMeta) -> Result<(), StoreError> {
        Store::write_meta(&dir.join(COLLECTION_FILE), meta)
    }

    /// The `folder.toml` of the folder at `dir`; defaults (inherit everything) if missing.
    pub fn load_folder_meta(dir: &Path) -> Result<FolderMeta, StoreError> {
        Ok(Store::read_meta(&dir.join(FOLDER_FILE))?.unwrap_or_default())
    }

    /// Write `folder.toml` in the folder `dir` (atomically).
    pub fn save_folder_meta(dir: &Path, meta: &FolderMeta) -> Result<(), StoreError> {
        Store::write_meta(&dir.join(FOLDER_FILE), meta)
    }

    // ---- environments, globals, settings (FR-ENV-01/02, FR-PER-01) ----

    fn env_dir(&self) -> PathBuf {
        self.data_dir.join("environments")
    }

    fn globals_path(&self) -> PathBuf {
        self.data_dir.join("globals.toml")
    }

    fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.toml")
    }

    pub(crate) fn read_versioned(path: &Path) -> Result<String, StoreError> {
        let text = fs::read_to_string(path).map_err(io(path))?;
        let v: Versioned = toml::from_str(&text).map_err(|e| StoreError::Parse {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
        if v.schema_version > SCHEMA_VERSION {
            return Err(StoreError::TooNew {
                path: path.to_path_buf(),
                found: v.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(text)
    }

    /// All environments, sorted by name. Unreadable files are skipped (they are never overwritten
    /// unless the user saves an environment with the same file name).
    pub fn list_environments(&self) -> Vec<EnvEntry> {
        let dir = self.env_dir();
        let mut out = Vec::new();
        let Ok(rd) = fs::read_dir(&dir) else {
            return out;
        };
        for entry in rd.flatten() {
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if is_hidden(&file_name)
                || !file_name.ends_with(".toml")
                || secrets::is_companion(&file_name)
                || path.is_dir()
            {
                continue;
            }
            let parsed = Store::read_versioned(&path).and_then(|text| {
                toml::from_str::<EnvFile>(&text).map_err(|e| StoreError::Parse {
                    path: path.clone(),
                    msg: e.to_string(),
                })
            });
            // An unreadable local file skips the environment too, so a save can never
            // replace secrets it could not read.
            let local = secrets::read_local(&path);
            if let (Ok(f), Ok(local)) = (parsed, local) {
                out.push(EnvEntry {
                    env: Environment {
                        name: f.name,
                        variables: secrets::merge(f.variables, local),
                    },
                    path,
                });
            }
        }
        out.sort_by_key(|e| e.env.name.to_lowercase());
        out
    }

    /// Save an environment as its shareable `<slug>.toml` plus the private
    /// `<slug>.local.toml` holding the secret values (G-14). `replace` is the file it was
    /// loaded from (overwritten in place, or removed with its local file if the new name maps
    /// to a different file name). A pre-split file that still holds secrets in plain text is
    /// copied to `<slug>.toml.bak` once before it is rewritten.
    pub fn save_environment(
        &self,
        env: &Environment,
        replace: Option<&Path>,
    ) -> Result<PathBuf, StoreError> {
        let dir = self.env_dir();
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        ensure_gitignore(&dir, ENV_GITIGNORE)?;
        let target = unique_path(&dir, &slug(&env.name), ".toml", replace);
        if let Some(old) = replace {
            secrets::backup_once(old)?;
        }
        secrets::write_split(&target, &env.variables, |vars| {
            toml::to_string_pretty(&EnvFileRef {
                schema_version: SCHEMA_VERSION,
                name: &env.name,
                variables: vars,
            })
            .map_err(|e| StoreError::Serialize {
                path: target.clone(),
                msg: e.to_string(),
            })
        })?;
        if let Some(old) = replace {
            if old != target {
                for f in [old.to_path_buf(), local_path(old)] {
                    if f.exists() {
                        fs::remove_file(&f).map_err(io(&f))?;
                    }
                }
            }
        }
        Ok(target)
    }

    /// Delete an environment file and its local (secrets) file. A backup is left alone.
    pub fn delete_environment(&self, path: &Path) -> Result<(), StoreError> {
        fs::remove_file(path).map_err(io(path))?;
        let local = local_path(path);
        if local.exists() {
            fs::remove_file(&local).map_err(io(&local))?;
        }
        Ok(())
    }

    /// Global variables, with secret values from `globals.local.toml`; a missing file means
    /// none.
    pub fn load_globals(&self) -> Result<Vec<EnvVar>, StoreError> {
        let path = self.globals_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = Store::read_versioned(&path)?;
        let f: GlobalsFile = toml::from_str(&text).map_err(|e| StoreError::Parse {
            path: path.clone(),
            msg: e.to_string(),
        })?;
        Ok(secrets::merge(f.variables, secrets::read_local(&path)?))
    }

    /// Save the globals the same way as an environment: secret values go to
    /// `globals.local.toml` (G-14).
    pub fn save_globals(&self, vars: &[EnvVar]) -> Result<(), StoreError> {
        let path = self.globals_path();
        if vars.iter().any(|v| v.secret) {
            ensure_gitignore(&self.data_dir, GLOBALS_GITIGNORE)?;
        }
        if path.exists() {
            secrets::backup_once(&path)?;
        }
        secrets::write_split(&path, vars, |vars| {
            toml::to_string_pretty(&GlobalsFileRef {
                schema_version: SCHEMA_VERSION,
                variables: vars,
            })
            .map_err(|e| StoreError::Serialize {
                path: path.clone(),
                msg: e.to_string(),
            })
        })
    }

    /// Settings; a missing or unreadable file gives the defaults (settings are never worth a crash).
    pub fn load_settings(&self) -> Settings {
        let path = self.settings_path();
        Store::read_versioned(&path)
            .ok()
            .and_then(|t| toml::from_str::<Settings>(&t).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), StoreError> {
        let path = self.settings_path();
        let text = toml::to_string_pretty(s).map_err(|e| StoreError::Serialize {
            path: path.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&path, &text)
    }

    pub fn session_path(&self) -> PathBuf {
        self.data_dir.join("session.toml")
    }

    /// The last session, `None` if there is none yet. A file from a newer app version is an
    /// error (the caller must then not overwrite it); a corrupt one is an error too.
    pub fn load_session(&self) -> Result<Option<Session>, StoreError> {
        let path = self.session_path();
        if !path.exists() {
            return Ok(None);
        }
        let text = Store::read_versioned(&path)?;
        toml::from_str::<Session>(&text)
            .map(Some)
            .map_err(|e| StoreError::Parse {
                path,
                msg: e.to_string(),
            })
    }

    pub fn save_session(&self, s: &Session) -> Result<(), StoreError> {
        let path = self.session_path();
        let text = toml::to_string_pretty(s).map_err(|e| StoreError::Serialize {
            path: path.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::{ApiKeyLocation, Auth, Body, FormField, FormValue, KeyValue, Method, OAuth2};

    fn without_uid(mut r: Request) -> Request {
        r.uid.clear();
        r
    }

    // ---- files written before uid / oauth2 / digest / graphql / folder.toml existed ----

    const OLD_REQUEST: &str = r#"schema_version = 1

[request]
id = ""
name = "Legacy"
method = "POST"
url = "https://{{host}}/users"
pre_request_script = 'pm.variables.set("a", 1);'

[[request.params]]
key = "q"
value = "1"
description = ""
enabled = true

[[request.headers]]
key = "X-A"
value = "b"
description = ""
enabled = true

[request.body]
type = "json"
raw = """
{
  "a": 1
}"""

[request.auth]
type = "basic"
username = "u"
password = "p"

[request.settings]
timeout_ms = 30000
follow_redirects = true
verify_tls = true
"#;

    const OLD_SESSION: &str = r#"schema_version = 1
active = 0

[[tabs]]
path = "c/a.toml"
unsaved = true

[tabs.draft]
id = ""
name = "Legacy"
method = "POST"
url = "https://{{host}}/users"

[tabs.draft.body]
type = "text"
raw = "hi"

[tabs.draft.auth]
type = "jwt"
token = "t"
prefix = "JWT"

[tabs.draft.settings]
timeout_ms = 30000
follow_redirects = true
verify_tls = true
"#;

    const OLD_COLLECTION: &str = "schema_version = 1\nname = \"Old API\"\ndescription = \"\"\n";

    #[test]
    fn old_request_file_loads_and_resave_only_adds_uid() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        let path = col.dir.join("legacy.toml");
        fs::write(&path, OLD_REQUEST).unwrap();

        let r = Store::load_request(&path).unwrap();
        assert_eq!(r.uid, "");
        assert_eq!(r.name, "Legacy");
        assert_eq!(
            r.auth,
            Auth::Basic {
                username: "u".into(),
                password: "p".into()
            }
        );

        let saved = Store::save_request(&col.dir, &r, Some(&path)).unwrap();
        assert_eq!(saved, path);
        let text = fs::read_to_string(&path).unwrap();
        let uid = Store::load_request(&path).unwrap().uid;
        assert_eq!(uid.len(), 36, "uuid v4 text: {uid}");
        let expected =
            OLD_REQUEST.replacen("[request]\n", &format!("[request]\nuid = \"{uid}\"\n"), 1);
        assert_eq!(text, expected);

        // saving again changes nothing
        let again = Store::load_request(&path).unwrap();
        Store::save_request(&col.dir, &again, Some(&path)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    }

    #[test]
    fn old_session_and_settings_still_load() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        fs::write(store.session_path(), OLD_SESSION).unwrap();
        let s = store.load_session().unwrap().unwrap();
        assert_eq!(s.tabs.len(), 1);
        let d = &s.tabs[0].draft;
        assert_eq!(d.uid, "");
        assert_eq!(d.body, Body::Text { raw: "hi".into() });
        // re-saving a session doesn't add a uid (drafts aren't request files)
        store.save_session(&s).unwrap();
        assert!(!fs::read_to_string(store.session_path())
            .unwrap()
            .contains("uid"));

        fs::write(
            t.path().join("settings.toml"),
            "schema_version = 1\nactive_environment = \"Dev\"\n",
        )
        .unwrap();
        assert_eq!(
            store.load_settings().active_environment.as_deref(),
            Some("Dev")
        );
    }

    #[test]
    fn uid_survives_rename_and_move() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        let p1 = Store::save_request(&col.dir, &req("First", "https://a.test"), None).unwrap();
        let uid = Store::load_request(&p1).unwrap().uid;
        assert!(!uid.is_empty());

        // the editor may not know the uid: it comes from the file being replaced
        let p2 =
            Store::save_request(&col.dir, &req("Renamed", "https://a.test"), Some(&p1)).unwrap();
        assert!(!p1.exists());
        assert_eq!(Store::load_request(&p2).unwrap().uid, uid);

        let folder = Store::create_folder(&col.dir, "Sub").unwrap();
        let p3 = Store::move_request(&p2, &folder).unwrap();
        assert!(!p2.exists());
        assert_eq!(p3.parent().unwrap(), folder);
        assert_eq!(Store::load_request(&p3).unwrap().uid, uid);

        // an explicit uid is kept; separate new requests get different ones
        let other = Store::save_request(&col.dir, &req("Other", "https://a.test"), None).unwrap();
        assert_ne!(Store::load_request(&other).unwrap().uid, uid);
        let fixed = Request {
            uid: "fixed".into(),
            ..req("Fixed", "https://a.test")
        };
        let p4 = Store::save_request(&col.dir, &fixed, None).unwrap();
        assert_eq!(Store::load_request(&p4).unwrap().uid, "fixed");
    }

    #[test]
    fn collection_and_folder_meta_roundtrip_and_stay_out_of_the_tree() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("Meta API").unwrap();
        // what create_collection writes is unchanged
        assert_eq!(
            fs::read_to_string(col.dir.join(COLLECTION_FILE)).unwrap(),
            "schema_version = 1\nname = \"Meta API\"\ndescription = \"\"\n"
        );
        let cm = CollectionMeta {
            name: "Meta API".into(),
            description: "All the things".into(),
            variables: vec![EnvVar::new("base", "https://m.test")],
            headers: vec![KeyValue::new("X-Team", "core")],
            auth: Auth::Bearer {
                token: "{{token}}".into(),
            },
            pre_request_script: "console.log(1)".into(),
            test_script: String::new(),
            trusted: false,
        };
        Store::save_collection_meta(&col.dir, &cm).unwrap();
        assert_eq!(Store::load_collection_meta(&col.dir).unwrap(), cm);

        let folder = Store::create_folder(&col.dir, "Users").unwrap();
        assert_eq!(
            Store::load_folder_meta(&folder).unwrap(),
            FolderMeta::default()
        );
        let fm = FolderMeta {
            description: "user endpoints".into(),
            auth: Auth::oauth2(OAuth2 {
                client_id: "cid".into(),
                ..Default::default()
            }),
            test_script: "pm.test('x', () => {})".into(),
            ..Default::default()
        };
        Store::save_folder_meta(&folder, &fm).unwrap();
        assert_eq!(Store::load_folder_meta(&folder).unwrap(), fm);
        let text = fs::read_to_string(folder.join(FOLDER_FILE)).unwrap();
        assert!(text.starts_with("schema_version = 1\n"), "{text}");
        // a default folder file is just the version
        let empty = Store::create_folder(&col.dir, "Empty").unwrap();
        Store::save_folder_meta(&empty, &FolderMeta::default()).unwrap();
        assert_eq!(
            fs::read_to_string(empty.join(FOLDER_FILE)).unwrap(),
            "schema_version = 1\n"
        );

        Store::save_request(&folder, &req("List", "https://x.test"), None).unwrap();
        let all = store.load_all().unwrap();
        assert_eq!(all[0].name, "Meta API");
        let Node::Folder { children, .. } = all[0]
            .children
            .iter()
            .find(|n| n.name() == "Users")
            .unwrap()
        else {
            panic!()
        };
        let names: Vec<&str> = children.iter().map(|n| n.name()).collect();
        assert_eq!(names, vec!["List"], "folder.toml is not a request");
        assert!(all[0]
            .children
            .iter()
            .all(|n| !matches!(n, Node::Broken { .. })));
    }

    #[test]
    fn old_collection_file_and_missing_or_bad_meta() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let dir = store.root().join("old-api");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(COLLECTION_FILE), OLD_COLLECTION).unwrap();
        let m = Store::load_collection_meta(&dir).unwrap();
        assert_eq!(m.name, "Old API");
        assert_eq!(m.auth, Auth::None);
        assert!(m.variables.is_empty());
        assert_eq!(store.load_all().unwrap()[0].name, "Old API");

        // no file: the directory name
        let bare = store.root().join("bare");
        fs::create_dir_all(&bare).unwrap();
        assert_eq!(Store::load_collection_meta(&bare).unwrap().name, "bare");

        // newer and corrupt files are errors, never silently defaults
        fs::write(bare.join(FOLDER_FILE), "schema_version = 99\n").unwrap();
        assert!(matches!(
            Store::load_folder_meta(&bare),
            Err(StoreError::TooNew { found: 99, .. })
        ));
        fs::write(bare.join(COLLECTION_FILE), "= =").unwrap();
        assert!(Store::load_collection_meta(&bare).is_err());
        // ... but the collection still lists, under its directory name
        assert!(store.load_all().unwrap().iter().any(|c| c.name == "bare"));
    }

    #[test]
    fn requests_named_like_settings_files() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        let p = Store::save_request(&col.dir, &req("Collection", "https://a.test"), None).unwrap();
        assert_eq!(p.file_name().unwrap(), "collection-request.toml");
        let folder = Store::create_folder(&col.dir, "F").unwrap();
        let p = Store::save_request(&folder, &req("Folder", "https://a.test"), None).unwrap();
        assert_eq!(p.file_name().unwrap(), "folder-request.toml");
        assert_eq!(Store::load_collection_meta(&col.dir).unwrap().name, "C");

        // an older request saved as folder.toml is still listed, and not taken for settings
        let legacy = Store::create_folder(&col.dir, "Legacy").unwrap();
        fs::write(legacy.join(FOLDER_FILE), OLD_REQUEST).unwrap();
        let tree = Store::load_tree(&legacy).unwrap();
        assert!(matches!(&tree[..], [Node::Request { name, .. }] if name == "Legacy"));
        assert_eq!(
            Store::load_folder_meta(&legacy).unwrap(),
            FolderMeta::default()
        );
        assert!(Store::save_folder_meta(&legacy, &FolderMeta::default()).is_err());
        assert!(fs::read_to_string(legacy.join(FOLDER_FILE))
            .unwrap()
            .contains("[request]"));
    }

    #[test]
    fn new_variants_roundtrip_through_toml() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        for (i, (auth, body)) in [
            (
                Auth::oauth2(OAuth2 {
                    token_url: "https://t.test".into(),
                    scope: "read write".into(),
                    ..Default::default()
                }),
                Body::GraphQl {
                    query: "query {\n  me { id }\n}".into(),
                    variables: "{\"a\": 1}".into(),
                },
            ),
            (
                Auth::Digest {
                    username: "u".into(),
                    password: "p".into(),
                },
                Body::None,
            ),
            (Auth::Inherit, Body::None),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Request {
                auth,
                body,
                ..req(&format!("r{i}"), "https://a.test")
            };
            let p = Store::save_request(&col.dir, &r, None).unwrap();
            assert_eq!(without_uid(Store::load_request(&p).unwrap()), r);
        }
        let text = fs::read_to_string(col.dir.join("r0.toml")).unwrap();
        assert!(text.contains("[request.auth]\ntype = \"oauth2\""), "{text}");
        assert!(
            text.contains("[request.body]\ntype = \"graphql\""),
            "{text}"
        );
    }

    fn req(name: &str, url: &str) -> Request {
        Request {
            name: name.into(),
            url: url.into(),
            ..Default::default()
        }
    }

    #[test]
    fn scripts_roundtrip_and_stay_readable() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("S").unwrap();
        let r = Request {
            name: "Scripted".into(),
            url: "https://s.test".into(),
            pre_request_script: "pm.variables.set(\"ts\", Date.now());\n// done".into(),
            test_script: "pm.test(\"ok\", () => pm.response.to.be.ok);".into(),
            headers: vec![KeyValue::new("A", "1")],
            ..Default::default()
        };
        let path = Store::save_request(&col.dir, &r, None).unwrap();
        assert_eq!(without_uid(Store::load_request(&path).unwrap()), r);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("pre_request_script"), "{text}");
        // requests without scripts don't grow empty keys
        let plain = Store::save_request(&col.dir, &req("Plain", "https://p.test"), None).unwrap();
        assert!(!std::fs::read_to_string(plain).unwrap().contains("script"));
    }

    #[test]
    fn session_roundtrip_missing_corrupt_and_too_new() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        assert_eq!(store.load_session().unwrap(), None);

        let rich = Request {
            name: "Draft".into(),
            method: Method::Post,
            url: "https://{{host}}/x".into(),
            headers: vec![KeyValue::new("A", "1")],
            body: Body::FormData {
                fields: vec![FormField {
                    key: "f".into(),
                    value: FormValue::File {
                        path: "/tmp/a.png".into(),
                    },
                    enabled: true,
                }],
            },
            auth: Auth::Basic {
                username: "u".into(),
                password: "p".into(),
            },
            ..Default::default()
        };
        let s = Session {
            active: 1,
            tabs: vec![
                SessionTab {
                    path: Some(t.path().join("collections/c/a.toml")),
                    save_dir: None,
                    unsaved: false,
                    draft: req("A", "https://a.test"),
                },
                SessionTab {
                    path: None,
                    save_dir: Some(t.path().join("collections/c")),
                    unsaved: true,
                    draft: rich,
                },
            ],
            ..Default::default()
        };
        store.save_session(&s).unwrap();
        assert_eq!(store.load_session().unwrap(), Some(s.clone()));

        std::fs::write(store.session_path(), "= nope").unwrap();
        assert!(store.load_session().is_err());

        std::fs::write(store.session_path(), "schema_version = 99\n").unwrap();
        assert!(matches!(
            store.load_session(),
            Err(StoreError::TooNew { found: 99, .. })
        ));
    }

    #[test]
    fn folder_names_stay_readable_and_safe() {
        assert_eq!(folder_name("User Management"), "User Management");
        assert_eq!(folder_name("  a/b:c?  "), "a-b-c-");
        assert_eq!(folder_name("..hidden"), "hidden");
        assert_eq!(folder_name("trailing. . "), "trailing");
        assert_eq!(folder_name("   "), "Untitled folder");
        assert_eq!(folder_name("CON"), "CON folder");
        assert_eq!(folder_name("Console"), "Console");
    }

    #[test]
    fn create_folder_dedupes_and_shows_in_tree() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("API").unwrap();
        let a = Store::create_folder(&col.dir, "Users").unwrap();
        let b = Store::create_folder(&col.dir, "Users").unwrap();
        assert_eq!(a.file_name().unwrap(), "Users");
        assert_eq!(b.file_name().unwrap(), "Users 2");
        Store::save_request(&a, &req("List", "https://x.test"), None).unwrap();
        let tree = Store::load_tree(&col.dir).unwrap();
        assert!(
            matches!(&tree[0], Node::Folder { name, children, .. } if name == "Users" && children.len() == 1)
        );
        assert_eq!(store.data_dir(), t.path());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Get Users!"), "get-users");
        assert_eq!(slug("  "), "untitled");
        assert_eq!(slug("a//b\\c:d"), "a-b-c-d");
        assert_eq!(slug("CON"), "con-file");
        assert_eq!(slug("com1"), "com1-file");
        assert_eq!(slug("Ünï cödé"), "ünï-cödé");
    }

    #[test]
    fn roundtrip_rich_request() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("My API").unwrap();
        let r = Request {
            id: "abc".into(),
            name: "Create user".into(),
            method: Method::Post,
            url: "https://{{host}}/users?x=1".into(),
            params: vec![
                KeyValue::new("q", "a b"),
                KeyValue {
                    enabled: false,
                    ..KeyValue::new("off", "1")
                },
            ],
            headers: vec![KeyValue::new("X-Test", "hi")],
            body: Body::Json {
                raw: "{\n  \"a\": \"line1\\nline2\",\n  \"q\": \"\\\"quoted\\\"\"\n}".into(),
            },
            auth: Auth::ApiKey {
                key: "k".into(),
                value: "v".into(),
                location: ApiKeyLocation::Query,
            },
            ..Default::default()
        };
        let path = Store::save_request(&col.dir, &r, None).unwrap();
        assert_eq!(path.file_name().unwrap(), "create-user.toml");
        assert_eq!(without_uid(Store::load_request(&path).unwrap()), r);

        let form = Request {
            name: "Upload".into(),
            body: Body::FormData {
                fields: vec![
                    FormField {
                        key: "t".into(),
                        value: FormValue::Text { value: "v".into() },
                        enabled: true,
                    },
                    FormField {
                        key: "f".into(),
                        value: FormValue::File {
                            path: "/tmp/x".into(),
                        },
                        enabled: true,
                    },
                ],
            },
            url: "https://x.test".into(),
            ..Default::default()
        };
        let p2 = Store::save_request(&col.dir, &form, None).unwrap();
        assert_eq!(without_uid(Store::load_request(&p2).unwrap()), form);
    }

    #[test]
    fn duplicate_names_get_suffix_and_replace_overwrites() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        let a = Store::save_request(&col.dir, &req("Same", "https://a.test"), None).unwrap();
        let b = Store::save_request(&col.dir, &req("Same", "https://b.test"), None).unwrap();
        assert_ne!(a, b);
        assert_eq!(b.file_name().unwrap(), "same-2.toml");
        // saving again with replace=a overwrites a in place
        let a2 =
            Store::save_request(&col.dir, &req("Same", "https://changed.test"), Some(&a)).unwrap();
        assert_eq!(a, a2);
        assert_eq!(Store::load_request(&a).unwrap().url, "https://changed.test");
    }

    #[test]
    fn rename_moves_file() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        let old = Store::save_request(&col.dir, &req("Old name", "https://a.test"), None).unwrap();
        let new =
            Store::save_request(&col.dir, &req("New name", "https://a.test"), Some(&old)).unwrap();
        assert!(!old.exists());
        assert!(new.exists());
    }

    #[test]
    fn tree_is_sorted_folders_first() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        Store::save_request(&col.dir, &req("zeta", "https://a.test"), None).unwrap();
        Store::save_request(&col.dir, &req("Alpha", "https://a.test"), None).unwrap();
        let folder = col.dir.join("users");
        Store::save_request(&folder, &req("List", "https://a.test"), None).unwrap();
        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 1);
        let names: Vec<&str> = all[0].children.iter().map(|n| n.name()).collect();
        assert_eq!(names, vec!["users", "Alpha", "zeta"]);
        match &all[0].children[0] {
            Node::Folder { children, .. } => assert_eq!(children[0].name(), "List"),
            other => panic!("expected folder, got {other:?}"),
        }
    }

    #[test]
    fn broken_and_too_new_files_do_not_panic() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        fs::write(col.dir.join("bad.toml"), "this is = = not toml").unwrap();
        fs::write(
            col.dir.join("future.toml"),
            "schema_version = 99\n[request]\nurl = \"x\"\n",
        )
        .unwrap();
        let all = store.load_all().unwrap();
        let kinds: Vec<bool> = all[0]
            .children
            .iter()
            .map(|n| matches!(n, Node::Broken { .. }))
            .collect();
        assert_eq!(kinds, vec![true, true]);
        let err = Store::load_request(&col.dir.join("future.toml")).unwrap_err();
        assert!(
            matches!(err, StoreError::TooNew { found: 99, .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn no_temp_files_left_behind() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let col = store.create_collection("C").unwrap();
        for i in 0..5 {
            Store::save_request(&col.dir, &req(&format!("r{i}"), "https://a.test"), None).unwrap();
        }
        let leftovers: Vec<_> = fs::read_dir(&col.dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn collections_get_unique_dirs_but_keep_display_names() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let a = store.create_collection("Work API").unwrap();
        let b = store.create_collection("Work API").unwrap();
        assert_ne!(a.dir_name, b.dir_name);
        let all = store.load_all().unwrap();
        assert!(all.iter().all(|c| c.name == "Work API"));
        assert_eq!(all.len(), 2);
    }
    #[test]
    fn environments_roundtrip_rename_and_delete() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        assert!(store.list_environments().is_empty());
        let env = Environment {
            name: "Staging".into(),
            variables: vec![
                EnvVar::new("host", "staging.test"),
                EnvVar {
                    secret: true,
                    ..EnvVar::new("token", "s3cr\"et")
                },
            ],
        };
        let path = store.save_environment(&env, None).unwrap();
        assert!(path.ends_with("environments/staging.toml"));
        let list = store.list_environments();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].env, env);

        // rename moves the file
        let renamed = Environment {
            name: "Prod".into(),
            ..env.clone()
        };
        let p2 = store.save_environment(&renamed, Some(&path)).unwrap();
        assert!(!path.exists());
        assert!(p2.ends_with("environments/prod.toml"));
        assert_eq!(store.list_environments().len(), 1);

        store.delete_environment(&p2).unwrap();
        assert!(store.list_environments().is_empty());
    }

    #[test]
    fn same_environment_name_gets_a_second_file() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let env = Environment {
            name: "Dev".into(),
            variables: vec![],
        };
        let a = store.save_environment(&env, None).unwrap();
        let b = store.save_environment(&env, None).unwrap();
        assert_ne!(a, b);
        assert_eq!(store.list_environments().len(), 2);
    }

    #[test]
    fn broken_environment_file_is_skipped_not_fatal() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        store
            .save_environment(
                &Environment {
                    name: "Ok".into(),
                    variables: vec![],
                },
                None,
            )
            .unwrap();
        fs::write(t.path().join("environments/bad.toml"), "= =").unwrap();
        fs::write(
            t.path().join("environments/new.toml"),
            "schema_version = 99\nname = \"Future\"\n",
        )
        .unwrap();
        let list = store.list_environments();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].env.name, "Ok");
    }

    #[test]
    fn globals_missing_means_empty_and_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        assert!(store.load_globals().unwrap().is_empty());
        let vars = vec![EnvVar::new("a", "1"), EnvVar::new("b", "line1\nline2")];
        store.save_globals(&vars).unwrap();
        assert_eq!(store.load_globals().unwrap(), vars);
        // saving an empty list is valid too
        store.save_globals(&[]).unwrap();
        assert!(store.load_globals().unwrap().is_empty());
    }

    #[test]
    fn corrupt_globals_is_an_error_not_silent() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        fs::write(t.path().join("globals.toml"), "= =").unwrap();
        assert!(store.load_globals().is_err());
    }

    #[test]
    fn settings_default_roundtrip_and_bad_file() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        assert_eq!(store.load_settings(), Settings::default());
        let s = Settings {
            active_environment: Some("Prod".into()),
            ..Default::default()
        };
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings(), s);
        store.save_settings(&Settings::default()).unwrap();
        assert_eq!(store.load_settings().active_environment, None);
        fs::write(t.path().join("settings.toml"), "= =").unwrap();
        assert_eq!(store.load_settings(), Settings::default());
    }

    #[test]
    fn settings_new_fields_roundtrip() {
        let s = Settings {
            default_timeout_ms: 5000,
            verify_tls: false,
            history_cap: 50,
            proxy: "http://localhost:8080".into(),
            ..Default::default()
        };
        let text = toml::to_string(&s).unwrap();
        let back: Settings = toml::from_str(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn settings_old_format_gets_new_defaults() {
        let old = r#"
schema_version = 1
active_environment = "Prod"
"#;
        let s: Settings = toml::from_str(old).unwrap();
        assert_eq!(s.active_environment, Some("Prod".into()));
        assert_eq!(s.default_timeout_ms, 30000);
        assert!(s.verify_tls);
        assert_eq!(s.history_cap, 1000);
        assert_eq!(s.proxy, "");
        assert_eq!(s.ca_bundle, "");
        assert_eq!(s.min_tls, "");
        assert!(s.client_certs.is_empty());
    }

    #[test]
    fn settings_tls_fields_roundtrip_through_the_file() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let s = Settings {
            active_environment: Some("Prod".into()),
            proxy: "http://proxy.test:8080".into(),
            ca_bundle: "C:/certs/corp-ca.pem".into(),
            min_tls: "1.3".into(),
            client_certs: vec![
                ClientCertSetting {
                    host: "api.example.com".into(),
                    path: "/certs/client.pem".into(),
                    password: None,
                },
                ClientCertSetting {
                    host: "legacy.example.com".into(),
                    path: "/certs/legacy.p12".into(),
                    password: Some("pw".into()),
                },
            ],
            ..Default::default()
        };
        store.save_settings(&s).unwrap();
        assert_eq!(store.load_settings(), s);
        // the defaults add nothing to the file
        store.save_settings(&Settings::default()).unwrap();
        let text = fs::read_to_string(t.path().join("settings.toml")).unwrap();
        assert!(
            !text.contains("ca_bundle") && !text.contains("client_certs"),
            "{text}"
        );
    }
}
