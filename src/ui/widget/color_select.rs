use egui::{
    Color32, CursorIcon, Id, Response, Sense, Stroke, StrokeKind, Ui, Widget,
    color_picker::{Alpha, color_picker_color32},
    vec2,
};

use crate::ui::{theme::ThemeExt, widget::context_menu::ContextMenuButton};

const SWATCH: f32 = 18.;
const GAP: f32 = 4.;
const PER_ROW: usize = 5;

/// The theme's palette as swatches, plus a custom colour picker shown on
/// demand. The response is marked changed when the colour changes. Picking
/// a swatch closes the menu or popup it's in; the custom picker keeps it
/// open while adjusting.
pub struct ColorSelect<'a> {
    color: &'a mut Color32,
    id_salt: Id,
}

impl<'a> ColorSelect<'a> {
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug, color: &'a mut Color32) -> Self {
        Self {
            color,
            id_salt: Id::new(id_salt),
        }
    }
}

impl Widget for ColorSelect<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = ui.app_theme();
        let custom_id = ui.make_persistent_id(self.id_salt).with("custom");
        let mut changed = false;

        let mut response = ui
            .vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                for row in theme.palette.chunks(PER_ROW) {
                    ui.horizontal(|ui| {
                        for swatch in row {
                            let current = *swatch == *self.color;
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                            let outline = if current {
                                Stroke::new(2., theme.clip_selected)
                            } else if response.hovered() {
                                Stroke::new(1., theme.text_muted)
                            } else {
                                Stroke::NONE
                            };
                            ui.painter()
                                .rect(rect, 3., *swatch, outline, StrokeKind::Outside);
                            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                                *self.color = *swatch;
                                changed = true;
                                ui.data_mut(|d| d.insert_temp(custom_id, false));
                                ui.close();
                            }
                        }
                    });
                }

                ui.add_space(2.);
                let mut custom = ui.data(|d| d.get_temp(custom_id).unwrap_or(false));
                if ui
                    .add(ContextMenuButton::new(
                        egui_phosphor::regular::EYEDROPPER,
                        "Custom colour",
                    ))
                    .clicked()
                {
                    custom = !custom;
                    ui.data_mut(|d| d.insert_temp(custom_id, custom));
                }
                if custom {
                    changed |= color_picker_color32(ui, self.color, Alpha::Opaque);
                }
            })
            .response;
        if changed {
            response.mark_changed();
        }
        response
    }
}
