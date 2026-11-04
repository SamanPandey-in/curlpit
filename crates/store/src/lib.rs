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

use at_core::{EnvVar, Environment, Request};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
const COLLECTION_FILE: &str = "collection.toml";

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
}

fn current_schema() -> u32 {
    SCHEMA_VERSION
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_environment: None,
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

#[derive(Serialize, Deserialize)]
struct CollectionFile {
    schema_version: u32,
    name: String,
    #[serde(default)]
    description: String,
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
        let file = CollectionFile {
            schema_version: SCHEMA_VERSION,
            name: name.to_string(),
            description: String::new(),
        };
        let text = toml::to_string_pretty(&file).map_err(|e| StoreError::Serialize {
            path: dir.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&dir.join(COLLECTION_FILE), &text)?;
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
            let name = fs::read_to_string(dir.join(COLLECTION_FILE))
                .ok()
                .and_then(|t| toml::from_str::<CollectionFile>(&t).ok())
                .map(|c| c.name)
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
            } else if file_name.ends_with(".toml") && file_name != COLLECTION_FILE {
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
    pub fn save_request(
        dir: &Path,
        req: &Request,
        replace: Option<&Path>,
    ) -> Result<PathBuf, StoreError> {
        let target = unique_path(dir, &slug(&req.name), ".toml", replace);
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

    fn read_versioned(path: &Path) -> Result<String, StoreError> {
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
            if is_hidden(&file_name) || !file_name.ends_with(".toml") || path.is_dir() {
                continue;
            }
            let parsed = Store::read_versioned(&path).and_then(|text| {
                toml::from_str::<EnvFile>(&text).map_err(|e| StoreError::Parse {
                    path: path.clone(),
                    msg: e.to_string(),
                })
            });
            if let Ok(f) = parsed {
                out.push(EnvEntry {
                    env: Environment {
                        name: f.name,
                        variables: f.variables,
                    },
                    path,
                });
            }
        }
        out.sort_by_key(|e| e.env.name.to_lowercase());
        out
    }

    /// Save an environment. `replace` is the file it was loaded from (overwritten in place, or
    /// removed if the new name maps to a different file name).
    pub fn save_environment(
        &self,
        env: &Environment,
        replace: Option<&Path>,
    ) -> Result<PathBuf, StoreError> {
        let dir = self.env_dir();
        fs::create_dir_all(&dir).map_err(io(&dir))?;
        let target = unique_path(&dir, &slug(&env.name), ".toml", replace);
        let text = toml::to_string_pretty(&EnvFileRef {
            schema_version: SCHEMA_VERSION,
            name: &env.name,
            variables: &env.variables,
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

    pub fn delete_environment(&self, path: &Path) -> Result<(), StoreError> {
        fs::remove_file(path).map_err(io(path))
    }

    /// Global variables; a missing file means none.
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
        Ok(f.variables)
    }

    pub fn save_globals(&self, vars: &[EnvVar]) -> Result<(), StoreError> {
        let path = self.globals_path();
        let text = toml::to_string_pretty(&GlobalsFileRef {
            schema_version: SCHEMA_VERSION,
            variables: vars,
        })
        .map_err(|e| StoreError::Serialize {
            path: path.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&path, &text)
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
    use at_core::{ApiKeyLocation, Auth, Body, FormField, FormValue, KeyValue, Method};

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
        assert_eq!(Store::load_request(&path).unwrap(), r);
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
        assert_eq!(Store::load_request(&path).unwrap(), r);

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
        assert_eq!(Store::load_request(&p2).unwrap(), form);
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
}
