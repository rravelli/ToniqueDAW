use crate::{
    core::{effect::EffectKind, state::ProjectState},
    ui::{
        browser::FileBrowser,
        commands::Commands,
        dnd::DragPayload,
        theme::ThemeExt,
        widget::{list_row::ListRow, search_bar::SearchBar, tab_bar::TabBar},
        workspace::Workspace,
    },
};
use egui::{Frame, Margin, Ui};

/// Space around and between the tab bar and the search bar.
const HEADER_SPACING: f32 = 4.;

#[derive(Clone, Copy, PartialEq)]
pub enum LeftPanelTab {
    Files,
    Effects,
}

pub struct LeftPanel {
    pub file_browser: FileBrowser,
    /// The last click was in the panel: the browser has the keyboard.
    focused: bool,
    tab: LeftPanelTab,
    search: SearchBar,
}

impl LeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FileBrowser::new(),
            focused: false,
            tab: LeftPanelTab::Files,
            search: SearchBar::new("Search"),
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
    ) {
        let mut open = workspace.left_panel_open;
        egui::Panel::left("left-panel")
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
                    .fill(ui.app_theme().bg_panel)
                    .corner_radius(4.0),
            )
            .default_size(220.)
            .show_collapsible(ui, &mut open, |ui| {
                let panel = ui.max_rect();
                if let Some(pos) = ui
                    .input(|i| {
                        i.pointer
                            .primary_pressed()
                            .then(|| i.pointer.interact_pos())
                    })
                    .flatten()
                {
                    self.focused = panel.contains(pos);
                }
                self.ui(ui, state, commands);
            });
        workspace.left_panel_open = open;
        if !open {
            self.focused = false;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState, commands: &mut Commands) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            Frame::new()
                .inner_margin(Margin::symmetric(2, HEADER_SPACING as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = HEADER_SPACING;
                    ui.add(
                        TabBar::new(
                            &mut self.tab,
                            [
                                (LeftPanelTab::Files, "Files"),
                                (LeftPanelTab::Effects, "Effects"),
                            ],
                        )
                        .height(25.),
                    );
                    if let Some(query) = self.search.ui(ui) {
                        self.file_browser.trigger_search(query);
                    }
                });

            match self.tab {
                LeftPanelTab::Files => {
                    self.file_browser.ui(ui, state, commands, self.focused);
                }
                LeftPanelTab::Effects => {
                    for kind in EffectKind::ALL {
                        let icon = match kind {
                            EffectKind::Filter => egui_phosphor::fill::FUNNEL_SIMPLE,
                            EffectKind::Echo => egui_phosphor::fill::WAVES,
                        };
                        let res = ui
                            .add(ListRow::new(format!("{icon} {}", kind.name())))
                            .on_hover_text("Drag onto a track or the effects panel");
                        res.dnd_set_drag_payload(DragPayload::Effect(kind));
                    }
                }
            }
        });
    }
}
