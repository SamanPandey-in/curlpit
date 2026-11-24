//! `ScriptReport`, the Tests view's record of what the scripts of the last send did. The
//! script glue itself (inputs, taking changes back, merging variables) lives in `at_runner`.

use at_script::ScriptOutput;

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
