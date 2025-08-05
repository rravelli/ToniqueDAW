use egui::{Button, Ui};
use rtrb::Producer;

use crate::{
    analysis::AudioInfo,
    components::{
        buttons::left_aligned_selectable,
        effect::{UIEffect, UIEffectContent},
        effects::{EffectId, equalizer::EqualizerEffect},
        filepicker::FilePicker,
    },
    message::GuiToPlayerMsg,
};

#[derive(Clone, Copy, PartialEq)]
pub enum LeftPanelTabs {
    Files,
    Effects,
}

#[derive(Clone)]
pub enum DraggedItem {
    File(AudioInfo),
    Effect(Box<dyn UIEffectContent>),
    None,
}

#[derive(Clone)]
pub enum DragPayload {
    File(AudioInfo),
    Effect(EffectId),
    None,
}

pub struct UILeftPanel {
    pub file_browser: FilePicker,
    tab: LeftPanelTabs,

    pub dragged_item: DraggedItem,
    pub released_item: DraggedItem,
}

impl UILeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FilePicker::new(),
            tab: LeftPanelTabs::Files,
            dragged_item: DraggedItem::None,
            released_item: DraggedItem::None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        tx: &mut Producer<GuiToPlayerMsg>,
    ) -> (Option<AudioInfo>, bool) {
        let mut dragged_audio_info = None;
        let mut is_released = false;

        self.released_item = DraggedItem::None;

        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            self.tab_bar(ui);

            ui.separator();

            if self.tab == LeftPanelTabs::Files {
                (dragged_audio_info, is_released) = self.file_browser.ui(ui, tx);
                if let Some(audio_info) = dragged_audio_info.clone() {
                    self.dragged_item = DraggedItem::File(audio_info);
                }
            }

            if self.tab == LeftPanelTabs::Effects {
                let res = left_aligned_selectable(
                    ui,
                    format!("{} {}", egui_phosphor::fill::STAR_FOUR, "Filter"),
                    false,
                );
                res.dnd_set_drag_payload(DragPayload::Effect(EffectId::Equalizer));
                if res.drag_started() {
                    self.dragged_item = DraggedItem::Effect(Box::new(EqualizerEffect::new()));
                }
                if res.drag_stopped() {
                    self.released_item = self.dragged_item.clone();
                    self.dragged_item = DraggedItem::None;
                }
            }
        });

        (dragged_audio_info, is_released)
    }

    pub fn tab_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            self.tab_bar_button(ui, egui_phosphor::fill::FILES, LeftPanelTabs::Files);
            self.tab_bar_button(ui, egui_phosphor::fill::SPARKLE, LeftPanelTabs::Effects);
        });
    }

    pub fn tab_bar_button(&mut self, ui: &mut Ui, icon: &str, value: LeftPanelTabs) {
        let res = ui.add(Button::new(icon).selected(self.tab == value));

        if res.clicked() {
            self.tab = value;
        }
    }
}
