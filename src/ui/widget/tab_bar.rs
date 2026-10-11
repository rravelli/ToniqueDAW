use crate::ui::theme::font_size;
use egui::{Align2, CursorIcon, FontId, Rect, Response, Sense, Stroke, Ui, Widget, pos2, vec2};

use crate::ui::theme::ThemeExt;

/// Thickness of the line under the current tab.
const UNDERLINE: f32 = 2.;
const HEIGHT: f32 = 24.;

/// Row of equal-width tabs spanning the available width, the current one
/// underlined. The response is marked changed when the user picks another
/// tab.
pub struct TabBar<'a, T> {
    value: &'a mut T,
    tabs: Vec<(T, String)>,
}

impl<'a, T: PartialEq + Copy> TabBar<'a, T> {
    pub fn new(value: &'a mut T, tabs: impl IntoIterator<Item = (T, impl ToString)>) -> Self {
        Self {
            value,
            tabs: tabs
                .into_iter()
                .map(|(tab, name)| (tab, name.to_string()))
                .collect(),
        }
    }
}

impl<T: PartialEq + Copy> Widget for TabBar<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut changed = false;
        let mut response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.;
                let theme = ui.app_theme();
                let width = ui.available_width() / self.tabs.len().max(1) as f32;
                for (tab, name) in self.tabs {
                    let selected = *self.value == tab;
                    let (rect, res) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click());
                    let painter = ui.painter_at(rect);
                    if res.hovered() && !selected {
                        painter.rect_filled(rect, 2., theme.hover_overlay);
                    }
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        name,
                        FontId::proportional(font_size::BODY),
                        if selected {
                            theme.text
                        } else {
                            theme.text_muted
                        },
                    );
                    if selected {
                        let line = Rect::from_min_max(
                            pos2(rect.left(), rect.bottom() - UNDERLINE),
                            rect.max,
                        );
                        painter.rect_filled(line, 0., theme.accent);
                    } else {
                        painter.hline(
                            rect.x_range(),
                            rect.bottom() - 0.5,
                            Stroke::new(1., theme.border),
                        );
                    }
                    if res.on_hover_cursor(CursorIcon::PointingHand).clicked() && !selected {
                        *self.value = tab;
                        changed = true;
                    }
                }
            })
            .response;
        if changed {
            response.mark_changed();
        }
        response
    }
}
