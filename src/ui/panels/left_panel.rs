use crate::{
    analysis::AudioInfo,
    core::state::ToniqueProjectState,
    ui::{
        effects::EffectId,
        font::PHOSPHOR_REGULAR,
        theme::PRIMARY_COLOR,
        view::filebrowser::FileBrowser,
        widget::{item_button::ItemButton, square_button::SquareButton},
    },
};
use egui::{Color32, Context, FontId, Frame, Margin, RichText, Stroke, TextEdit, Ui, vec2};
use std::time::{Duration, Instant};

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

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        egui::SidePanel::left("left-pannel")
            .min_width(100.)
            .max_width(400.)
            .frame(
                Frame::new()
                    .inner_margin(Margin {
                        bottom: 0,
                        left: 2,
                        right: 2,
                        top: 0,
                    })
                    .fill(ctx.style().visuals.panel_fill)
                    .corner_radius(4.0),
            )
            .default_width(220.)
            .show_animated(ctx, state.left_panel_open, |ui| {
                self.ui(ui, state);
            });
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            self.tab_bar(ui);
            self.search_bar(ui);
            ui.add_space(4.0);

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
        Frame::new().inner_margin(Margin::same(2)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing.x = 2.0;
                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                self.tab_bar_button(ui, LeftPanelTabs::Files, "Files", width);
                self.tab_bar_button(ui, LeftPanelTabs::Effects, "Effects", width);
            });
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

    fn search_bar(&mut self, ui: &mut Ui) {
        Frame::new()
            .stroke(Stroke::new(2.0, Color32::from_gray(100)))
            .corner_radius(2.0)
            .inner_margin(Margin::symmetric(2, 1))
            .fill(Color32::from_gray(180))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                            .color(Color32::from_gray(30))
                            .family(egui::FontFamily::Name(PHOSPHOR_REGULAR.into())),
                    );
                    TextEdit::singleline(&mut self.search)
                        .background_color(Color32::TRANSPARENT)
                        .frame(false)
                        .desired_width(ui.available_width() - 20.)
                        .font(FontId::new(10., egui::FontFamily::Proportional))
                        .text_color(Color32::from_gray(30))
                        .show(ui)
                        .response;

                    if !self.search.is_empty() {
                        let x_response = ui.add(
                            SquareButton::ghost(egui_phosphor::regular::X)
                                .border_radius(5.0)
                                .square(10.)
                                .color(Color32::from_gray(30))
                                .font(FontId::new(
                                    10.,
                                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                                )),
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
            self.file_browser.trigger_search(&self.search);
            self.last_search_time = None;
        }
    }
}
