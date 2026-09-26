use crate::{
    analysis::AudioInfo,
    core::state::ToniqueProjectState,
    ui::{
        effects::EffectId,
        theme::PRIMARY_COLOR,
        view::filebrowser::FileBrowser,
        widget::{item_button::ItemButton, search_bar::SearchBar, square_button::SquareButton},
    },
};
use egui::{Color32, Frame, Margin, Ui, vec2};

/// Space around and between the tab bar and the search bar.
const HEADER_SPACING: f32 = 4.;

#[derive(Clone, Copy, PartialEq)]
pub enum LeftPanelTabs {
    Files,
    Effects,
}

#[derive(Clone)]
pub enum DragPayload {
    File(AudioInfo),
    Effect(EffectId),
}

pub struct UILeftPanel {
    pub file_browser: FileBrowser,
    tab: LeftPanelTabs,
    search: SearchBar,
}

impl UILeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FileBrowser::new(),
            tab: LeftPanelTabs::Files,
            search: SearchBar::new("Search"),
        }
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let mut open = state.left_panel_open;
        egui::Panel::left("left-pannel")
            .min_size(100.)
            .max_size(400.)
            .frame(
                Frame::new()
                    .inner_margin(Margin {
                        bottom: 0,
                        left: 2,
                        right: 2,
                        top: 0,
                    })
                    .fill(ui.style().visuals.panel_fill)
                    .corner_radius(4.0),
            )
            .default_size(220.)
            .show_collapsible(ui, &mut open, |ui| {
                self.ui(ui, state);
            });
        state.left_panel_open = open;
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            Frame::new()
                .inner_margin(Margin::symmetric(2, HEADER_SPACING as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = HEADER_SPACING;
                    self.tab_bar(ui);
                    if let Some(query) = self.search.ui(ui) {
                        self.file_browser.trigger_search(query);
                    }
                });

            match self.tab {
                LeftPanelTabs::Files => {
                    self.file_browser.ui(ui, state);
                }
                LeftPanelTabs::Effects => {
                    let res = ui.add(ItemButton::new(format!(
                        "{} {}",
                        egui_phosphor::fill::STAR_FOUR,
                        "Filter"
                    )));
                    res.dnd_set_drag_payload(DragPayload::Effect(EffectId::Equalizer));
                }
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            self.tab_bar_button(ui, LeftPanelTabs::Files, "Files", width);
            self.tab_bar_button(ui, LeftPanelTabs::Effects, "Effects", width);
        });
    }

    fn tab_bar_button(&mut self, ui: &mut Ui, value: LeftPanelTabs, name: &str, width: f32) {
        let res = ui.add(SquareButton::new(name).size(vec2(width, 25.)).fill(
            if self.tab == value {
                PRIMARY_COLOR
            } else {
                Color32::from_gray(100)
            },
        ));

        if res.clicked() {
            self.tab = value;
        }
    }
}
