use egui::{Align2, Color32, FontId, Pos2, Sense, Stroke, TextWrapMode, Vec2, Widget, WidgetText};

use crate::ui::theme::{ThemeExt, with_alpha};

const HEIGHT: f32 = 16.;
const FONT_SIZE: f32 = 12.;
/// Left of the row to the indent.
const PADDING: f32 = 6.;
/// Width of the caret's and the icon's slots.
pub const SLOT: f32 = 14.;
/// Room for the dot at the row's end, and its size.
const DOT_ROOM: f32 = 12.;
const DOT_RADIUS: f32 = 3.;

/// Where the dot at the end of a row in `rect` is: rows of a tree drawn
/// one under another line theirs up.
pub fn dot_center(rect: egui::Rect) -> Pos2 {
    Pos2::new(rect.right() - PADDING - DOT_RADIUS, rect.center().y)
}
/// How opaque indent guides are, in muted ink: faint.
const GUIDE_ALPHA: u8 = 50;

/// A row of a list or tree: an optional caret and icon, muted, then the
/// text, cut with an ellipsis when it doesn't fit (the full text then shows
/// on hover).
pub struct ListRow {
    text: String,
    selected: bool,
    indent: f32,
    caret: Option<&'static str>,
    icon: Option<&'static str>,
    /// Drawn in this colour rather than muted.
    icon_color: Option<Color32>,
    /// Indent guides: a line down the row under each of this many levels'
    /// carets.
    guides: usize,
    /// A dot of this colour at the row's end (a label's).
    dot: Option<Color32>,
}

impl ListRow {
    pub fn new(text: impl ToString) -> Self {
        Self {
            text: text.to_string(),
            selected: false,
            indent: 0.,
            caret: None,
            icon: None,
            icon_color: None,
            guides: 0,
            dot: None,
        }
    }

    /// A dot of `color` at the row's end, or none.
    pub fn dot(mut self, color: Option<Color32>) -> Self {
        self.dot = color;
        self
    }

    /// Lines down the row, one under each of `levels` carets before its
    /// own: rows of a tree drawn one under another join them up.
    pub fn guides(mut self, levels: usize) -> Self {
        self.guides = levels;
        self
    }

    /// The icon in `color`, to set it apart.
    pub fn icon_color(mut self, color: Color32) -> Self {
        self.icon_color = Some(color);
        self
    }

    pub fn selected(mut self, val: bool) -> Self {
        self.selected = val;
        self
    }

    /// Space before the caret, in points.
    pub fn indent(mut self, indent: f32) -> Self {
        self.indent = indent;
        self
    }

    /// Drawn in a slot before the icon; rows without one keep the slot
    /// empty when given `None`, so tree rows line up.
    pub fn caret(mut self, caret: Option<&'static str>) -> Self {
        self.caret = caret;
        self.indent += if caret.is_none() { SLOT } else { 0. };
        self
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl Widget for ListRow {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), HEIGHT),
            Sense::click_and_drag(),
        );

        let theme = ui.app_theme();
        let bg_color = if self.selected || response.has_focus() {
            with_alpha(theme.accent, 60)
        } else if response.hovered() {
            theme.hover_overlay
        } else {
            egui::Color32::TRANSPARENT
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., bg_color);
        for level in 0..self.guides {
            let x = rect.left() + PADDING + level as f32 * SLOT + SLOT / 2.;
            painter.vline(
                x.round() + 0.5,
                rect.y_range(),
                Stroke::new(1., with_alpha(theme.text_muted, GUIDE_ALPHA)),
            );
        }

        let font = FontId::proportional(FONT_SIZE);
        let mut x = rect.left() + PADDING + self.indent;
        let icon_color = self.icon_color.unwrap_or(theme.text_muted);
        for (glyph, color) in [(self.caret, theme.text_muted), (self.icon, icon_color)] {
            let Some(glyph) = glyph else {
                continue;
            };
            painter.text(
                Pos2::new(x + SLOT / 2., rect.center().y),
                Align2::CENTER_CENTER,
                glyph,
                font.clone(),
                color,
            );
            x += SLOT;
        }
        if self.icon.is_some() {
            x += 2.;
        }

        let dot_room = if self.dot.is_some() { DOT_ROOM } else { 0. };
        let galley = WidgetText::from(self.text.as_str()).into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            (rect.right() - x - PADDING - dot_room).max(0.),
            font,
        );
        let elided = galley.elided;
        if let Some(color) = self.dot {
            painter.circle_filled(dot_center(rect), DOT_RADIUS, color);
        }
        painter.galley(
            Pos2::new(x, rect.center().y - galley.size().y / 2.),
            galley,
            theme.text,
        );

        if elided {
            response.on_hover_text(self.text)
        } else {
            response
        }
    }
}
