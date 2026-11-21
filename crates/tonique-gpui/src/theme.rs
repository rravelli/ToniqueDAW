//! Tonique's default "Dark" theme (`ui/theme/presets/dark.toml`), and the
//! colour maths Tonique does on it, in the same sRGB space, on the
//! project's own colour type.

pub use crate::utils::color::Color;
use gpui::{Font, Hsla, Rgba, font};

pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono NL";

/// Font sizes, as `ui::theme::font_size`.
pub mod size {
    pub const SMALL: f32 = 10.;
    pub const LABEL: f32 = 11.;
    pub const BODY: f32 = 12.;
    pub const HEADING: f32 = 14.;
}

/// Loads the fonts the egui app bundles, so text is compared like for like.
pub fn load_fonts(cx: &mut gpui::App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        include_bytes!("../../../fonts/Inter-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../fonts/Inter-SemiBold.ttf")
            .as_slice()
            .into(),
        include_bytes!("../../../fonts/JetBrainsMonoNL-Regular.ttf")
            .as_slice()
            .into(),
        include_bytes!("../assets/fonts/Phosphor.ttf")
            .as_slice()
            .into(),
        include_bytes!("../assets/fonts/Phosphor-Fill.ttf")
            .as_slice()
            .into(),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("Couldn't load the fonts: {e}");
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

const fn rgb(hex: u32) -> Color {
    Color::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn rgba(hex: u32) -> Color {
    Color::rgba(
        (hex >> 24) as u8,
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
    )
}

pub const BLACK: Color = Color::rgb(0, 0, 0);
pub const WHITE: Color = Color::rgb(255, 255, 255);

#[derive(Clone)]
pub struct Theme {
    pub bg_deep: Color,
    pub bg_base: Color,
    pub bg_panel: Color,
    pub bg_raised: Color,
    pub bg_control: Color,
    pub bg_control_hover: Color,
    pub border: Color,
    pub separator: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_disabled: Color,
    pub text_on_accent: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub danger: Color,
    pub warning: Color,
    pub hover_overlay: Color,
    pub shadow: Color,
    pub playhead: Color,
    pub edit_cursor: Color,
    pub clip_selected: Color,
    pub loop_region: Color,
    pub grid_bar: Color,
    pub grid_beat: Color,
    pub meter_low: Color,
    pub meter_mid: Color,
    pub meter_high: Color,
    pub palette: Vec<Color>,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg_deep: rgb(0x0f1014),
            bg_base: rgb(0x1b1d21),
            bg_panel: rgb(0x222529),
            bg_raised: rgb(0x2a2d32),
            bg_control: rgb(0x363a40),
            bg_control_hover: rgb(0x42474e),
            border: rgb(0x464b53),
            separator: rgb(0x2f3338),
            text: rgb(0xe8eaed),
            text_muted: rgb(0xa4a9b1),
            text_disabled: rgb(0x6c717a),
            text_on_accent: rgb(0x0a1416),
            accent: rgb(0x00c9d4),
            accent_hover: rgb(0x33d4dd),
            danger: rgb(0xff6b6b),
            warning: rgb(0xf5a524),
            hover_overlay: rgba(0xffffff0e),
            shadow: rgba(0x0000008c),
            playhead: rgb(0xf2f3f5),
            edit_cursor: rgb(0xe8eaed),
            clip_selected: rgb(0xf2f3f5),
            loop_region: rgba(0x00c9d428),
            grid_bar: rgba(0xffffff16),
            grid_beat: rgba(0xffffff09),
            meter_low: rgb(0x4cc864),
            meter_mid: rgb(0xf5c542),
            meter_high: rgb(0xff4d4f),
            palette: [
                0xe5736b, 0xeba35a, 0xe3c85a, 0x9ccf6a, 0x4fc49a, 0x4fb8d9, 0x6f9ceb, 0x9a86e8,
                0xd17ad1, 0xe07aa0,
            ]
            .into_iter()
            .map(rgb)
            .collect(),
        }
    }

    /// The text colour readable on `bg`: the theme's text or its deepest
    /// background.
    pub fn text_on(&self, bg: Color) -> Color {
        if contrast(self.text, bg) >= contrast(self.bg_deep, bg) {
            self.text
        } else {
            self.bg_deep
        }
    }
}

/// A colour as gpui's.
pub fn h(c: Color) -> Hsla {
    let Color { r, g, b, a } = c;
    Rgba {
        r: r as f32 / 255.,
        g: g as f32 / 255.,
        b: b as f32 / 255.,
        a: a as f32 / 255.,
    }
    .into()
}

/// `a` mixed towards `b` by `t`, channel by channel in sRGB with alpha
/// premultiplied, as Tonique mixes its theme's colours.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let pre = |c: Color| {
        let k = c.a as f32 / 255.;
        [c.r as f32 * k, c.g as f32 * k, c.b as f32 * k, c.a as f32]
    };
    let (pa, pb) = (pre(a), pre(b));
    let [r, g, b, alpha] = [0, 1, 2, 3].map(|i| pa[i] + (pb[i] - pa[i]) * t);
    if alpha <= 0. {
        return Color::rgba(0, 0, 0, 0);
    }
    let un = |v: f32| (v * 255. / alpha).round().clamp(0., 255.) as u8;
    Color::rgba(un(r), un(g), un(b), alpha.round() as u8)
}

pub fn with_alpha(c: Color, alpha: u8) -> Color {
    Color { a: alpha, ..c }
}

/// `overlay` (translucent) painted over `c`.
pub fn over(c: Color, overlay: Color) -> Color {
    mix(c, with_alpha(overlay, 255), overlay.a as f32 / 255.)
}

fn luminance(c: Color) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two opaque colours.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
