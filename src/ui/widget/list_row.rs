use egui::{Align2, FontId, Pos2, Sense, TextWrapMode, Vec2, Widget, WidgetText};

use crate::ui::theme::{ThemeExt, with_alpha};

const HEIGHT: f32 = 16.;
const FONT_SIZE: f32 = 12.;
/// Left of the row to the indent.
const PADDING: f32 = 6.;
/// Width of the caret's and the icon's slots.
pub const SLOT: f32 = 14.;

/// A row of a list or tree: an optional caret and icon, muted, then the
/// text, cut with an ellipsis when it doesn't fit (the full text then shows
/// on hover).
pub struct ListRow {
    text: String,
    selected: bool,
    indent: f32,
    caret: Option<&'static str>,
    icon: Option<&'static str>,
}

impl ListRow {
    pub fn new(text: impl ToString) -> Self {
        Self {
            text: text.to_string(),
            selected: false,
            indent: 0.,
            caret: None,
            icon: None,
        }
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

        let font = FontId::proportional(FONT_SIZE);
        let mut x = rect.left() + PADDING + self.indent;
        for glyph in [self.caret, self.icon].into_iter().flatten() {
            painter.text(
                Pos2::new(x + SLOT / 2., rect.center().y),
                Align2::CENTER_CENTER,
                glyph,
                font.clone(),
                theme.text_muted,
            );
            x += SLOT;
        }
        if self.icon.is_some() {
            x += 2.;
        }

        let galley = WidgetText::from(self.text.as_str()).into_galley(
            ui,
            Some(TextWrapMode::Truncate),
            (rect.right() - x - PADDING).max(0.),
            font,
        );
        let elided = galley.elided;
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
