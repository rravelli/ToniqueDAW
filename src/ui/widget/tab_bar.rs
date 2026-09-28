use egui::{FontId, Response, Ui, Widget, vec2};

use crate::ui::widget::flat_button::FlatButton;

const GAP: f32 = 2.;

/// Row of equal-width tabs spanning the available width. The response is
/// marked changed when the user picks another tab.
pub struct TabBar<'a, T> {
    value: &'a mut T,
    tabs: Vec<(T, String)>,
    height: f32,
}

impl<'a, T: PartialEq + Copy> TabBar<'a, T> {
    pub fn new(value: &'a mut T, tabs: impl IntoIterator<Item = (T, impl ToString)>) -> Self {
        Self {
            value,
            tabs: tabs
                .into_iter()
                .map(|(tab, name)| (tab, name.to_string()))
                .collect(),
            height: 24.,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl<T: PartialEq + Copy> Widget for TabBar<'_, T> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut changed = false;
        let mut response = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                let count = self.tabs.len().max(1) as f32;
                let width = (ui.available_width() - GAP * (count - 1.)) / count;
                for (tab, name) in self.tabs {
                    let selected = *self.value == tab;
                    let button = FlatButton::new(name)
                        .size(vec2(width, self.height))
                        .font(FontId::proportional(12.))
                        .border_radius(2.)
                        .selected(selected);
                    if ui.add(button).clicked() && !selected {
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
