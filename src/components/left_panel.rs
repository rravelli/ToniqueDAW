use std::time::{Duration, Instant};

use egui::{
    Button, Color32, FontId, FontSelection, Frame, Margin, RichText, Stroke, TextEdit, Ui, Vec2,
};
use rtrb::Producer;

use crate::{
    analysis::AudioInfo,
    components::{buttons::left_aligned_selectable, effects::EffectId, filebrowser::FileBrowser},
    message::GuiToPlayerMsg,
};

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
    search: String,
    last_search: String,
    last_search_time: Option<Instant>,
}

impl UILeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FileBrowser::new(),
            tab: LeftPanelTabs::Files,
            search: "".into(),
            last_search: "".into(),
            last_search_time: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, tx: &mut Producer<GuiToPlayerMsg>) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            self.tab_bar(ui);
            self.search_bar(ui);
            ui.add_space(4.0);

            match self.tab {
                LeftPanelTabs::Files => {
                    self.file_browser.ui(ui, tx);
                }
                LeftPanelTabs::Effects => {
                    let res = left_aligned_selectable(
                        ui,
                        format!("{} {}", egui_phosphor::fill::STAR_FOUR, "Filter"),
                        false,
                    );
                    res.dnd_set_drag_payload(DragPayload::Effect(EffectId::Equalizer));
                }
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        Frame::new().inner_margin(Margin::same(2)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing = Vec2::new(4.0, 4.0);
                self.tab_bar_button(ui, egui_phosphor::fill::FILES, LeftPanelTabs::Files);
                self.tab_bar_button(ui, egui_phosphor::fill::SPARKLE, LeftPanelTabs::Effects);
            });
        });
    }

    fn tab_bar_button(&mut self, ui: &mut Ui, icon: &str, value: LeftPanelTabs) {
        let res = ui.add(
            Button::new(icon)
                .fill(if self.tab == value {
                    ui.style().visuals.extreme_bg_color
                } else {
                    Color32::TRANSPARENT
                })
                .selected(self.tab == value)
                .min_size(Vec2::new(30., 20.)),
        );

        if res.clicked() {
            self.tab = value;
        }
    }

    fn search_bar(&mut self, ui: &mut Ui) {
        Frame::new()
            .stroke(Stroke::new(2.0, Color32::from_gray(100)))
            .corner_radius(2.0)
            .inner_margin(Margin::symmetric(1, 0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                            .family(egui::FontFamily::Name("phosphor_regular".into())),
                    );
                    TextEdit::singleline(&mut self.search)
                        .background_color(Color32::TRANSPARENT)
                        .frame(false)
                        .desired_width(ui.available_width() - 20.)
                        .font(FontSelection::FontId(FontId::new(
                            10.,
                            egui::FontFamily::Proportional,
                        )))
                        .show(ui);
                    if !self.search.is_empty() {
                        let x_response = ui.label(
                            RichText::new(egui_phosphor::regular::X)
                                .size(10.)
                                .family(egui::FontFamily::Name("phosphor_regular".into())),
                        );
                        if x_response.clicked() {
                            self.search = "".into();
                        }
                        x_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                })
            });
        if self.search != self.last_search {
            self.last_search = self.search.clone();
            self.last_search_time = Some(Instant::now());
        } else if let Some(last_time) = self.last_search_time
            && last_time.elapsed() >= Duration::from_millis(300)
        {
            self.file_browser.trigger_search(self.search.clone());
            self.last_search_time = None;
        }
    }
}
