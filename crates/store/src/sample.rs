//! The "Getting started" sample workspace seeded on a fresh install (G-22).
//!
//! A new user should be able to press Send on the first launch and see a real response. So when
//! the data directory is brand new, one small collection of requests against `httpbin.org` (a
//! public echo service) is written, plus an `httpbin` environment holding `baseUrl`, which is
//! made the active environment.
//!
//! "Brand new" is decided from the data directory alone: it holds nothing but an empty
//! `collections/` directory (which [`Store::open_at`] creates). Any other visible entry
//! (`settings.toml`, `history.db`, `session.toml`, an environment, a collection) means the app
//! has run here before or the user put data here, and nothing is written. Seeding itself writes
//! `settings.toml` last, so deleting every collection afterwards never brings the sample back.
use std::fs;
use std::path::Path;

use at_core::{Body, CollectionMeta, EnvVar, Environment, KeyValue, Method, Request};

use crate::{io, is_hidden, Store, StoreError};

/// Display name of the seeded collection.
pub const SAMPLE_COLLECTION: &str = "Getting started";
/// Name of the seeded environment.
pub const SAMPLE_ENVIRONMENT: &str = "httpbin";
/// Value of `baseUrl` in the seeded environment.
pub const SAMPLE_BASE_URL: &str = "https://httpbin.org";

const DESCRIPTION: &str = "Sample requests against httpbin.org, a public echo service. \
Select one and press Send (Ctrl+Enter, or Cmd+Enter on macOS). \
Delete this collection whenever you like; it is not recreated.";

/// Visible (non-hidden) entries of `dir`; a missing directory has none.
fn visible_entries(dir: &Path) -> Result<Vec<String>, StoreError> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io(dir)(e)),
    };
    let mut out = Vec::new();
    for entry in rd {
        let name = entry.map_err(io(dir))?.file_name();
        let name = name.to_string_lossy().into_owned();
        if !is_hidden(&name) {
            out.push(name);
        }
    }
    Ok(out)
}

/// The sample requests, in sidebar order (the sidebar sorts by name).
pub fn sample_requests() -> Vec<Request> {
    vec![
        Request {
            name: "1. GET with query params".into(),
            method: Method::Get,
            // A literal URL, so the very first Send works even with no environment selected.
            url: "https://httpbin.org/get".into(),
            params: vec![
                KeyValue::new("search", "curlpit"),
                KeyValue::new("page", "1"),
            ],
            ..Default::default()
        },
        Request {
            name: "2. POST JSON".into(),
            method: Method::Post,
            url: "https://httpbin.org/post".into(),
            body: Body::Json {
                raw: "{\n  \"name\": \"Curlpit\",\n  \"tags\": [\"native\", \"fast\"]\n}".into(),
            },
            ..Default::default()
        },
        Request {
            name: "3. Header and variable".into(),
            method: Method::Get,
            url: "{{baseUrl}}/headers".into(),
            headers: vec![KeyValue::new("X-Demo", "hello from Curlpit")],
            ..Default::default()
        },
        Request {
            name: "4. Test script".into(),
            method: Method::Get,
            url: "{{baseUrl}}/json".into(),
            test_script: [
                "pm.test(\"status is 200\", () => {",
                "  pm.response.to.have.status(200);",
                "});",
                "",
                "pm.test(\"body has a slideshow title\", () => {",
                "  const body = pm.response.json();",
                "  pm.expect(body.slideshow).to.have.property(\"title\");",
                "});",
                "",
            ]
            .join("\n"),
            ..Default::default()
        },
        Request {
            name: "5. Pre-request script".into(),
            method: Method::Get,
            url: "{{baseUrl}}/anything".into(),
            headers: vec![KeyValue::new("X-Request-Id", "{{requestId}}")],
            pre_request_script: [
                "// Runs before the request is sent. pm.variables lives for this send only.",
                "pm.variables.set(\"requestId\", \"demo-\" + Date.now());",
                "",
            ]
            .join("\n"),
            test_script: [
                "pm.test(\"the header was sent\", () => {",
                "  const sent = pm.response.json().headers[\"X-Request-Id\"];",
                "  pm.expect(sent).to.match(/^demo-/);",
                "});",
                "",
            ]
            .join("\n"),
            ..Default::default()
        },
    ]
}

impl Store {
    /// True when the data directory holds nothing but an empty `collections/` directory.
    fn is_fresh(&self) -> Result<bool, StoreError> {
        let top = visible_entries(&self.data_dir)?;
        if top.iter().any(|n| n != "collections") {
            return Ok(false);
        }
        Ok(visible_entries(&self.root)?.is_empty())
    }

    /// Seed the "Getting started" collection and the `httpbin` environment on a fresh install,
    /// and make that environment active. Returns `true` if it seeded, `false` if the data
    /// directory was not fresh (then nothing is written).
    ///
    /// Settings are load-modify-save (D44) and written last, so a seeded data directory is
    /// never fresh again, even after the user deletes the sample.
    pub fn seed_sample_if_fresh(&self) -> Result<bool, StoreError> {
        if !self.is_fresh()? {
            return Ok(false);
        }
        let col = self.create_collection(SAMPLE_COLLECTION)?;
        Store::save_collection_meta(
            &col.dir,
            &CollectionMeta {
                name: SAMPLE_COLLECTION.into(),
                description: DESCRIPTION.into(),
                ..Default::default()
            },
        )?;
        for req in sample_requests() {
            Store::save_request(&col.dir, &req, None)?;
        }
        self.save_environment(
            &Environment {
                name: SAMPLE_ENVIRONMENT.into(),
                variables: vec![EnvVar::new("baseUrl", SAMPLE_BASE_URL)],
            },
            None,
        )?;
        let mut settings = self.load_settings();
        settings.active_environment = Some(SAMPLE_ENVIRONMENT.into());
        self.save_settings(&settings)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, Settings};

    fn names(store: &Store) -> Vec<String> {
        store
            .load_all()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect()
    }

    #[test]
    fn seeds_once_on_a_fresh_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        assert!(store.seed_sample_if_fresh().unwrap());

        let cols = store.load_all().unwrap();
        assert_eq!(cols.len(), 1);
        let col = &cols[0];
        assert_eq!(col.name, SAMPLE_COLLECTION);
        let meta = Store::load_collection_meta(&col.dir).unwrap();
        assert!(meta.description.contains("httpbin.org"));

        // every request loads back, has a uid, and the sidebar order is the intended one
        let want = sample_requests();
        assert!((4..=6).contains(&want.len()));
        let got: Vec<Request> = col
            .children
            .iter()
            .map(|n| match n {
                Node::Request { path, .. } => Store::load_request(path).unwrap(),
                other => panic!("unexpected node {other:?}"),
            })
            .collect();
        assert_eq!(got.len(), want.len());
        for (g, w) in got.iter().zip(&want) {
            assert!(!g.uid.is_empty());
            assert_eq!(
                Request {
                    uid: String::new(),
                    ..g.clone()
                },
                *w
            );
        }
        // the shapes the guide promises
        assert!(want.iter().any(|r| !r.params.is_empty()));
        assert!(want.iter().any(|r| matches!(r.body, Body::Json { .. })));
        assert!(want
            .iter()
            .any(|r| !r.headers.is_empty() && r.url.contains("{{baseUrl}}")));
        assert!(want.iter().any(|r| !r.test_script.is_empty()));
        assert!(
            want.iter()
                .all(|r| r.url.starts_with("https://httpbin.org")
                    || r.url.starts_with("{{baseUrl}}/"))
        );

        let envs = store.list_environments();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].env.name, SAMPLE_ENVIRONMENT);
        assert_eq!(
            envs[0].env.variables,
            vec![EnvVar::new("baseUrl", SAMPLE_BASE_URL)]
        );
        assert_eq!(
            store.load_settings().active_environment.as_deref(),
            Some(SAMPLE_ENVIRONMENT)
        );

        // a second start does nothing
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert_eq!(names(&store), vec![SAMPLE_COLLECTION.to_string()]);
        assert_eq!(store.list_environments().len(), 1);
    }

    #[test]
    fn never_reseeds_after_the_user_deletes_everything() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        assert!(store.seed_sample_if_fresh().unwrap());
        for c in store.load_all().unwrap() {
            fs::remove_dir_all(&c.dir).unwrap();
        }
        for e in store.list_environments() {
            store.delete_environment(&e.path).unwrap();
        }
        // even reopening the store (a restart) doesn't bring it back
        let store = Store::open_at(tmp.path()).unwrap();
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert!(store.load_all().unwrap().is_empty());
        assert!(store.list_environments().is_empty());
    }

    #[test]
    fn never_touches_a_dir_that_has_data() {
        // user collections, no settings yet
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        store.create_collection("Mine").unwrap();
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert_eq!(names(&store), vec!["Mine".to_string()]);
        assert!(!tmp.path().join("settings.toml").exists());
        assert!(store.list_environments().is_empty());

        // an older install: settings and history exist, but every collection was deleted
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        let settings = Settings {
            default_timeout_ms: 1234,
            proxy: "http://p:1".into(),
            ..Default::default()
        };
        store.save_settings(&settings).unwrap();
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert!(store.load_all().unwrap().is_empty());
        assert_eq!(store.load_settings(), settings);

        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("history.db"), b"").unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert!(store.load_all().unwrap().is_empty());

        // only an environment
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        store
            .save_environment(
                &Environment {
                    name: "Prod".into(),
                    variables: vec![],
                },
                None,
            )
            .unwrap();
        assert!(!store.seed_sample_if_fresh().unwrap());
        assert!(store.load_all().unwrap().is_empty());
        assert!(!tmp.path().join("settings.toml").exists());
    }

    #[test]
    fn hidden_entries_do_not_count_as_data() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        fs::write(store.root().join(".DS_Store"), b"").unwrap();
        assert!(store.seed_sample_if_fresh().unwrap());
        assert_eq!(names(&store), vec![SAMPLE_COLLECTION.to_string()]);
    }

    #[test]
    fn seeding_keeps_other_settings_fields() {
        // Only `active_environment` changes; every other field keeps its default.
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open_at(tmp.path()).unwrap();
        assert!(store.seed_sample_if_fresh().unwrap());
        let s = store.load_settings();
        assert_eq!(
            s,
            Settings {
                active_environment: Some(SAMPLE_ENVIRONMENT.into()),
                ..Default::default()
            }
        );
    }
}
