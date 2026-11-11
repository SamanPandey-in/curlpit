//! The bridge between a request and the `at_script` sandbox: what a script is shown
//! (`script_input`), what is taken back out of it (`take_script_*`), and how its variable
//! changes are merged into stored variables as a diff so concurrent tabs don't clobber
//! each other. Also `ScriptReport`, the Tests view's record of the last send.

use at_core::{Body, EnvVar, KeyValue, Request, Variables};
use at_http::ResponseBody;
use at_script::{ScriptInput, ScriptOutput, ScriptRequest, ScriptResponse, VarMap};

/// What the scripts of the last send did, for the Tests view.
#[derive(Clone, Default)]
pub(crate) struct ScriptReport {
    pub(crate) pre: Option<ScriptOutput>,
    pub(crate) test: Option<ScriptOutput>,
    /// e.g. "pm.environment changes were not kept: no environment is active"
    pub(crate) notes: Vec<String>,
}

impl ScriptReport {
    pub(crate) fn label(&self) -> String {
        let errors = [&self.pre, &self.test]
            .iter()
            .any(|o| o.as_ref().is_some_and(|o| o.error.is_some()));
        match &self.test {
            Some(t) if !t.tests.is_empty() => {
                let counted = t.tests.iter().filter(|x| !x.skipped).count();
                format!("Tests ({}/{counted})", t.passed())
            }
            _ if errors => "Tests (!)".into(),
            _ => "Tests".into(),
        }
    }
}

/// Response body text handed to test scripts (big bodies are cut; spilled ones aren't read).
pub(crate) const SCRIPT_BODY_MAX: usize = 10 * 1024 * 1024;

/// The request as a script sees it: as typed (variables unresolved), enabled headers only.
pub(crate) fn script_request(req: &Request) -> ScriptRequest {
    ScriptRequest {
        method: req.method.as_str().to_string(),
        url: req.url.clone(),
        headers: req
            .headers
            .iter()
            .filter(|h| h.enabled && !h.key.is_empty())
            .map(|h| (h.key.clone(), h.value.clone()))
            .collect(),
        body: match &req.body {
            Body::Json { raw } | Body::Xml { raw } | Body::Html { raw } | Body::Text { raw } => {
                raw.clone()
            }
            _ => String::new(),
        },
    }
}

pub(crate) fn to_var_map(m: &std::collections::HashMap<String, String>) -> VarMap {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

pub(crate) fn script_input(
    req: &Request,
    vars: &Variables,
    env_name: Option<String>,
    response: Option<ScriptResponse>,
) -> ScriptInput {
    ScriptInput {
        request_name: req.name.clone(),
        request: script_request(req),
        response,
        environment_name: env_name,
        environment: to_var_map(&vars.environment),
        globals: to_var_map(&vars.global),
        variables: to_var_map(&vars.runtime),
    }
}

/// Take a script's variable changes into the variables used for this send.
pub(crate) fn take_script_vars(vars: &mut Variables, o: &ScriptOutput) {
    vars.environment = o.environment.clone().into_iter().collect();
    vars.global = o.globals.clone().into_iter().collect();
    vars.runtime = o.variables.clone().into_iter().collect();
}

/// Take a pre-request script's changes to headers, URL and raw body into the request.
pub(crate) fn take_script_request(
    req: &mut Request,
    before: &ScriptRequest,
    after: &ScriptRequest,
) {
    if after.headers != before.headers {
        req.headers = after
            .headers
            .iter()
            .map(|(k, v)| KeyValue::new(k, v))
            .collect();
    }
    if after.url != before.url {
        req.url = after.url.clone();
    }
    if after.body != before.body {
        req.body = match &req.body {
            Body::Xml { .. } => Body::Xml {
                raw: after.body.clone(),
            },
            Body::Html { .. } => Body::Html {
                raw: after.body.clone(),
            },
            Body::Text { .. } => Body::Text {
                raw: after.body.clone(),
            },
            // a script can give a body-less request a JSON body; form bodies stay as they are
            Body::UrlEncoded { .. } | Body::FormData { .. } => req.body.clone(),
            _ => Body::Json {
                raw: after.body.clone(),
            },
        };
    }
}

pub(crate) fn script_response(resp: &at_http::HttpResponse) -> ScriptResponse {
    ScriptResponse {
        code: resp.status,
        status: resp.status_text.clone(),
        headers: resp.headers.clone(),
        body: match &resp.body {
            ResponseBody::Memory(b) => {
                String::from_utf8_lossy(&b[..b.len().min(SCRIPT_BODY_MAX)]).into_owned()
            }
            ResponseBody::File { .. } => String::new(),
        },
        time_ms: resp.elapsed.as_millis() as u64,
        size: resp.size,
    }
}

/// Apply what scripts changed (`before` -> `after`, enabled variables only) to a stored list.
/// Works as a diff, so two tabs whose scripts ran at the same time don't undo each other, and
/// secret flags and disabled variables are kept. `None` when nothing changed.
pub(crate) fn merge_script_vars(
    existing: &[EnvVar],
    before: &VarMap,
    after: &VarMap,
) -> Option<Vec<EnvVar>> {
    if before == after {
        return None;
    }
    let mut out = existing.to_vec();
    for k in before.keys().filter(|k| !after.contains_key(*k)) {
        out.retain(|v| !(v.enabled && &v.key == k));
    }
    for (k, v) in after {
        if before.get(k) == Some(v) {
            continue;
        }
        match out.iter_mut().find(|e| e.enabled && &e.key == k) {
            Some(e) => e.value = v.clone(),
            None => out.push(EnvVar::new(k, v)),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use at_core::{Body, EnvVar, KeyValue, Request};
    use at_script::VarMap;

    #[test]
    fn script_variable_changes_merge_as_a_diff() {
        let map = |pairs: &[(&str, &str)]| -> VarMap {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let mut secret = EnvVar::new("token", "old");
        secret.secret = true;
        let mut off = EnvVar::new("off", "x");
        off.enabled = false;
        let stored = vec![
            EnvVar::new("host", "h"),
            secret,
            off,
            EnvVar::new("gone", "1"),
        ];
        let before = map(&[("host", "h"), ("token", "old"), ("gone", "1")]);
        let after = map(&[("host", "h"), ("token", "new"), ("added", "2")]);
        let merged = merge_script_vars(&stored, &before, &after).unwrap();
        let keys: Vec<_> = merged.iter().map(|v| v.key.as_str()).collect();
        assert_eq!(keys, vec!["host", "token", "off", "added"]);
        assert!(merged[1].secret, "secret flag kept");
        assert_eq!(merged[1].value, "new");
        assert!(!merged[2].enabled, "disabled variable untouched");
        assert_eq!(merge_script_vars(&stored, &before, &before), None);

        // a change made elsewhere meanwhile survives (diff, not overwrite)
        let mut elsewhere = stored.clone();
        elsewhere[0].value = "changed-by-other-tab".into();
        let merged = merge_script_vars(&elsewhere, &before, &after).unwrap();
        assert_eq!(merged[0].value, "changed-by-other-tab");
    }

    #[test]
    fn pre_request_changes_flow_into_the_request() {
        let mut req = Request {
            url: "https://a.test".into(),
            headers: vec![KeyValue::new("A", "1"), {
                let mut off = KeyValue::new("Off", "x");
                off.enabled = false;
                off
            }],
            body: Body::Xml { raw: "<a/>".into() },
            ..Default::default()
        };
        let before = script_request(&req);
        assert_eq!(before.headers, vec![("A".to_string(), "1".to_string())]);
        let mut after = before.clone();
        after.headers.push(("B".into(), "2".into()));
        after.body = "<b/>".into();
        take_script_request(&mut req, &before, &after);
        assert_eq!(req.headers.len(), 2);
        assert_eq!(req.body, Body::Xml { raw: "<b/>".into() }, "raw kind kept");
        assert_eq!(req.url, "https://a.test", "unchanged parts stay");
    }
}
