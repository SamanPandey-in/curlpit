//! The one place colour is decided: Zed's default "One" theme, as data.
//!
//! Colour used to come from two sources that never agreed — `gpui_component`'s stock
//! theme through `cx.theme()`, and a scatter of `rgb(0x…)` literals in the render
//! modules — so the library's widgets and our own drawing drifted apart and no value had
//! been picked on purpose. Everything now derives from one [`Palette`]: [`apply`] fills
//! all ~103 of the library's `ThemeColor` fields from it at startup, and every colour we
//! paint ourselves comes from a named accessor below. Hex literals live in this file and
//! nowhere else, so a palette change is a one-file change.
//!
//! The values are Zed's, copied from `assets/themes/one/one.json` in zed-industries/zed,
//! because the brief is "make it look like Zed's default theme". Both appearances are
//! built in and follow the system (FR-SET-01): nothing is tuned for dark only.
//!
//! Accessors are named for the job, never the hue (`status_success`, not `green`), so a
//! later repaint cannot leave a function called `green` returning red.

use at_core::json_tree::Kind;
use gpui::{hsla, px, rgb, App, Hsla, Window};
use gpui_component::highlighter::HighlightTheme;
use gpui_component::{ActiveTheme as _, Theme, ThemeColor, ThemeMode};
use std::sync::Arc;

/// The 4 px spacing scale. Panels used to mix `p_1`, `p_2`, `gap_3` with no system, so
/// neighbouring rows sat at different insets; these are the only gaps we reach for.
pub(crate) const S1: f32 = 4.0;
/// One step: the standard gap between controls in a row.
pub(crate) const S2: f32 = 8.0;
/// Two steps: the standard inset of a panel from the window edge.
pub(crate) const S3: f32 = 12.0;

/// Every row of chrome (tab bar, status row, find bar) is this tall before padding, so
/// the stack of bars above the viewer lines up.
pub(crate) const ROW_PAD_Y: f32 = 6.0;

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// One appearance of the One theme. The fields are Zed's token names; the methods below
/// are the only way anything else reads them.
#[derive(Clone, Copy)]
pub(crate) struct Palette {
    dark: bool,
    /// Zed's `background`: the backdrop behind panes. Rarely seen, used here for muted
    /// fills (switches, skeletons) where a flat neutral is wanted.
    window_background: u32,
    editor_background: u32,
    /// Zed's `surface.background` / `panel.background` / `tab_bar.background` — they are
    /// the same value in One, and so is `elevated_surface` in dark.
    surface: u32,
    elevated_surface: u32,
    border: u32,
    border_variant: u32,
    border_focused: u32,
    element_background: u32,
    element_hover: u32,
    element_selected: u32,
    text: u32,
    text_muted: u32,
    text_placeholder: u32,
    text_accent: u32,
    tab_active_background: u32,
    tab_inactive_background: u32,
    scrollbar_thumb: u32,
    error: u32,
    warning: u32,
    success: u32,
    info: u32,
    syntax_string: u32,
    syntax_number: u32,
    syntax_boolean: u32,
    syntax_keyword: u32,
    syntax_property: u32,
    syntax_comment: u32,
    syntax_type: u32,
    syntax_function: u32,
    syntax_variable: u32,
}

/// Zed One Dark.
pub(crate) const ONE_DARK: Palette = Palette {
    dark: true,
    window_background: 0x3b414d,
    editor_background: 0x282c33,
    surface: 0x2f343e,
    elevated_surface: 0x2f343e,
    border: 0x464b57,
    border_variant: 0x363c46,
    border_focused: 0x47679e,
    element_background: 0x2e343e,
    element_hover: 0x363c46,
    element_selected: 0x454a56,
    text: 0xdce0e5,
    text_muted: 0xa9afbc,
    text_placeholder: 0x878a98,
    text_accent: 0x74ade8,
    tab_active_background: 0x282c33,
    tab_inactive_background: 0x2f343e,
    scrollbar_thumb: 0xc8ccd4,
    error: 0xd07277,
    warning: 0xdec184,
    success: 0xa1c181,
    info: 0x74ade8,
    syntax_string: 0xa1c181,
    syntax_number: 0xbf956a,
    syntax_boolean: 0xbf956a,
    syntax_keyword: 0xb477cf,
    syntax_property: 0xd07277,
    syntax_comment: 0x5d636f,
    syntax_type: 0x6eb4bf,
    syntax_function: 0x73ade9,
    syntax_variable: 0xacb2be,
};

/// Zed One Light. Three tokens One Light does not publish are derived rather than
/// invented: `border_variant` reuses the hover element fill (the next step in from the
/// border, exactly as dark One does), and `border_focused` and `elevated_surface` reuse
/// the accent and the surface, the roles they play in dark One.
pub(crate) const ONE_LIGHT: Palette = Palette {
    dark: false,
    window_background: 0xdcdcdd,
    editor_background: 0xfafafa,
    surface: 0xebebec,
    elevated_surface: 0xebebec,
    border: 0xc9c9ca,
    border_variant: 0xdfdfe0,
    border_focused: 0x5c78e2,
    element_background: 0xebebec,
    element_hover: 0xdfdfe0,
    element_selected: 0xcacaca,
    text: 0x242529,
    text_muted: 0x58585a,
    text_placeholder: 0x7e8086,
    text_accent: 0x5c78e2,
    tab_active_background: 0xfafafa,
    tab_inactive_background: 0xebebec,
    scrollbar_thumb: 0x383a41,
    error: 0xd36151,
    warning: 0xa48819,
    success: 0x669f59,
    info: 0x5c78e2,
    syntax_string: 0x649f57,
    syntax_number: 0xad6e25,
    syntax_boolean: 0xad6e25,
    syntax_keyword: 0xa449ab,
    syntax_property: 0xd3604f,
    syntax_comment: 0xa2a3a7,
    syntax_type: 0x3882b7,
    syntax_function: 0x5b79e3,
    syntax_variable: 0x242529,
};

impl Palette {
    /// The palette matching the mode the library is currently in.
    pub(crate) fn of(cx: &App) -> Palette {
        if cx.theme().mode.is_dark() {
            ONE_DARK
        } else {
            ONE_LIGHT
        }
    }

    pub(crate) fn for_mode(mode: ThemeMode) -> Palette {
        if mode.is_dark() {
            ONE_DARK
        } else {
            ONE_LIGHT
        }
    }

    pub(crate) fn is_dark(self) -> bool {
        self.dark
    }

    // ---- surfaces ----

    /// The pane: the response viewer, the request editor, the window behind them.
    pub(crate) fn editor_background(self) -> Hsla {
        c(self.editor_background)
    }

    /// Chrome around the pane: sidebar, tab bar, status row — Zed's panel colour.
    pub(crate) fn surface(self) -> Hsla {
        c(self.surface)
    }

    /// Popovers and menus, which sit above everything else.
    pub(crate) fn elevated_surface(self) -> Hsla {
        c(self.elevated_surface)
    }

    /// A flat neutral fill for controls with no state of their own (switch, skeleton).
    pub(crate) fn muted_surface(self) -> Hsla {
        c(self.window_background)
    }

    // ---- lines ----

    /// The line between two regions (sidebar and pane, chrome and viewer).
    pub(crate) fn border(self) -> Hsla {
        c(self.border)
    }

    /// The quieter line used *inside* a region, e.g. between rows of a list.
    pub(crate) fn border_variant(self) -> Hsla {
        c(self.border_variant)
    }

    /// The focus ring.
    pub(crate) fn border_focused(self) -> Hsla {
        c(self.border_focused)
    }

    // ---- interactive element fills ----

    pub(crate) fn element_background(self) -> Hsla {
        c(self.element_background)
    }

    pub(crate) fn element_hover(self) -> Hsla {
        c(self.element_hover)
    }

    /// The quiet surface change that marks "this one is selected" — Zed never fills a
    /// selected tab or segment with the accent colour.
    pub(crate) fn element_selected(self) -> Hsla {
        c(self.element_selected)
    }

    // ---- text ----

    pub(crate) fn text(self) -> Hsla {
        c(self.text)
    }

    /// Labels, counts, hints: present but not competing with the content.
    pub(crate) fn text_muted(self) -> Hsla {
        c(self.text_muted)
    }

    /// Text standing in for content that is not there yet ("Send a request to see the
    /// response"), quieter again than a label.
    pub(crate) fn text_placeholder(self) -> Hsla {
        c(self.text_placeholder)
    }

    /// The single accent: links, the caret, the focus ring, the primary button.
    pub(crate) fn text_accent(self) -> Hsla {
        c(self.text_accent)
    }

    /// Text drawn *on* a solid accent/status fill. The pane colour reads cleanly on every
    /// One fill in both appearances, and it keeps solid buttons theme-aware.
    pub(crate) fn on_solid(self) -> Hsla {
        c(self.editor_background)
    }

    // ---- tabs ----

    /// The open tab, which in Zed is simply the pane showing through.
    pub(crate) fn tab_active_background(self) -> Hsla {
        c(self.tab_active_background)
    }

    pub(crate) fn tab_inactive_background(self) -> Hsla {
        c(self.tab_inactive_background)
    }

    /// Zed draws the thumb at 30 % so content stays readable underneath.
    pub(crate) fn scrollbar_thumb(self) -> Hsla {
        c(self.scrollbar_thumb).alpha(0.3)
    }

    pub(crate) fn scrollbar_thumb_hover(self) -> Hsla {
        c(self.scrollbar_thumb).alpha(0.5)
    }

    // ---- semantic: request outcome ----

    /// A 2xx response.
    pub(crate) fn status_success(self) -> Hsla {
        c(self.success)
    }

    /// A 3xx response: a redirect is not a failure, so it takes the warning hue.
    pub(crate) fn status_redirect(self) -> Hsla {
        c(self.warning)
    }

    /// A 4xx/5xx response, a transport error, a failed test, a broken file.
    pub(crate) fn status_error(self) -> Hsla {
        c(self.error)
    }

    /// The colour of an HTTP status code (FR-RES-02).
    pub(crate) fn status_class(self, code: u16) -> Hsla {
        match code {
            200..=299 => self.status_success(),
            300..=399 => self.status_redirect(),
            _ => self.status_error(),
        }
    }

    /// Something worked but deserves a second look: an unresolved `{{variable}}`, TLS
    /// verification switched off, a secret about to be copied into a snippet.
    pub(crate) fn warning_text(self) -> Hsla {
        c(self.warning)
    }

    /// A passing test, a finished run.
    pub(crate) fn success_text(self) -> Hsla {
        c(self.success)
    }

    /// Purely informational, never alarming.
    pub(crate) fn info_text(self) -> Hsla {
        c(self.info)
    }

    // ---- semantic: the JSON tree (FR-RES-03) ----

    pub(crate) fn json_string(self) -> Hsla {
        c(self.syntax_string)
    }

    pub(crate) fn json_number(self) -> Hsla {
        c(self.syntax_number)
    }

    /// Zed paints a JSON `true`/`false` like a number. FR-RES-03 asks for one colour per
    /// type, so booleans take the keyword hue instead — the colour Zed gives `true` and
    /// `false` in the languages where they are keywords, so it still belongs to One.
    pub(crate) fn json_bool(self) -> Hsla {
        c(self.syntax_keyword)
    }

    /// `null`, and the summary line of an object or an array: structure, not a value.
    pub(crate) fn json_structural(self) -> Hsla {
        c(self.text_muted)
    }

    pub(crate) fn json_kind(self, kind: Kind) -> Hsla {
        match kind {
            Kind::String => self.json_string(),
            Kind::Number => self.json_number(),
            Kind::Bool => self.json_bool(),
            Kind::Null | Kind::Object | Kind::Array => self.json_structural(),
        }
    }

    /// An object key in the tree — the same `property` colour Zed highlights JSON keys
    /// with, so the tree and the body editor agree.
    pub(crate) fn json_key(self) -> Hsla {
        c(self.syntax_property)
    }

    // ---- semantic: Find (FR-RES-05) ----

    /// A match that is not the current one.
    pub(crate) fn find_match_bg(self) -> Hsla {
        c(self.warning)
    }

    /// The match the ↑/↓ buttons are on.
    pub(crate) fn find_current_bg(self) -> Hsla {
        c(self.text_accent)
    }

    /// Text inside a highlight. This used to be a fixed near-black, which vanished on the
    /// light theme's darker fills; the pane colour inverts with the appearance instead.
    pub(crate) fn find_match_fg(self) -> Hsla {
        self.on_solid()
    }

    /// The row the current match lives on, highlighted the way a selected row is.
    pub(crate) fn find_row_bg(self) -> Hsla {
        self.element_selected()
    }

    /// The response viewer's line numbers. Zed paints its gutter with the comment
    /// colour, which is why it recedes instead of competing with the body.
    pub(crate) fn gutter_text(self) -> Hsla {
        c(self.syntax_comment)
    }
}

/// Fill in every `ThemeColor` field from `p`, plus the handful of non-colour theme
/// settings that decide how Zed-like the chrome reads (flat, small radii, our mono font).
fn colors(p: Palette) -> ThemeColor {
    let accent = p.text_accent();
    let on_solid = p.on_solid();
    ThemeColor {
        // Hover/selection tint for menu and list items.
        accent: p.element_hover(),
        accent_foreground: p.text(),
        accordion: p.surface(),
        accordion_hover: p.element_hover(),
        background: p.editor_background(),
        border: p.border(),
        group_box: p.surface(),
        group_box_foreground: p.text(),
        caret: accent,
        // Five distinct One hues, in the order charts consume them.
        chart_1: p.info_text(),
        chart_2: p.success_text(),
        chart_3: p.warning_text(),
        chart_4: p.status_error(),
        chart_5: p.json_bool(),
        danger: p.status_error(),
        danger_active: lift(p.status_error(), p, -0.06),
        danger_foreground: on_solid,
        danger_hover: lift(p.status_error(), p, 0.06),
        description_list_label: p.surface(),
        description_list_label_foreground: p.text_muted(),
        drag_border: accent,
        drop_target: accent.alpha(0.2),
        foreground: p.text(),
        info: p.info_text(),
        info_active: lift(p.info_text(), p, -0.06),
        info_foreground: on_solid,
        info_hover: lift(p.info_text(), p, 0.06),
        input: p.border(),
        link: accent,
        link_active: accent,
        link_hover: accent,
        list: p.surface(),
        list_active: p.element_selected(),
        list_active_border: p.border_focused(),
        list_even: p.surface(),
        list_head: p.surface(),
        list_hover: p.element_hover(),
        muted: p.muted_surface(),
        muted_foreground: p.text_muted(),
        popover: p.elevated_surface(),
        popover_foreground: p.text(),
        primary: accent,
        primary_active: lift(accent, p, -0.06),
        primary_foreground: on_solid,
        primary_hover: lift(accent, p, 0.06),
        progress_bar: accent,
        ring: p.border_focused(),
        // Zed leaves the track empty and lets the thumb float over the content.
        scrollbar: hsla(0., 0., 0., 0.),
        scrollbar_thumb: p.scrollbar_thumb(),
        scrollbar_thumb_hover: p.scrollbar_thumb_hover(),
        secondary: p.element_background(),
        secondary_active: p.element_selected(),
        secondary_foreground: p.text(),
        secondary_hover: p.element_hover(),
        selection: accent.alpha(0.3),
        sidebar: p.surface(),
        sidebar_accent: p.element_hover(),
        sidebar_accent_foreground: p.text(),
        sidebar_border: p.border(),
        sidebar_foreground: p.text(),
        sidebar_primary: accent,
        sidebar_primary_foreground: on_solid,
        skeleton: p.muted_surface(),
        slider_bar: accent,
        slider_thumb: p.text(),
        success: p.success_text(),
        success_foreground: on_solid,
        success_hover: lift(p.success_text(), p, 0.06),
        success_active: lift(p.success_text(), p, -0.06),
        bullish: p.success_text(),
        bearish: p.status_error(),
        switch: p.muted_surface(),
        switch_thumb: p.text(),
        tab: p.tab_inactive_background(),
        tab_active: p.tab_active_background(),
        tab_active_foreground: p.text(),
        tab_bar: p.surface(),
        tab_bar_segmented: p.element_background(),
        tab_foreground: p.text_muted(),
        table: p.editor_background(),
        table_active: p.element_selected(),
        table_active_border: p.border_focused(),
        table_even: p.surface(),
        table_head: p.surface(),
        table_head_foreground: p.text_muted(),
        table_hover: p.element_hover(),
        table_row_border: p.border_variant(),
        title_bar: p.surface(),
        title_bar_border: p.border(),
        tiles: p.surface(),
        warning: p.warning_text(),
        warning_active: lift(p.warning_text(), p, -0.06),
        warning_hover: lift(p.warning_text(), p, 0.06),
        warning_foreground: on_solid,
        // A dialog scrim: the same black in both appearances, as Zed does.
        overlay: hsla(0., 0., 0., 0.5),
        window_border: p.border(),
        // The base hues components fall back to when a field above is not consulted.
        red: p.status_error(),
        red_light: lift(p.status_error(), p, 0.1),
        green: p.success_text(),
        green_light: lift(p.success_text(), p, 0.1),
        blue: p.info_text(),
        blue_light: lift(p.info_text(), p, 0.1),
        yellow: p.warning_text(),
        yellow_light: lift(p.warning_text(), p, 0.1),
        magenta: p.json_bool(),
        magenta_light: lift(p.json_bool(), p, 0.1),
        cyan: c(p.syntax_type),
        cyan_light: lift(c(p.syntax_type), p, 0.1),
    }
}

/// Nudge `base` by `amount` of lightness *away from* the appearance's background, so
/// "hover" always means "a little more present" in both light and dark.
fn lift(base: Hsla, p: Palette, amount: f32) -> Hsla {
    let delta = if p.is_dark() { amount } else { -amount };
    Hsla {
        l: (base.l + delta).clamp(0., 1.),
        ..base
    }
}

/// The syntax theme for the body and script editors, in Zed's own theme-file shape.
///
/// `gpui_component` can only build one of these by deserializing it — the styles inside
/// `SyntaxColors` have private fields — so the palette is rendered to JSON here rather
/// than kept as a second copy of the colours.
fn highlight_json(p: Palette) -> String {
    let hex = |v: u32| format!("#{v:06x}");
    format!(
        r##"{{
  "name": "One {mode}",
  "appearance": "{mode_key}",
  "style": {{
    "editor.background": "{editor_bg}",
    "editor.foreground": "{text}",
    "editor.active_line.background": "{active_line}",
    "editor.line_number": "{comment}",
    "editor.active_line_number": "{text}",
    "error": "{error}",
    "warning": "{warning}",
    "info": "{info}",
    "success": "{success}",
    "hint": "{type_}",
    "syntax": {{
      "attribute": {{ "color": "{function}" }},
      "boolean": {{ "color": "{boolean}" }},
      "comment": {{ "color": "{comment}", "font_style": "italic" }},
      "comment.doc": {{ "color": "{comment}", "font_style": "italic" }},
      "constant": {{ "color": "{type_}" }},
      "constructor": {{ "color": "{function}" }},
      "enum": {{ "color": "{property}" }},
      "function": {{ "color": "{function}" }},
      "hint": {{ "color": "{comment}" }},
      "keyword": {{ "color": "{keyword}" }},
      "label": {{ "color": "{function}" }},
      "link_text": {{ "color": "{string}" }},
      "link_uri": {{ "color": "{string}" }},
      "number": {{ "color": "{number}" }},
      "operator": {{ "color": "{variable}" }},
      "predictive": {{ "color": "{comment}" }},
      "preproc": {{ "color": "{text}" }},
      "primary": {{ "color": "{text}" }},
      "property": {{ "color": "{property}" }},
      "punctuation": {{ "color": "{variable}" }},
      "punctuation.bracket": {{ "color": "{variable}" }},
      "punctuation.delimiter": {{ "color": "{variable}" }},
      "punctuation.list_marker": {{ "color": "{property}" }},
      "punctuation.special": {{ "color": "{number}" }},
      "string": {{ "color": "{string}" }},
      "string.escape": {{ "color": "{comment}" }},
      "string.regex": {{ "color": "{number}" }},
      "string.special": {{ "color": "{number}" }},
      "string.special.symbol": {{ "color": "{string}" }},
      "tag": {{ "color": "{function}" }},
      "text.literal": {{ "color": "{string}" }},
      "title": {{ "color": "{property}", "font_weight": 700 }},
      "type": {{ "color": "{type_}" }},
      "variable": {{ "color": "{variable}" }},
      "variable.special": {{ "color": "{number}" }},
      "variant": {{ "color": "{function}" }}
    }}
  }}
}}"##,
        mode = if p.is_dark() { "Dark" } else { "Light" },
        mode_key = if p.is_dark() { "dark" } else { "light" },
        editor_bg = hex(p.editor_background),
        active_line = hex(p.surface),
        text = hex(p.text),
        error = hex(p.error),
        warning = hex(p.warning),
        info = hex(p.info),
        success = hex(p.success),
        string = hex(p.syntax_string),
        number = hex(p.syntax_number),
        boolean = hex(p.syntax_boolean),
        keyword = hex(p.syntax_keyword),
        property = hex(p.syntax_property),
        comment = hex(p.syntax_comment),
        type_ = hex(p.syntax_type),
        function = hex(p.syntax_function),
        variable = hex(p.syntax_variable),
    )
}

/// Parse [`highlight_json`]. The string is built from constants in this file, so a failure
/// would be a bug here rather than bad input; the library's own theme is a safe stand-in
/// until the test below catches it.
fn highlight_theme(p: Palette) -> Arc<HighlightTheme> {
    match serde_json::from_str::<HighlightTheme>(&highlight_json(p)) {
        Ok(t) => Arc::new(t),
        Err(_) if p.is_dark() => HighlightTheme::default_dark(),
        Err(_) => HighlightTheme::default_light(),
    }
}

/// Put One into `gpui_component`'s global theme, so the library's own widgets (buttons,
/// inputs, lists, scrollbars, popovers) are painted from the same palette we paint with.
///
/// This deliberately does not go through `Theme::change`, which would reload the
/// library's bundled theme over the top.
pub(crate) fn apply(mode: ThemeMode, cx: &mut App) {
    let p = Palette::for_mode(mode);
    let theme = Theme::global_mut(cx);
    theme.mode = mode;
    theme.colors = colors(p);
    theme.highlight_theme = highlight_theme(p);
    // Zed's chrome is flat with small radii; the library defaults to 6/8 px and shadows.
    theme.radius = px(4.);
    theme.radius_lg = px(6.);
    theme.shadow = false;
}

/// Follow the system appearance (FR-SET-01). Called once when the window opens and again
/// from `observe_window_appearance` whenever the user flips light/dark.
pub(crate) fn sync(window: &Window, cx: &mut App) {
    apply(ThemeMode::from(window.appearance()), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [Palette; 2] = [ONE_DARK, ONE_LIGHT];

    fn name(p: Palette) -> &'static str {
        if p.is_dark() {
            "One Dark"
        } else {
            "One Light"
        }
    }

    /// Two colours a user has to tell apart must not be the same colour.
    fn all_distinct(p: Palette, named: &[(&str, Hsla)]) {
        for (i, (an, a)) in named.iter().enumerate() {
            for (bn, b) in &named[i + 1..] {
                assert_ne!(
                    a,
                    b,
                    "{}: {an} and {bn} are the same colour, so the two states are \
                     indistinguishable",
                    name(p)
                );
            }
        }
    }

    #[test]
    fn every_json_kind_has_its_own_colour_in_both_modes() {
        for p in MODES {
            // Object/Array/Null share one "structural" colour on purpose: the marker and
            // the indentation already say which is which.
            assert_eq!(p.json_kind(Kind::Null), p.json_kind(Kind::Object));
            assert_eq!(p.json_kind(Kind::Null), p.json_kind(Kind::Array));
            all_distinct(
                p,
                &[
                    ("string", p.json_kind(Kind::String)),
                    ("number", p.json_kind(Kind::Number)),
                    ("bool", p.json_kind(Kind::Bool)),
                    ("structural", p.json_kind(Kind::Null)),
                    // the key sits next to the value on the same row
                    ("key", p.json_key()),
                ],
            );
        }
    }

    #[test]
    fn every_status_class_has_its_own_colour_in_both_modes() {
        for p in MODES {
            all_distinct(
                p,
                &[
                    ("2xx", p.status_class(204)),
                    ("3xx", p.status_class(301)),
                    ("4xx", p.status_class(404)),
                ],
            );
            // the whole class maps to one colour, not just the examples above
            assert_eq!(p.status_class(200), p.status_class(299));
            assert_eq!(p.status_class(300), p.status_class(399));
            assert_eq!(p.status_class(400), p.status_class(599));
            assert_eq!(p.status_class(500), p.status_error());
            // a 1xx is not a success
            assert_eq!(p.status_class(100), p.status_error());
        }
    }

    #[test]
    fn states_drawn_on_top_of_each_other_stay_apart() {
        for p in MODES {
            all_distinct(
                p,
                &[
                    ("pane", p.editor_background()),
                    ("chrome", p.surface()),
                    ("hover", p.element_hover()),
                    ("selected", p.element_selected()),
                    ("border", p.border()),
                ],
            );
            // the two Find highlights have to be told apart at a glance
            all_distinct(
                p,
                &[
                    ("find match", p.find_match_bg()),
                    ("find current", p.find_current_bg()),
                    ("find row", p.find_row_bg()),
                ],
            );
            // text on a highlight must not be the highlight
            assert_ne!(p.find_match_fg(), p.find_match_bg(), "{}", name(p));
            assert_ne!(p.find_match_fg(), p.find_current_bg(), "{}", name(p));
        }
    }

    #[test]
    fn text_is_readable_on_its_own_background() {
        for p in MODES {
            // dark text on a dark pane (or the reverse) would be the one mistake that
            // makes a theme unusable, so check the direction, not just inequality
            let light_text = p.text().l > p.editor_background().l;
            assert_eq!(
                light_text,
                p.is_dark(),
                "{}: text/background inverted",
                name(p)
            );
            assert_eq!(
                p.text_muted().l > p.editor_background().l,
                p.is_dark(),
                "{}: muted text/background inverted",
                name(p)
            );
            // text on a solid accent fill must contrast with that fill, not match it
            assert_eq!(
                p.on_solid().l < p.text_accent().l,
                p.is_dark(),
                "{}: button text does not contrast with its fill",
                name(p)
            );
        }
    }

    #[test]
    fn the_whole_palette_reaches_the_library_theme() {
        for p in MODES {
            let t = colors(p);
            assert_eq!(t.background, p.editor_background());
            assert_eq!(t.foreground, p.text());
            assert_eq!(t.border, p.border());
            assert_eq!(t.muted_foreground, p.text_muted());
            assert_eq!(t.primary, p.text_accent());
            assert_eq!(t.danger, p.status_error());
            assert_eq!(t.ring, p.border_focused());
            // the thumb floats over content, so it keeps Zed's 30 % alpha
            assert!(t.scrollbar_thumb.a < 1.0);
            // nothing may be left at `Hsla::default()`, which is transparent black
            let black = Hsla::default();
            for (n, v) in [
                ("popover", t.popover),
                ("list_hover", t.list_hover),
                ("tab_bar", t.tab_bar),
                ("caret", t.caret),
                ("warning", t.warning),
                ("success", t.success),
                ("drop_target", t.drop_target),
            ] {
                assert_ne!(v, black, "{}: {n} was left at the library default", name(p));
            }
        }
    }

    #[test]
    fn hover_and_active_move_in_opposite_directions_from_the_base() {
        for p in MODES {
            let base = p.text_accent();
            let t = colors(p);
            assert!(
                (t.primary_hover.l - base.l).signum() != (t.primary_active.l - base.l).signum(),
                "{}: hover and active land on the same side of the base colour",
                name(p)
            );
        }
    }

    #[test]
    fn the_syntax_theme_parses() {
        for p in MODES {
            let json = highlight_json(p);
            let parsed = serde_json::from_str::<HighlightTheme>(&json)
                .unwrap_or_else(|e| panic!("{}: {e}\n{json}", name(p)));
            assert_eq!(parsed.appearance.is_dark(), p.is_dark());
            assert_eq!(
                parsed.style.editor_background,
                Some(p.editor_background()),
                "{}: the editor must share the pane's background",
                name(p)
            );
            assert!(parsed.style.syntax.string.is_some());
            assert!(parsed.style.syntax.number.is_some());
        }
    }
}
