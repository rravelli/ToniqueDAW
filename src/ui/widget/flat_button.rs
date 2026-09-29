use egui::{
    Align2, Color32, CursorIcon, FontFamily, FontId, RichText, Sense, Stroke, Vec2, Widget, vec2,
};

use crate::ui::theme::ThemeExt;

/// Button with a solid fill. Colours default to the theme's control colours
/// (or the accent when [`Self::selected`]); `fill` and `color` override them.
pub struct FlatButton {
    size: Vec2,
    ghost: bool,
    selected: bool,
    fill: Option<Color32>,
    border_radius: f32,
    /// Horizontal padding around the text when the width fits the text.
    padding: Option<f32>,
    // Text
    text: String,
    text_color: Option<Color32>,
    font: FontId,
    // Tooltip
    tooltip_text: String,
}

impl FlatButton {
    pub fn new(text: impl ToString) -> Self {
        Self {
            size: vec2(15., 15.),
            ghost: false,
            selected: false,
            fill: None,
            text: text.to_string(),
            font: FontId::proportional(8.),
            text_color: None,
            tooltip_text: "".to_string(),
            border_radius: 1.0,
            padding: None,
        }
    }
    /// No fill until hovered; muted text, accent text when selected.
    pub fn ghost(text: impl ToString) -> Self {
        Self {
            ghost: true,
            ..Self::new(text)
        }
    }
    /// Show as switched on: accent fill (accent text for a ghost button).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn fill(mut self, bg: Color32) -> Self {
        self.fill = Some(bg);
        self
    }
    pub fn font(mut self, font_id: FontId) -> Self {
        self.font = font_id;
        self
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }
    pub fn square(mut self, size: f32) -> Self {
        self.size = vec2(size, size);
        self
    }
    pub fn size(mut self, size: Vec2) -> Self {
        self.size = size;
        self
    }
    pub fn family(mut self, family: FontFamily) -> Self {
        self.font.family = family;
        self
    }
    pub fn tooltip(mut self, text: impl ToString) -> Self {
        self.tooltip_text = text.to_string();
        self
    }
    pub fn border_radius(mut self, border_radius: f32) -> Self {
        self.border_radius = border_radius;
        self
    }
    /// Size the width to the text plus `padding` on each side (the height
    /// stays as set).
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Some(padding);
        self
    }
}

impl Widget for FlatButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        // Disabled through `ui.add_enabled` or a disabled parent
        let enabled = ui.is_enabled();
        let theme = ui.app_theme();
        let (fill, text_color, hover) = match (self.ghost, self.selected) {
            (false, false) => (theme.bg_control, theme.text, theme.bg_control_hover),
            (false, true) => (theme.accent, theme.text_on_accent, theme.accent_hover),
            (true, false) => (Color32::TRANSPARENT, theme.text_muted, theme.hover_overlay),
            (true, true) => (Color32::TRANSPARENT, theme.accent, theme.hover_overlay),
        };
        let fill = self.fill.unwrap_or(fill);
        let text_color = self.text_color.unwrap_or(text_color);
        let hover = match self.fill {
            // A custom fill lightens (or darkens) like the others.
            Some(fill) if !self.ghost => fill.blend(theme.hover_overlay),
            _ => hover,
        };

        let mut size = self.size;
        let galley = ui
            .painter()
            .layout_no_wrap(self.text, self.font, text_color);
        if let Some(padding) = self.padding {
            size.x = galley.size().x + 2. * padding;
        }
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (mut res, painter) = ui.allocate_painter(size, sense);
        let rect = res.rect;
        let mut curr_color = fill;
        let mut text_color = text_color;
        let mut stroke = Stroke::NONE;
        if !enabled {
            curr_color = curr_color.gamma_multiply(0.5);
            text_color = text_color.gamma_multiply(0.5);
        } else if res.hovered() {
            curr_color = hover;
        }
        if res.has_focus() {
            stroke = Stroke::new(1.0, theme.accent);
        }
        // Paint widget
        painter.rect(
            rect,
            self.border_radius,
            curr_color,
            stroke,
            egui::StrokeKind::Inside,
        );

        painter.galley(
            Align2::CENTER_CENTER
                .align_size_within_rect(galley.size(), rect)
                .min,
            galley,
            text_color,
        );
        // Update response
        if enabled {
            res = res.on_hover_cursor(CursorIcon::PointingHand);
        }

        if !self.tooltip_text.is_empty() {
            res = res.on_hover_text(
                RichText::new(self.tooltip_text)
                    .font(FontId::new(8., egui::FontFamily::Proportional)),
            )
        }

        res
    }
}
