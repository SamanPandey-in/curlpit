//! Shareable and local variable files (G-14).
//!
//! An environment (and the globals) is kept in two files side by side:
//!
//! * `<env>.toml`, safe to share or commit: every variable, but a secret one has an empty
//!   `value` there (its `key` and `secret = true` stay, so the shape is shared);
//! * `<env>.local.toml`, private: just the secret values, as `[[secrets]]` rows of
//!   `key` and `value`.
//!
//! Loading merges the two (the local value wins for a secret key; no local file means empty
//! secrets). A file from before the split still holds its secret values in `<env>.toml`;
//! those load as they are, and the next save rewrites the pair, first copying the old file
//! to `<env>.toml.bak` once. The directory's `.gitignore` keeps the private files out of git.
use std::fs;
use std::path::{Path, PathBuf};

use at_core::EnvVar;
use serde::{Deserialize, Serialize};

use crate::{atomic_write, io, StoreError, SCHEMA_VERSION};

/// Suffix of the private file next to `<name>.toml`.
pub const LOCAL_SUFFIX: &str = ".local.toml";
/// Suffix of the one-time backup of a pre-split file.
pub const BACKUP_SUFFIX: &str = ".toml.bak";
/// Lines the environments directory's `.gitignore` must hold.
pub const ENV_GITIGNORE: &[&str] = &["*.local.toml", "*.toml.bak"];

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub(crate) struct LocalVar {
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(Serialize)]
struct LocalFileRef<'a> {
    schema_version: u32,
    secrets: &'a [LocalVar],
}

#[derive(Deserialize)]
struct LocalFile {
    #[allow(dead_code)]
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    secrets: Vec<LocalVar>,
}

/// Just the variables of an environment or globals file.
#[derive(Deserialize)]
struct VarsOnly {
    #[serde(default)]
    variables: Vec<EnvVar>,
}

/// `dir/name.toml` -> `dir/name.local.toml`.
pub fn local_path(public: &Path) -> PathBuf {
    public.with_file_name(format!("{}{LOCAL_SUFFIX}", file_stem(public)))
}

/// `dir/name.toml` -> `dir/name.toml.bak`.
pub fn backup_path(public: &Path) -> PathBuf {
    public.with_file_name(format!("{}{BACKUP_SUFFIX}", file_stem(public)))
}

fn file_stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// True for a file that belongs next to an environment rather than being one.
pub fn is_companion(file_name: &str) -> bool {
    file_name.ends_with(LOCAL_SUFFIX) || file_name.ends_with(BACKUP_SUFFIX)
}

/// The variables as they go into the shareable file (secret values blanked) and the secret
/// values for the local file.
fn split(vars: &[EnvVar]) -> (Vec<EnvVar>, Vec<LocalVar>) {
    let mut public = Vec::with_capacity(vars.len());
    let mut secrets = Vec::new();
    for v in vars {
        if v.secret {
            secrets.push(LocalVar {
                key: v.key.clone(),
                value: v.value.clone(),
            });
            public.push(EnvVar {
                value: String::new(),
                ..v.clone()
            });
        } else {
            public.push(v.clone());
        }
    }
    (public, secrets)
}

/// Put the local values back into the secret variables. A secret key with no local value
/// keeps what the shareable file holds: empty after the split, the real value in a file from
/// before it. Repeated keys pair up in order.
pub(crate) fn merge(mut vars: Vec<EnvVar>, local: Vec<LocalVar>) -> Vec<EnvVar> {
    let mut local: Vec<Option<LocalVar>> = local.into_iter().map(Some).collect();
    for v in vars.iter_mut().filter(|v| v.secret) {
        if let Some(slot) = local
            .iter_mut()
            .find(|l| l.as_ref().is_some_and(|l| l.key == v.key))
        {
            if let Some(l) = slot.take() {
                v.value = l.value;
            }
        }
    }
    vars
}

/// The local file's secrets: empty if there is none, an error if it is unreadable or newer.
pub(crate) fn read_local(public: &Path) -> Result<Vec<LocalVar>, StoreError> {
    let path = local_path(public);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = crate::Store::read_versioned(&path)?;
    toml::from_str::<LocalFile>(&text)
        .map(|f| f.secrets)
        .map_err(|e| StoreError::Parse {
            path,
            msg: e.to_string(),
        })
}

/// Whether `public` is a pre-split file that still holds a secret value in plain text.
fn holds_plain_secrets(public: &Path) -> bool {
    fs::read_to_string(public)
        .ok()
        .and_then(|t| toml::from_str::<VarsOnly>(&t).ok())
        .is_some_and(|f| f.variables.iter().any(|v| v.secret && !v.value.is_empty()))
}

/// Before `public` is rewritten: if it still holds secrets in plain text (the old one-file
/// format), copy it to `<name>.toml.bak`, unless a backup is already there.
pub(crate) fn backup_once(public: &Path) -> Result<(), StoreError> {
    if !holds_plain_secrets(public) {
        return Ok(());
    }
    let bak = backup_path(public);
    if bak.exists() {
        return Ok(());
    }
    fs::copy(public, &bak).map_err(io(&bak))?;
    Ok(())
}

/// Write `vars` as the pair `public` + its local file. `render` turns the blanked variables
/// into the shareable file's text. The local file is written first (each write is atomic), and
/// removed when there are no secrets left, so stale secrets never linger.
pub(crate) fn write_split(
    public: &Path,
    vars: &[EnvVar],
    render: impl FnOnce(&[EnvVar]) -> Result<String, StoreError>,
) -> Result<(), StoreError> {
    let (shared, secrets) = split(vars);
    let local = local_path(public);
    if secrets.is_empty() {
        if local.exists() {
            fs::remove_file(&local).map_err(io(&local))?;
        }
    } else {
        let text = toml::to_string_pretty(&LocalFileRef {
            schema_version: SCHEMA_VERSION,
            secrets: &secrets,
        })
        .map_err(|e| StoreError::Serialize {
            path: local.clone(),
            msg: e.to_string(),
        })?;
        atomic_write(&local, &text)?;
    }
    atomic_write(public, &render(&shared)?)
}

/// Make sure `dir/.gitignore` lists every line in `lines`, appending only the missing ones
/// (an existing file's contents are kept as they are).
pub fn ensure_gitignore(dir: &Path, lines: &[&str]) -> Result<(), StoreError> {
    let path = dir.join(".gitignore");
    let mut text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io(&path)(e)),
    };
    let missing: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| !text.lines().any(|have| have.trim() == *l))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for l in missing {
        text.push_str(l);
        text.push('\n');
    }
    atomic_write(&path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;
    use at_core::Environment;

    fn secret(key: &str, value: &str) -> EnvVar {
        EnvVar {
            secret: true,
            ..EnvVar::new(key, value)
        }
    }

    fn staging() -> Environment {
        Environment {
            name: "Staging".into(),
            variables: vec![
                EnvVar::new("host", "staging.test"),
                secret("token", "s3cr\"et-TOKEN"),
                EnvVar {
                    enabled: false,
                    ..secret("password", "hunter2-PASS")
                },
            ],
        }
    }

    const OLD_ENV: &str = r#"schema_version = 1
name = "Old"

[[variables]]
key = "host"
value = "old.test"
secret = false
enabled = true

[[variables]]
key = "token"
value = "plain-OLD-SECRET"
secret = true
enabled = true
"#;

    #[test]
    fn split_roundtrip_and_secrets_never_in_the_shared_file() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let env = staging();
        let path = store.save_environment(&env, None).unwrap();
        let shared = fs::read_to_string(&path).unwrap();
        let local = fs::read_to_string(local_path(&path)).unwrap();
        for v in ["s3cr", "TOKEN", "hunter2"] {
            assert!(!shared.contains(v), "{v} leaked into {shared}");
            assert!(local.contains(v), "{v} missing from {local}");
        }
        // the shared file still says which keys are secret
        assert!(shared.contains("key = \"token\"") && shared.contains("secret = true"));
        assert!(
            !local.contains("staging.test"),
            "only secrets go local: {local}"
        );
        assert!(local_path(&path).ends_with("environments/staging.local.toml"));

        let list = store.list_environments();
        assert_eq!(list.len(), 1, "the local file is not an environment");
        assert_eq!(list[0].env, env);

        // no secrets left: the local file goes away
        let plain = Environment {
            name: "Staging".into(),
            variables: vec![EnvVar::new("host", "h")],
        };
        store.save_environment(&plain, Some(&path)).unwrap();
        assert!(!local_path(&path).exists());
        assert_eq!(store.list_environments()[0].env, plain);
    }

    #[test]
    fn missing_local_file_means_empty_secrets() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let path = store.save_environment(&staging(), None).unwrap();
        fs::remove_file(local_path(&path)).unwrap();
        let env = &store.list_environments()[0].env;
        assert_eq!(env.variables[0].value, "staging.test");
        assert!(env.variables[1].secret && env.variables[1].value.is_empty());
        assert!(env.variables[2].value.is_empty());
    }

    #[test]
    fn old_single_file_loads_then_saves_split_with_one_backup() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let dir = t.path().join("environments");
        fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.toml");
        fs::write(&old, OLD_ENV).unwrap();

        // loads as it is, secret value included
        let (env, path) = {
            let e = store.list_environments().remove(0);
            (e.env, e.path)
        };
        assert_eq!(env.variables[1].value, "plain-OLD-SECRET");
        assert!(
            !backup_path(&old).exists(),
            "loading changes nothing on disk"
        );

        // the next save splits it, keeping the old file as a backup
        let p = store.save_environment(&env, Some(&path)).unwrap();
        assert_eq!(p, old);
        assert_eq!(fs::read_to_string(backup_path(&old)).unwrap(), OLD_ENV);
        assert!(!fs::read_to_string(&old)
            .unwrap()
            .contains("plain-OLD-SECRET"));
        assert!(fs::read_to_string(local_path(&old))
            .unwrap()
            .contains("plain-OLD-SECRET"));
        assert_eq!(store.list_environments()[0].env, env);

        // a later save never touches the backup
        let mut changed = env.clone();
        changed.variables[1].value = "new-secret".into();
        store.save_environment(&changed, Some(&p)).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&old)).unwrap(), OLD_ENV);
        assert_eq!(store.list_environments()[0].env, changed);
        assert_eq!(store.list_environments().len(), 1, "the .bak is not listed");
    }

    #[test]
    fn local_value_wins_for_secret_keys_only() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let dir = t.path().join("environments");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("old.toml"), OLD_ENV).unwrap();
        fs::write(
            dir.join("old.local.toml"),
            "schema_version = 1\n[[secrets]]\nkey = \"token\"\nvalue = \"from-local\"\n\
             [[secrets]]\nkey = \"host\"\nvalue = \"ignored\"\n",
        )
        .unwrap();
        let env = &store.list_environments()[0].env;
        assert_eq!(env.variables[0].value, "old.test");
        assert_eq!(env.variables[1].value, "from-local");
    }

    #[test]
    fn unreadable_local_file_skips_the_environment() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let path = store.save_environment(&staging(), None).unwrap();
        fs::write(local_path(&path), "= =").unwrap();
        assert!(store.list_environments().is_empty());
        fs::write(local_path(&path), "schema_version = 99\n").unwrap();
        assert!(store.list_environments().is_empty());
    }

    #[test]
    fn listing_ignores_companion_files() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let dir = t.path().join("environments");
        fs::create_dir_all(&dir).unwrap();
        // companions that would parse as environments if they were listed
        fs::write(dir.join("a.local.toml"), OLD_ENV).unwrap();
        fs::write(dir.join("a.toml.bak"), OLD_ENV).unwrap();
        fs::write(dir.join(".gitignore"), "*.local.toml\n").unwrap();
        assert!(store.list_environments().is_empty());
        fs::write(dir.join("a.toml"), OLD_ENV).unwrap();
        assert_eq!(store.list_environments().len(), 1);
    }

    #[test]
    fn rename_and_delete_take_the_local_file_along() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let p = store.save_environment(&staging(), None).unwrap();
        let renamed = Environment {
            name: "Prod".into(),
            ..staging()
        };
        let p2 = store.save_environment(&renamed, Some(&p)).unwrap();
        assert!(!p.exists() && !local_path(&p).exists());
        assert!(local_path(&p2).exists());
        assert_eq!(store.list_environments()[0].env, renamed);
        store.delete_environment(&p2).unwrap();
        assert!(!local_path(&p2).exists());
        assert!(store.list_environments().is_empty());
    }

    #[test]
    fn gitignore_is_created_and_appended_idempotently() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let dir = t.path().join("environments");
        let gi = dir.join(".gitignore");
        store.save_environment(&staging(), None).unwrap();
        let first = fs::read_to_string(&gi).unwrap();
        assert_eq!(first, "*.local.toml\n*.toml.bak\n");
        store.save_environment(&staging(), None).unwrap();
        assert_eq!(fs::read_to_string(&gi).unwrap(), first);

        // someone else's file: kept, and only what is missing is added
        fs::write(&gi, "node_modules/\n*.toml.bak").unwrap();
        ensure_gitignore(&dir, ENV_GITIGNORE).unwrap();
        ensure_gitignore(&dir, ENV_GITIGNORE).unwrap();
        assert_eq!(
            fs::read_to_string(&gi).unwrap(),
            "node_modules/\n*.toml.bak\n*.local.toml\n"
        );
    }

    #[test]
    fn globals_split_and_migrate_too() {
        let t = tempfile::tempdir().unwrap();
        let store = Store::open_at(t.path()).unwrap();
        let globals = t.path().join("globals.toml");
        fs::write(
            &globals,
            "schema_version = 1\n[[variables]]\nkey = \"k\"\nvalue = \"OLD-G\"\nsecret = true\n",
        )
        .unwrap();
        let vars = store.load_globals().unwrap();
        assert_eq!(vars[0].value, "OLD-G");
        store.save_globals(&vars).unwrap();
        assert!(!fs::read_to_string(&globals).unwrap().contains("OLD-G"));
        assert!(fs::read_to_string(backup_path(&globals))
            .unwrap()
            .contains("OLD-G"));
        assert!(fs::read_to_string(local_path(&globals))
            .unwrap()
            .contains("OLD-G"));
        assert_eq!(store.load_globals().unwrap(), vars);
        let gi = fs::read_to_string(t.path().join(".gitignore")).unwrap();
        assert!(gi.contains("globals.local.toml"));
        // a corrupt local file is an error, like a corrupt globals.toml
        fs::write(local_path(&globals), "= =").unwrap();
        assert!(store.load_globals().is_err());
    }

    #[test]
    fn repeated_secret_keys_pair_up_in_order() {
        let vars = vec![secret("a", ""), EnvVar::new("a", "plain"), secret("a", "")];
        let local = vec![
            LocalVar {
                key: "a".into(),
                value: "1".into(),
            },
            LocalVar {
                key: "a".into(),
                value: "2".into(),
            },
        ];
        let out = merge(vars, local);
        let values: Vec<_> = out.iter().map(|v| v.value.as_str()).collect();
        assert_eq!(values, ["1", "plain", "2"]);
    }
}
