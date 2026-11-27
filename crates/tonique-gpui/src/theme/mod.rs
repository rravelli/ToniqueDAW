//! The interface's colours, by role, as the egui app's theme: built-in
//! presets and the user's theme files (see [`ThemeLibrary`]); and the
//! colour maths Tonique does on them, in the same sRGB space, on the
//! project's own colour type.

mod file;
mod library;

use std::{rc::Rc, sync::LazyLock};

use gpui::{App, Font, Global, Hsla, Rgba, font};
pub use library::ThemeLibrary;
pub use tonique_core::utils::color::Color;

pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono NL";

/// The theme in use, app-wide: views read it with [`current`].
struct ActiveTheme(Rc<Theme>);

impl Global for ActiveTheme {}

/// The theme in use. Cheap to keep: views take it at the start of a render
/// and lend it out while they use the context.
pub fn current(app: &App) -> Rc<Theme> {
    app.global::<ActiveTheme>().0.clone()
}

/// Uses `theme` everywhere from now on; the windows draw again with it.
pub fn install(theme: Theme, app: &mut App) {
    app.set_global(ActiveTheme(Rc::new(theme)));
    app.refresh_windows();
}

/// Font sizes, as `ui::theme::font_size`.
pub mod size {
    pub const SMALL: f32 = 10.;
    pub const LABEL: f32 = 11.;
    pub const BODY: f32 = 12.;
    pub const HEADING: f32 = 14.;
}

/// Loads the fonts the egui app bundles, so text is compared like for like.
pub fn load_fonts(context: &mut gpui::App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../../../fonts/Inter-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../../fonts/Inter-SemiBold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../../fonts/JetBrainsMonoNL-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../assets/fonts/Phosphor.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../assets/fonts/Phosphor-Fill.ttf")
            .as_slice()
            .into(),
    ];
    if let Err(error) = context.text_system().add_fonts(fonts) {
        eprintln!("Couldn't load the fonts: {error}");
    }
}

/// Phosphor's font families, as named in its files.
pub const ICONS: &str = "Phosphor";
pub const ICONS_FILL: &str = "Phosphor-Fill";

fn icon_in(family: &'static str, glyph: &'static str) -> gpui::Div {
    use gpui::{ParentElement, Styled};
    gpui::div().flex_none().font_family(family).child(glyph)
}

/// A Phosphor icon (one of [`crate::icons`]) in the regular style, sized
/// and coloured like text.
pub fn icon(glyph: &'static str) -> gpui::Div {
    icon_in(ICONS, glyph)
}

/// The same in the fill style.
pub fn icon_fill(glyph: &'static str) -> gpui::Div {
    icon_in(ICONS_FILL, glyph)
}

/// The font to paint an icon with on a canvas.
pub fn icon_font(fill: bool) -> Font {
    font(if fill { ICONS_FILL } else { ICONS })
}

pub fn mono() -> Font {
    font(FONT_MONO)
}

pub const BLACK: Color = Color::rgb(0, 0, 0);
pub const WHITE: Color = Color::rgb(255, 255, 255);

/// Declares every colour of [`Theme`] once, with the group and description
/// shown in the theme editor, as the egui app's. The field names are the
/// keys of theme files.
macro_rules! theme_colors {
    ($($group:literal { $($field:ident: $label:literal, $description:literal,)* })*) => {
        /// Every colour the interface uses, named by role, as the egui
        /// app's theme.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Theme {
            pub name: String,
            $($(
                #[doc = $description]
                pub $field: Color,
            )*)*
            /// Categorical colours: new tracks.
            pub palette: Vec<Color>,
        }

        /// Every colour of a [`Theme`], in display order.
        pub const COLORS: &[ColorRole] = &[
            $($(ColorRole {
                key: stringify!($field),
                group: $group,
                label: $label,
            },)*)*
        ];

        impl Theme {
            /// All colours transparent: the start of the base theme, which
            /// sets them all.
            fn blank() -> Self {
                Self {
                    name: String::new(),
                    $($($field: Color::rgba(0, 0, 0, 0),)*)*
                    palette: Vec::new(),
                }
            }

            /// The colour stored under `key` in theme files.
            pub fn color(&self, key: &str) -> Option<Color> {
                match key {
                    $($(stringify!($field) => Some(self.$field),)*)*
                    _ => None,
                }
            }

            pub fn color_mut(&mut self, key: &str) -> Option<&mut Color> {
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
        control_border: "Control outline", "Around buttons, inputs and selects at rest, where their fill alone doesn't stand out (light themes); transparent for none.",
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
        selection_fill: "Selection fill", "Inside a rubber-band selection: a light veil.",
        selection_stroke: "Selection outline", "Dashed outline of a rubber-band selection.",
        clip_selected: "Selected clip", "What the selected part of a clip is lit towards: neutral, so it goes with any clip colour.",
        loop_region: "Loop", "Loop bounds.",
        grid_bar: "Grid bars", "Grid lines on bars.",
        grid_beat: "Grid beats", "Grid lines on beats (half as strong between beats).",
    }
    "Meters" {
        meter_low: "Normal", "Level meters, most of their range.",
        meter_mid: "Hot", "Level meters, from -12 dB to full scale (deepening towards the clipping colour), blended in from the normal colour.",
        meter_high: "Clipping", "Level meters, over full scale, and their clip lamps.",
    }
}

/// A colour of the theme, as listed in theme files and the editor.
pub struct ColorRole {
    /// Field of [`Theme`] and key in theme files.
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
}

impl Theme {
    /// The built-in Dark theme, from its file.
    pub fn dark() -> Self {
        static DARK: LazyLock<Theme> = LazyLock::new(library::base_theme);
        DARK.clone()
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
        type Check<'a> = (&'static str, Color, &'a [(&'static str, Color)]);
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
                    .map(|(name, background)| (*name, contrast(color, *background)))
                    .min_by(|first, second| first.1.total_cmp(&second.1))?;
                (ratio < 4.5).then(|| (key, format!("{ratio:.1}:1 on {on}, needs 4.5:1")))
            })
            .collect()
    }

    /// The text colour readable on `background`: the theme's text or its
    /// deepest background.
    pub fn text_on(&self, background: Color) -> Color {
        if contrast(self.text, background) >= contrast(self.bg_deep, background) {
            self.text
        } else {
            self.bg_deep
        }
    }
}

/// A colour as gpui's.
pub fn hsla(color: Color) -> Hsla {
    let channel = |value: u8| value as f32 / 255.;
    Rgba {
        r: channel(color.r),
        g: channel(color.g),
        b: channel(color.b),
        a: channel(color.a),
    }
    .into()
}

/// `from` mixed towards `to` by `amount`, channel by channel in sRGB with
/// alpha premultiplied, as Tonique mixes its theme's colours.
pub fn mix(from: Color, to: Color, amount: f32) -> Color {
    let premultiplied = |color: Color| {
        let opacity = color.a as f32 / 255.;
        [
            color.r as f32 * opacity,
            color.g as f32 * opacity,
            color.b as f32 * opacity,
            color.a as f32,
        ]
    };
    let (start, end) = (premultiplied(from), premultiplied(to));
    let [red, green, blue, alpha] =
        [0, 1, 2, 3].map(|channel| start[channel] + (end[channel] - start[channel]) * amount);
    if alpha <= 0. {
        return Color::rgba(0, 0, 0, 0);
    }
    let unpremultiplied = |value: f32| (value * 255. / alpha).round().clamp(0., 255.) as u8;
    Color::rgba(
        unpremultiplied(red),
        unpremultiplied(green),
        unpremultiplied(blue),
        alpha.round() as u8,
    )
}

pub fn with_alpha(color: Color, alpha: u8) -> Color {
    Color { a: alpha, ..color }
}

/// `overlay` (translucent) painted over `color`.
pub fn over(color: Color, overlay: Color) -> Color {
    mix(color, with_alpha(overlay, 255), overlay.a as f32 / 255.)
}

fn luminance(color: Color) -> f32 {
    let linear = |value: u8| {
        let value = value as f32 / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// WCAG contrast ratio between two opaque colours.
pub fn contrast(first: Color, second: Color) -> f32 {
    let (first, second) = (luminance(first), luminance(second));
    (first.max(second) + 0.05) / (first.min(second) + 0.05)
}
