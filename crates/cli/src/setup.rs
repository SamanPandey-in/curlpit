//! Turning the command line into a `RunPlan`: what to run, the data dir, environment,
//! globals and data file. Every problem here is a usage/config error (exit code 2).

use crate::RunArgs;
use at_core::{to_map, EnvVar, Environment, Variables};
use at_runner::{run_order, RunPlan};
use at_store::Store;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) struct Setup {
    /// Report name: the collection, folder or request.
    pub name: String,
    pub plan: Option<RunPlan>,
    /// The data dir, when there is one (it isn't created for a CLI run).
    pub store: Option<Store>,
    /// `--env` as stored, for `--persist-vars`.
    pub stored_env: Option<(Environment, PathBuf)>,
    pub stored_globals: Vec<EnvVar>,
}

/// An environment or globals file: the app's own format (`schema_version`, `name`,
/// `[[variables]]` with `key`, `value`, optional `enabled`/`secret`).
#[derive(Deserialize)]
struct VarFile {
    #[serde(default)]
    name: String,
    #[serde(default)]
    variables: Vec<EnvVar>,
}

fn read_var_file(path: &Path) -> Result<VarFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The data dir: `--data-dir`, else `$API_TESTER_DATA_DIR`, else the OS location the app uses
/// (mirrors `Store::open_default`, which would also create it).
fn data_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os("API_TESTER_DATA_DIR") {
        return Some(PathBuf::from(p));
    }
    directories::ProjectDirs::from("dev", "api-tester", "API Tester")
        .map(|d| d.data_dir().to_path_buf())
}

/// (report name, requests in run order).
type Target = (String, Vec<(String, Result<PathBuf, String>)>);

fn target(path: &Path, store: Option<&Store>) -> Result<Target, String> {
    if path.is_file() {
        let req = Store::load_request(path).map_err(|e| e.to_string())?;
        let name = if req.name.trim().is_empty() {
            stem(path)
        } else {
            req.name
        };
        return Ok((name.clone(), vec![(name, Ok(path.to_path_buf()))]));
    }
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        // a collection of the data dir, by display or directory name
        let wanted = path.to_string_lossy();
        let found = store
            .and_then(|s| s.load_all().ok())
            .into_iter()
            .flatten()
            .find(|c| c.name == wanted || c.dir_name == wanted)
            .or_else(|| {
                store
                    .and_then(|s| s.load_all().ok())
                    .into_iter()
                    .flatten()
                    .find(|c| c.name.eq_ignore_ascii_case(&wanted))
            });
        match found {
            Some(c) => c.dir,
            None => {
                return Err(format!(
                    "{}: no such file, directory or collection",
                    path.display()
                ))
            }
        }
    };
    let name = Store::load_collection_meta(&dir)
        .ok()
        .map(|m| m.name)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| stem(&dir));
    let tree = Store::load_tree(&dir).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    run_order(&tree, &mut files);
    Ok((name, files))
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

pub(crate) fn prepare(args: &RunArgs) -> Result<Setup, String> {
    if let Some(d) = &args.data_dir {
        if !d.is_dir() {
            return Err(format!("{}: data dir not found", d.display()));
        }
    }
    let store = data_dir(args.data_dir.as_deref())
        .filter(|d| d.is_dir())
        .map(Store::open_at)
        .transpose()
        .map_err(|e| e.to_string())?;

    let (name, files) = target(&args.path, store.as_ref())?;
    if files.is_empty() {
        return Err(format!("{}: no requests to run", args.path.display()));
    }

    // environment: --env from the data dir, then --env-file over it
    let mut env_name = None;
    let mut env_vars: Vec<EnvVar> = Vec::new();
    let mut stored_env = None;
    if let Some(wanted) = &args.env {
        let Some(s) = &store else {
            return Err(format!(
                "--env {wanted}: there is no data dir (use --data-dir or --env-file)"
            ));
        };
        let envs = s.list_environments();
        let found = envs.iter().find(|e| &e.env.name == wanted).or_else(|| {
            envs.iter()
                .find(|e| e.env.name.eq_ignore_ascii_case(wanted))
        });
        let Some(e) = found else {
            let names: Vec<_> = envs.iter().map(|e| e.env.name.as_str()).collect();
            return Err(format!(
                "no environment named \"{wanted}\" in {} (have: {})",
                s.data_dir().display(),
                if names.is_empty() {
                    "none".to_string()
                } else {
                    names.join(", ")
                }
            ));
        };
        env_name = Some(e.env.name.clone());
        env_vars = e.env.variables.clone();
        stored_env = Some((e.env.clone(), e.path.clone()));
    }
    if let Some(f) = &args.env_file {
        let file = read_var_file(f)?;
        if !file.name.trim().is_empty() || env_name.is_none() {
            env_name = Some(if file.name.trim().is_empty() {
                stem(f)
            } else {
                file.name
            });
        }
        env_vars.extend(file.variables);
    }

    // globals: the data dir's, then --global-file over them
    let stored_globals = match &store {
        Some(s) => s.load_globals().map_err(|e| e.to_string())?,
        None => Vec::new(),
    };
    let mut global_vars = stored_globals.clone();
    if let Some(f) = &args.global_file {
        global_vars.extend(read_var_file(f)?.variables);
    }

    let data = match &args.data {
        Some(p) => {
            let rows = at_runner::data::load(p)?;
            if rows.is_empty() {
                return Err(format!("{}: the data file has no rows", p.display()));
            }
            rows
        }
        None => Vec::new(),
    };
    let iterations = match args.iterations {
        Some(n) => usize::try_from(n).map_err(|_| "--iterations is too large".to_string())?,
        None => data.len().max(1),
    };
    let proxy = store
        .as_ref()
        .map(|s| s.load_settings().proxy)
        .unwrap_or_default();

    let plan = RunPlan {
        files,
        iterations,
        delay: Duration::from_millis(args.delay_ms),
        stop_on_fail: args.bail,
        vars: Variables {
            global: to_map(&global_vars),
            environment: to_map(&env_vars),
            ..Default::default()
        },
        env_name,
        proxy,
        data,
        timeout_ms: args.timeout_ms,
        insecure: args.insecure,
        ..Default::default()
    };
    Ok(Setup {
        name,
        plan: Some(plan),
        store,
        stored_env: if args.persist_vars { stored_env } else { None },
        stored_globals,
    })
}
