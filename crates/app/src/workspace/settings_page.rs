//! The app Settings screen (FR-SET-02, 03): load the stored defaults into the page, edit
//! them, and save. Per-request settings still live on each request tab; this is the app
//! default that seeds new requests, plus the history cap, outbound proxy and TLS options
//! (CA bundle, client certificates, minimum version; G-17).
use gpui::prelude::*;
use gpui::{App, Context, Entity, Global, PathPromptOptions, Window};
use gpui_component::input::InputState;
use std::path::PathBuf;

use crate::sidebar::MainView;
use crate::ui::Status;
use crate::workspace::Workspace;
use at_http::{ClientCert, TlsConfig, TlsVersion};
use at_store::{ClientCertSetting, Settings};

/// The TLS options as the HTTP engine wants them. Blank paths and hosts are left out.
pub(crate) fn tls_config(s: &Settings) -> TlsConfig {
    let ca = s.ca_bundle.trim();
    TlsConfig {
        ca_bundle: (!ca.is_empty()).then(|| PathBuf::from(ca)),
        client_certs: s
            .client_certs
            .iter()
            .filter(|c| !c.host.trim().is_empty() && !c.path.trim().is_empty())
            .map(|c| ClientCert {
                host: c.host.trim().to_string(),
                path: PathBuf::from(c.path.trim()),
            })
            .collect(),
        min_tls: TlsVersion::parse(&s.min_tls),
    }
}

/// Make the Settings' TLS options the engine default for the next request. Called before
/// every send and run, so it also covers the first one after startup.
pub(crate) fn apply_tls(s: &Settings) {
    at_http::set_default_tls(tls_config(s));
}

/// One client-certificate row on the Settings page.
pub(crate) struct CertRow {
    pub(crate) host: Entity<InputState>,
    pub(crate) path: Entity<InputState>,
    /// Not editable here (PKCS#12 is not supported); carried so saving never drops it.
    pub(crate) password: Option<String>,
}

/// The TLS part of the Settings form. A gpui global rather than `Workspace` fields, so the
/// form needs no change to the workspace struct; it is rebuilt each time the page opens.
pub(crate) struct TlsForm {
    pub(crate) ca_bundle: Entity<InputState>,
    /// `""`, `"1.2"` or `"1.3"`.
    pub(crate) min_tls: String,
    pub(crate) certs: Vec<CertRow>,
}

impl Global for TlsForm {}

impl TlsForm {
    pub(crate) fn get(cx: &App) -> Option<&TlsForm> {
        cx.try_global::<TlsForm>()
    }
}

fn input(
    value: &str,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.to_string();
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value)
    })
}

fn cert_row(c: &ClientCertSetting, window: &mut Window, cx: &mut App) -> CertRow {
    CertRow {
        host: input(&c.host, "api.example.com", window, cx),
        path: input(&c.path, "/path/to/client.pem", window, cx),
        password: c.password.clone(),
    }
}

/// Where a file chosen with "Choose…" goes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PickFor {
    CaBundle,
    Cert(usize),
}

impl Workspace {
    /// Open the Settings page, loading the current values into its fields.
    pub(crate) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.settings.clone();
        self.set_timeout.update(cx, |st, cx| {
            st.set_value(s.default_timeout_ms.to_string(), window, cx)
        });
        self.set_history_cap.update(cx, |st, cx| {
            st.set_value(s.history_cap.to_string(), window, cx)
        });
        self.set_proxy
            .update(cx, |st, cx| st.set_value(s.proxy.clone(), window, cx));
        self.set_verify_tls = s.verify_tls;
        let form = TlsForm {
            ca_bundle: input(&s.ca_bundle, "/path/to/ca-bundle.pem", window, cx),
            min_tls: match TlsVersion::parse(&s.min_tls) {
                Some(TlsVersion::Tls12) => "1.2".into(),
                Some(TlsVersion::Tls13) => "1.3".into(),
                None => String::new(),
            },
            certs: s
                .client_certs
                .iter()
                .map(|c| cert_row(c, window, cx))
                .collect(),
        };
        cx.set_global(form);
        self.status = Status::Idle;
        self.main_view = MainView::Settings;
        cx.notify();
    }

    pub(crate) fn toggle_setting_tls(&mut self, cx: &mut Context<Self>) {
        self.set_verify_tls = !self.set_verify_tls;
        cx.notify();
    }

    pub(crate) fn set_min_tls(&mut self, v: &str, cx: &mut Context<Self>) {
        if cx.has_global::<TlsForm>() {
            cx.global_mut::<TlsForm>().min_tls = v.to_string();
        }
        cx.notify();
    }

    pub(crate) fn add_cert_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !cx.has_global::<TlsForm>() {
            return;
        }
        let row = cert_row(&ClientCertSetting::default(), window, cx);
        cx.global_mut::<TlsForm>().certs.push(row);
        cx.notify();
    }

    pub(crate) fn remove_cert_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        if cx.has_global::<TlsForm>() {
            let certs = &mut cx.global_mut::<TlsForm>().certs;
            if ix < certs.len() {
                certs.remove(ix);
            }
        }
        cx.notify();
    }

    /// "Choose…": pick a PEM file for the CA bundle or a certificate row.
    pub(crate) fn pick_tls_file(
        &mut self,
        target: PickFor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |_this, window, cx| {
                let Some(form) = TlsForm::get(cx) else {
                    return;
                };
                let field = match target {
                    PickFor::CaBundle => Some(form.ca_bundle.clone()),
                    PickFor::Cert(ix) => form.certs.get(ix).map(|r| r.path.clone()),
                };
                if let Some(field) = field {
                    let text = path.display().to_string();
                    field.update(cx, |st, cx| st.set_value(text, window, cx));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Validate the fields, persist, and apply what can change live.
    pub(crate) fn save_settings_page(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            self.notice(Status::Error(
                "Storage is unavailable on this system".into(),
            ));
            cx.notify();
            return;
        };
        let timeout_txt = self.set_timeout.read(cx).value().trim().to_string();
        let cap_txt = self.set_history_cap.read(cx).value().trim().to_string();
        let proxy = self.set_proxy.read(cx).value().trim().to_string();

        let Ok(default_timeout_ms) = timeout_txt.parse::<u64>() else {
            self.notice(Status::Error(
                "Default timeout must be a whole number of milliseconds (0 for none)".into(),
            ));
            cx.notify();
            return;
        };
        let history_cap = match cap_txt.parse::<usize>() {
            Ok(n) if n >= 1 => n,
            _ => {
                self.notice(Status::Error(
                    "History cap must be a whole number, 1 or more".into(),
                ));
                cx.notify();
                return;
            }
        };

        // TLS: the form exists once the page has been opened; without it, keep what's stored.
        let tls = TlsForm::get(cx).map(|f| {
            let ca = f.ca_bundle.read(cx).value().trim().to_string();
            let certs: Vec<ClientCertSetting> = f
                .certs
                .iter()
                .map(|r| ClientCertSetting {
                    host: r.host.read(cx).value().trim().to_string(),
                    path: r.path.read(cx).value().trim().to_string(),
                    password: r.password.clone(),
                })
                .filter(|c| !c.host.is_empty() || !c.path.is_empty())
                .collect();
            (ca, f.min_tls.clone(), certs)
        });
        if let Some((_, _, certs)) = &tls {
            if let Some(c) = certs
                .iter()
                .find(|c| c.host.is_empty() || c.path.is_empty())
            {
                let which = if c.host.is_empty() {
                    format!("the certificate {} needs a host", c.path)
                } else {
                    format!("the certificate for {} needs a file", c.host)
                };
                self.notice(Status::Error(format!("Client certificates: {which}")));
                cx.notify();
                return;
            }
            let bad_host = |h: &str| {
                h.contains(['/', '*', ' '])
                    || (h.contains(':') && h.parse::<std::net::Ipv6Addr>().is_err())
            };
            if let Some(c) = certs.iter().find(|c| bad_host(&c.host)) {
                self.notice(Status::Error(format!(
                    "Client certificates: \"{}\" must be a plain host name (no scheme, port, path or wildcard)",
                    c.host
                )));
                cx.notify();
                return;
            }
        }

        // Load fresh so the active environment (saved separately) is preserved.
        let mut s = store.load_settings();
        s.default_timeout_ms = default_timeout_ms;
        s.verify_tls = self.set_verify_tls;
        s.history_cap = history_cap;
        s.proxy = proxy;
        if let Some((ca, min_tls, certs)) = tls {
            s.ca_bundle = ca;
            s.min_tls = min_tls;
            s.client_certs = certs;
        }
        if let Err(e) = store.save_settings(&s) {
            self.notice(Status::Error(e.to_string()));
            cx.notify();
            return;
        }
        // Files that aren't there are saved anyway (a drive may be offline), but said so now
        // rather than at the next Send.
        let missing: Vec<String> = std::iter::once(s.ca_bundle.as_str())
            .chain(s.client_certs.iter().map(|c| c.path.as_str()))
            .filter(|p| !p.is_empty() && !std::path::Path::new(p).is_file())
            .map(str::to_string)
            .collect();
        apply_tls(&s);
        self.settings = s;
        if missing.is_empty() {
            self.notice(Status::Info(
                "Settings saved. Proxy and TLS certificates apply to the next request; the default timeout and TLS verification apply to new tabs; the history limit applies next launch.".into(),
            ));
        } else {
            self.notice(Status::Error(format!(
                "Settings saved, but these files were not found: {}",
                missing.join(", ")
            )));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_skips_blanks_and_parses_the_version() {
        let s = Settings {
            ca_bundle: "  /certs/ca.pem ".into(),
            min_tls: "1.3".into(),
            client_certs: vec![
                ClientCertSetting {
                    host: " api.test ".into(),
                    path: "/c.pem".into(),
                    password: None,
                },
                ClientCertSetting {
                    host: "half.test".into(),
                    path: " ".into(),
                    password: None,
                },
            ],
            ..Default::default()
        };
        let t = tls_config(&s);
        assert_eq!(t.ca_bundle, Some(PathBuf::from("/certs/ca.pem")));
        assert_eq!(t.min_tls, Some(TlsVersion::Tls13));
        assert_eq!(t.client_certs.len(), 1);
        assert_eq!(t.client_certs[0].host, "api.test");
        assert_eq!(tls_config(&Settings::default()), TlsConfig::default());
    }
}
