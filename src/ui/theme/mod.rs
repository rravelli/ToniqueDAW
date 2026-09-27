// The one place colours are defined from values (see `clippy.toml`).
#![allow(clippy::disallowed_methods)]

use std::sync::{Arc, LazyLock};

use egui::{
    Color32, Context, CornerRadius, FontId, Id, Margin, Shadow, Spacing, Stroke, Style, TextStyle,
    Ui, Vec2, Visuals, style::WidgetVisuals,
};

mod file;
mod library;

pub use file::format_color;
pub use library::ThemeLibrary;

/// Declares every colour of [`Theme`] once, with the group and description
/// shown in the theme editor. The field names are the keys of theme files.
macro_rules! theme_colors {
    ($($group:literal { $($field:ident: $label:literal, $description:literal,)* })*) => {
        /// Every colour the interface uses, named by role rather than by
        /// value so a theme can change them consistently. Read it with
        /// [`ThemeExt::app_theme`].
        ///
        /// Surfaces go from the deepest (the timeline canvas, wells behind
        /// meters and inputs) to the most raised (controls).
        #[derive(Clone, Debug, PartialEq)]
        pub struct Theme {
            pub name: String,
            $($(
                #[doc = $description]
                pub $field: Color32,
            )*)*
            /// Categorical colours: new tracks, graph node kinds.
            pub palette: Vec<Color32>,
        }

        /// Every colour of a [`Theme`], in display order.
        pub const COLORS: &[ColorRole] = &[
            $($(ColorRole {
                key: stringify!($field),
                group: $group,
                label: $label,
                description: $description,
            },)*)*
        ];

        impl Theme {
            /// All colours transparent: the start of the base theme, which
            /// sets them all.
            fn blank() -> Self {
                Self {
                    name: String::new(),
                    $($($field: Color32::TRANSPARENT,)*)*
                    palette: Vec::new(),
                }
            }

            /// The colour stored under `key` in theme files.
            pub fn color(&self, key: &str) -> Option<Color32> {
                match key {
                    $($(stringify!($field) => Some(self.$field),)*)*
                    _ => None,
                }
            }

            pub fn color_mut(&mut self, key: &str) -> Option<&mut Color32> {
                match key {
                    $($(stringify!($field) => Some(&mut self.$field),)*)*
                    _ => None,
                }
            }
        }
    };
}

theme_colors! {
    "Surfaces" {
        bg_deep: "Deep", "Timeline canvas, graph background, meter and input wells.",
        bg_base: "Base", "Behind the panels.",
        bg_panel: "Panel", "Panels, top bar, menu bar.",
        bg_raised: "Raised", "Windows, popups, track headers.",
        bg_control: "Control", "Buttons, chips, selects.",
        bg_control_hover: "Control hover", "Hovered buttons and selects.",
    }
    "Lines" {
        border: "Border", "Outlines of windows, inputs and cards.",
        separator: "Separator", "Dividers between sections and rows.",
    }
    "Text" {
        text: "Text", "Text and icons.",
        text_muted: "Muted", "Labels, hints, secondary values.",
        text_disabled: "Disabled", "Disabled controls, faint marks.",
        text_on_accent: "On accent", "Text drawn on the accent colour.",
    }
    "Accent" {
        accent: "Accent", "Active toggles, selection, focus.",
        accent_hover: "Accent hover", "Hovered accent buttons.",
    }
    "Status" {
        danger: "Danger", "Errors and destructive actions.",
        warning: "Warning", "Warnings, muted tracks.",
        success: "Success", "Success messages.",
        record: "Record", "Record button and armed tracks.",
    }
    "Overlays" {
        hover_overlay: "Hover", "Tint over hovered rows and items.",
        shadow: "Shadow", "Behind labels floating on content, and loading clips.",
    }
    "Timeline" {
        playhead: "Playhead", "Playhead line and handle.",
        edit_cursor: "Edit cursor", "Edit cursor.",
        selection_fill: "Selection fill", "Inside a rubber-band selection: a faint veil.",
        selection_stroke: "Selection outline", "Dashed outline of a rubber-band selection.",
        clip_selected: "Selected clip", "Outline and tint of selected clips: neutral, so it goes with any clip colour.",
        loop_region: "Loop", "Loop bounds.",
        grid_bar: "Grid bars", "Grid lines on bars.",
        grid_beat: "Grid beats", "Grid lines on beats (half as strong between beats).",
    }
    "Meters" {
        meter_low: "Normal", "Level meters, normal level.",
        meter_mid: "Peak", "Level meters, peaks.",
        meter_high: "Clipping", "Level meters, clipping.",
    }
}

/// A colour of the theme, as listed in theme files and the editor.
pub struct ColorRole {
    /// Field of [`Theme`] and key in theme files.
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// The built-in Dark theme: neutral greys with a slight cool tint, a
    /// teal accent, and even lightness steps between surfaces.
    pub fn dark() -> Self {
        static DARK: LazyLock<Theme> = LazyLock::new(library::base_theme);
        DARK.clone()
    }

    /// Text colour that reads best on `bg`, e.g. a track's own colour.
    pub fn text_on(&self, bg: Color32) -> Color32 {
        if contrast(self.text, bg) >= contrast(self.bg_deep, bg) {
            self.text
        } else {
            self.bg_deep
        }
    }

    /// Colours too close to what they're drawn on to read comfortably
    /// (below WCAG AA, 4.5:1), with the worst case of each.
    pub fn contrast_issues(&self) -> Vec<(&'static str, String)> {
        let surfaces = [
            ("Deep", self.bg_deep),
            ("Base", self.bg_base),
            ("Panel", self.bg_panel),
            ("Raised", self.bg_raised),
            ("Control", self.bg_control),
            ("Control hover", self.bg_control_hover),
        ];
        // Muted text isn't used on hovered controls.
        type Check<'a> = (&'static str, Color32, &'a [(&'static str, Color32)]);
        let checks: [Check; 7] = [
            ("text", self.text, &surfaces),
            ("text_muted", self.text_muted, &surfaces[..5]),
            (
                "text_on_accent",
                self.text_on_accent,
                &[("Accent", self.accent), ("Accent hover", self.accent_hover)],
            ),
            ("accent", self.accent, &[("Raised", self.bg_raised)]),
            ("danger", self.danger, &[("Raised", self.bg_raised)]),
            ("warning", self.warning, &[("Raised", self.bg_raised)]),
            ("success", self.success, &[("Raised", self.bg_raised)]),
        ];
        checks
            .into_iter()
            .filter_map(|(key, color, backgrounds)| {
                let (on, ratio) = backgrounds
                    .iter()
                    .map(|(name, bg)| (*name, contrast(color, *bg)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))?;
                (ratio < 4.5).then(|| (key, format!("{ratio:.1}:1 on {on}, needs 4.5:1")))
            })
            .collect()
    }

    pub fn is_dark(&self) -> bool {
        luminance(self.bg_base) < 0.2
    }

    /// Make this the theme of the app: [`ThemeExt::app_theme`] returns it and
    /// egui's own widgets (menus, checkboxes, scroll bars…) follow it.
    pub fn install(self, ctx: &Context) {
        let style = Arc::new(self.style());
        // Same style whatever the system prefers: the theme decides.
        ctx.set_style_of(egui::Theme::Dark, style.clone());
        ctx.set_style_of(egui::Theme::Light, style);
        ctx.set_theme(if self.is_dark() {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.data_mut(|d| d.insert_temp(theme_id(), Arc::new(self)));
    }

    fn style(&self) -> Style {
        let mut style = Style {
            visuals: self.visuals(),
            spacing: spacing(),
            ..Default::default()
        };
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(12.0));
        style
    }

    /// egui's visuals, mapped from the theme's roles.
    fn visuals(&self) -> Visuals {
        let mut visuals = if self.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.panel_fill = self.bg_panel;
        visuals.window_fill = self.bg_raised;
        visuals.faint_bg_color = self.bg_raised;
        visuals.extreme_bg_color = self.bg_deep;
        visuals.code_bg_color = self.bg_deep;
        visuals.text_edit_bg_color = Some(self.bg_deep);
        visuals.window_stroke = Stroke::new(1., self.border);
        visuals.window_corner_radius = CornerRadius::same(2);
        visuals.menu_corner_radius = CornerRadius::same(2);
        visuals.popup_shadow = Shadow::NONE;
        visuals.hyperlink_color = self.accent;
        visuals.warn_fg_color = self.warning;
        visuals.error_fg_color = self.danger;
        visuals.selection.bg_fill = self.accent;
        visuals.selection.stroke = Stroke::new(1., self.text_on_accent);

        let widget =
            |bg: Color32, stroke: Color32, fg: Color32, base: WidgetVisuals| WidgetVisuals {
                bg_fill: bg,
                weak_bg_fill: bg,
                bg_stroke: Stroke::new(1., stroke),
                fg_stroke: Stroke::new(1., fg),
                corner_radius: CornerRadius::same(2),
                ..base
            };
        let w = visuals.widgets.clone();
        visuals.widgets.noninteractive = widget(
            self.bg_panel,
            self.separator,
            self.text_muted,
            w.noninteractive,
        );
        visuals.widgets.inactive =
            widget(self.bg_control, Color32::TRANSPARENT, self.text, w.inactive);
        visuals.widgets.hovered = widget(self.bg_control_hover, self.border, self.text, w.hovered);
        // egui also draws "strong" text (window titles, `RichText::strong`)
        // in the pressed colour, so it must read on any surface: no accent
        // fill here.
        visuals.widgets.active = widget(self.bg_control_hover, self.accent, self.text, w.active);
        visuals.widgets.open = widget(self.bg_control_hover, self.border, self.text, w.open);
        visuals
    }
}

fn spacing() -> Spacing {
    Spacing {
        item_spacing: Vec2::ZERO,
        window_margin: Margin::ZERO,
        menu_margin: Margin::same(4),
        ..Default::default()
    }
}

fn theme_id() -> Id {
    Id::new("tonique-theme")
}

/// Access to the installed [`Theme`].
pub trait ThemeExt {
    fn app_theme(&self) -> Arc<Theme>;
}

impl ThemeExt for Context {
    fn app_theme(&self) -> Arc<Theme> {
        self.data(|d| d.get_temp::<Arc<Theme>>(theme_id()))
            .unwrap_or_default()
    }
}

impl ThemeExt for Ui {
    fn app_theme(&self) -> Arc<Theme> {
        self.ctx().app_theme()
    }
}

/// `color` with its opacity replaced by `alpha`.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn luminance(color: Color32) -> f32 {
    let channel = |c: u8| {
        let c = c as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG contrast ratio, 1 (none) to 21 (black on white). Text needs 4.5.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every built-in theme: the rules below hold for all of them.
    fn presets() -> Vec<Theme> {
        let library = ThemeLibrary::load();
        library
            .list()
            .iter()
            .filter(|t| t.preset)
            .map(|t| library.resolve(&t.id).0)
            .collect()
    }

    fn surfaces(theme: &Theme) -> [(&str, Color32); 6] {
        [
            ("bg_deep", theme.bg_deep),
            ("bg_base", theme.bg_base),
            ("bg_panel", theme.bg_panel),
            ("bg_raised", theme.bg_raised),
            ("bg_control", theme.bg_control),
            ("bg_control_hover", theme.bg_control_hover),
        ]
    }

    #[test]
    fn surfaces_step_up_evenly() {
        for theme in presets() {
            let surfaces = surfaces(&theme);
            // Resting surfaces get lighter in even, visible steps.
            for pair in surfaces[..5].windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let step = contrast(a.1, b.1);
                assert!(
                    luminance(a.1) < luminance(b.1) && (1.07..1.4).contains(&step),
                    "{}: {} → {}: {step:.2}",
                    theme.name,
                    a.0,
                    b.0
                );
            }
            let hover = contrast(theme.bg_control, theme.bg_control_hover);
            assert!(hover >= 1.1, "{}: hover {hover:.2}", theme.name);
        }
    }

    #[test]
    fn text_is_readable_on_every_surface() {
        for theme in presets() {
            let name = &theme.name;
            for (surface, bg) in surfaces(&theme) {
                assert!(contrast(theme.text, bg) >= 7., "{name}: text on {surface}");
            }
            // Hovered controls are transient, so muted text only needs to
            // be readable on the resting surfaces.
            for (surface, bg) in &surfaces(&theme)[..5] {
                let ratio = contrast(theme.text_muted, *bg);
                assert!(ratio >= 4.5, "{name}: muted on {surface}");
            }
            assert!(
                contrast(theme.text_on_accent, theme.accent) >= 4.5,
                "{name}"
            );
            assert!(
                contrast(theme.text_on_accent, theme.accent_hover) >= 4.5,
                "{name}"
            );
            for (role, color) in [
                ("accent", theme.accent),
                ("danger", theme.danger),
                ("warning", theme.warning),
                ("success", theme.success),
            ] {
                assert!(contrast(color, theme.bg_raised) >= 4.5, "{name}: {role}");
            }
        }
    }

    #[test]
    fn presets_have_no_contrast_issues() {
        for theme in presets() {
            assert!(theme.contrast_issues().is_empty(), "{}", theme.name);
        }
        let mut theme = Theme::dark();
        theme.text_muted = theme.bg_panel;
        let issues = theme.contrast_issues();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].0, "text_muted");
        assert!(issues[0].1.contains("on Panel"), "{}", issues[0].1);
    }

    #[test]
    fn strong_text_is_readable() {
        for theme in presets() {
            let visuals = theme.visuals();
            let strong = visuals.strong_text_color();
            for bg in [theme.bg_panel, theme.bg_raised] {
                assert!(contrast(strong, bg) >= 7., "{}", theme.name);
            }
        }
    }

    #[test]
    fn contrast_matches_wcag() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.).abs() < 0.01);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.).abs() < 0.01);
        assert!(Theme::dark().is_dark());
        assert_eq!(Theme::dark().accent, Color32::from_rgb(0, 200, 150));
    }

    #[test]
    fn text_on_picks_the_readable_color() {
        for theme in presets() {
            for color in &theme.palette {
                let ink = theme.text_on(*color);
                assert!(contrast(ink, *color) >= 4.5, "{}: on {color:?}", theme.name);
            }
        }
    }

    #[test]
    fn installed_theme_is_shared() {
        let ctx = Context::default();
        let mut theme = Theme::dark();
        theme.name = "Custom".into();
        theme.accent = Color32::RED;
        theme.install(&ctx);
        assert_eq!(ctx.app_theme().name, "Custom");
        assert_eq!(ctx.global_style().visuals.selection.bg_fill, Color32::RED);
    }
}
