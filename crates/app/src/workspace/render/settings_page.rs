//! Drawing the Settings screen (FR-SET-02, 03): the app-wide defaults as a settings form
//! — a label column with a hint under each label, a control column — under the shared
//! page header, whose action is Save.

use crate::theme::{Palette, S1, S2, S3, SMALL_TEXT};
use crate::ui::{icon_btn, seg_btn_ix, Status};
use crate::workspace::settings_page::{PickFor, TlsForm};
use crate::workspace::Workspace;
use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::Input;
use gpui_component::switch::Switch;
use gpui_component::{Icon, IconName, Sizable as _};

/// Width of the label column.
const LABEL_W: f32 = 280.0;

impl Workspace {
    pub(crate) fn render_settings_page(&self, cx: &Context<Self>) -> impl IntoElement {
        let p = Palette::of(cx);
        let data_dir = self
            .store
            .as_ref()
            .map(|s| s.data_dir().display().to_string())
            .unwrap_or_else(|| "(storage unavailable)".into());

        let row = |label: &'static str, hint: &'static str, control: AnyElement| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(S3))
                .child(
                    div()
                        .w(px(LABEL_W))
                        .flex_none()
                        .flex()
                        .flex_col()
                        .child(div().text_color(p.text()).child(label))
                        .child(
                            div()
                                .text_size(px(SMALL_TEXT))
                                .text_color(p.text_placeholder())
                                .child(hint),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(420.))
                        .flex()
                        .child(control),
                )
        };

        let save = Button::new("settings-save")
            .small()
            .primary()
            .label("Save")
            .on_click(cx.listener(|this, _ev, _window, cx| this.save_settings_page(cx)))
            .into_any_element();
        let header = self.page_header(
            "Settings",
            Some(save),
            |this, _window, cx| {
                this.main_view = crate::sidebar::MainView::Request;
                cx.notify();
            },
            cx,
        );

        let note = match &self.status {
            Status::Info(m) => Some((m.clone(), p.text_muted(), IconName::Info)),
            Status::Error(e) => Some((e.clone(), p.status_error(), IconName::CircleX)),
            _ => None,
        };

        let form = div()
            .id("settings-form")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(S3))
            .flex()
            .flex_col()
            .gap(px(S3 + S1))
            .child(row(
                "Default timeout",
                "Milliseconds, 0 for none. Applies to new tabs.",
                div()
                    .w(px(140.))
                    .child(Input::new(&self.set_timeout).small())
                    .into_any_element(),
            ))
            .child(row(
                "Verify TLS certificates",
                "Default for new tabs.",
                Switch::new("settings-tls")
                    .checked(self.set_verify_tls)
                    .on_click(cx.listener(|this, on: &bool, _window, cx| {
                        if *on != this.set_verify_tls {
                            this.toggle_setting_tls(cx);
                        }
                    }))
                    .into_any_element(),
            ))
            .child(row(
                "History limit",
                "Entries kept. Applies next launch.",
                div()
                    .w(px(140.))
                    .child(Input::new(&self.set_history_cap).small())
                    .into_any_element(),
            ))
            .child(row(
                "Outbound proxy",
                "Proxy URL; empty for a direct connection.",
                Input::new(&self.set_proxy)
                    .small()
                    .flex_1()
                    .into_any_element(),
            ))
            .children(self.render_tls_rows(&row, cx))
            .child(row(
                "Data directory",
                "Collections, history and settings.",
                div()
                    .font_family(crate::ui::mono())
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.text_muted())
                    .child(data_dir)
                    .into_any_element(),
            ))
            .child(
                div()
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.text_placeholder())
                    .child("This app sends no telemetry."),
            );

        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(header)
            // The save result sits above the (scrolling) form so it is seen right after Save,
            // however long the form is.
            .children(note.map(|(m, col, icon)| {
                div()
                    .flex_none()
                    .px(px(S3))
                    .pt(px(S2))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(S2))
                    .text_color(col)
                    .child(Icon::new(icon).small().flex_none())
                    .child(div().flex_1().min_w_0().child(m))
            }))
            .child(form)
    }

    /// CA bundle, minimum TLS version and client certificates (G-17). Nothing until the
    /// page has been opened, which builds the form.
    fn render_tls_rows<R>(&self, row: &R, cx: &Context<Self>) -> Vec<AnyElement>
    where
        R: Fn(&'static str, &'static str, AnyElement) -> gpui::Div,
    {
        let Some(form) = TlsForm::get(cx) else {
            return Vec::new();
        };
        let p = Palette::of(cx);
        let choose = |id: &'static str, ix: usize, target: PickFor| {
            Button::new((id, ix)).small().label("Choose…").on_click(
                cx.listener(move |this, _ev, window, cx| this.pick_tls_file(target, window, cx)),
            )
        };

        let ca = div()
            .flex_1()
            .flex()
            .flex_row()
            .gap(px(S2))
            .child(Input::new(&form.ca_bundle).small().flex_1())
            .child(choose("tls-ca-pick", 0, PickFor::CaBundle))
            .into_any_element();

        let versions: [(&str, &str); 3] = [("", "Default"), ("1.2", "1.2"), ("1.3", "1.3")];
        let mut min = div().flex().flex_row().gap(px(S1));
        for (ix, (value, label)) in versions.into_iter().enumerate() {
            min = min.child(
                seg_btn_ix("tls-min", ix, label, form.min_tls == value, cx).on_click(
                    cx.listener(move |this, _ev, _window, cx| this.set_min_tls(value, cx)),
                ),
            );
        }

        let mut certs = div().flex_1().flex().flex_col().gap(px(S2));
        for (ix, r) in form.certs.iter().enumerate() {
            certs = certs.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(S2))
                    .child(div().w(px(150.)).child(Input::new(&r.host).small()))
                    .child(Input::new(&r.path).small().flex_1())
                    .child(choose("tls-cert-pick", ix, PickFor::Cert(ix)))
                    .child(
                        icon_btn(("tls-cert-del", ix), IconName::Close, "Remove", cx)
                            .xsmall()
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.remove_cert_row(ix, cx)
                            })),
                    ),
            );
        }
        certs = certs.child(
            div().flex().child(
                Button::new("tls-cert-add")
                    .small()
                    .label("Add certificate")
                    .on_click(cx.listener(|this, _ev, window, cx| this.add_cert_row(window, cx))),
            ),
        );
        if form.certs.iter().any(|r| {
            let path = r.path.read(cx).value().to_ascii_lowercase();
            path.ends_with(".p12") || path.ends_with(".pfx")
        }) {
            certs = certs.child(
                div()
                    .text_size(px(SMALL_TEXT))
                    .text_color(p.warning_text())
                    .child("PKCS#12 (.p12/.pfx) files can't be used yet. Convert to PEM: openssl pkcs12 -in cert.p12 -out cert.pem -nodes"),
            );
        }

        vec![
            row(
                "CA certificates",
                "PEM bundle trusted in addition to the built-in roots. Empty for none.",
                ca,
            )
            .into_any_element(),
            row(
                "Minimum TLS version",
                "Connections below it fail. Default lets the library choose (1.2).",
                min.into_any_element(),
            )
            .into_any_element(),
            row(
                "Client certificates",
                "Sent to the exact host (no port). One PEM file with the certificate and an unencrypted key; PKCS#12 isn't supported.",
                certs.into_any_element(),
            )
            .into_any_element(),
        ]
    }
}
