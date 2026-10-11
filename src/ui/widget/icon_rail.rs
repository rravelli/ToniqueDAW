use crate::ui::theme::font_size;
use egui::{Align2, CursorIcon, FontId, Rect, Response, RichText, Sense, Ui, Widget, pos2, vec2};

use crate::ui::theme::ThemeExt;

/// Thickness of the bar beside the current item.
const INDICATOR: f32 = 2.;

/// Column of square icon buttons, one per item, the current one marked by an
/// accent bar on its left edge. Each shows its name on hover. The response
/// is marked changed when the user picks another item.
pub struct IconRail<'a, T> {
    value: &'a mut T,
    items: Vec<(T, &'static str, String)>,
    size: f32,
}

impl<'a, T: PartialEq + Copy> IconRail<'a, T> {
    /// `items` are (value, icon, name).
    pub fn new(
        value: &'a mut T,
        items: impl IntoIterator<Item = (T, &'static str, impl ToString)>,
    ) -> Self {
        Self {
            value,
            items: items
                .into_iter()
                .map(|(item, icon, name)| (item, icon, name.to_string()))
                .collect(),
            size: 28.,
        }
    }

    /// Side of each button.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl<T: PartialEq + Copy> Widget for IconRail<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut changed = false;
        let mut response = ui
            .vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                let theme = ui.app_theme();
                for (item, icon, name) in self.items {
                    let selected = *self.value == item;
                    let (rect, res) =
                        ui.allocate_exact_size(vec2(self.size, self.size), Sense::click());
                    let painter = ui.painter_at(rect);
                    if res.hovered() && !selected {
                        painter.rect_filled(rect, 2., theme.hover_overlay);
                    }
                    painter.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        icon,
                        FontId::proportional(font_size::HEADING + 2.),
                        if selected {
                            theme.text
                        } else {
                            theme.text_muted
                        },
                    );
                    if selected {
                        let bar = Rect::from_min_max(
                            rect.min,
                            pos2(rect.left() + INDICATOR, rect.bottom()),
                        );
                        painter.rect_filled(bar, 0., theme.accent);
                    }
                    let res = res
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text(RichText::new(name).size(font_size::SMALL));
                    if res.clicked() && !selected {
                        *self.value = item;
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
