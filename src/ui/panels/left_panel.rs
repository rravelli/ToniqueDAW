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
use egui::{Frame, Margin, RichText, Ui};

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
    /// Each tab its own, so searching one leaves the other's as it was.
    file_search: SearchBar,
    effect_search: SearchBar,
}

impl LeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FileBrowser::new(),
            focused: false,
            tab: LeftPanelTab::Files,
            file_search: SearchBar::new("Search files"),
            effect_search: SearchBar::new("Search effects"),
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
                    match self.tab {
                        LeftPanelTab::Files => {
                            if let Some(query) = ui
                                .push_id("file-search", |ui| self.file_search.ui(ui))
                                .inner
                            {
                                self.file_browser.trigger_search(query);
                            }
                        }
                        // Filtered as typed, below.
                        LeftPanelTab::Effects => {
                            ui.push_id("effect-search", |ui| self.effect_search.ui(ui));
                        }
                    }
                });

            match self.tab {
                LeftPanelTab::Files => {
                    self.file_browser.ui(ui, state, commands, self.focused);
                }
                LeftPanelTab::Effects => {
                    let query = self.effect_search.query();
                    let found: Vec<_> = EffectKind::ALL
                        .into_iter()
                        .filter(|k| k.matches(query))
                        .collect();
                    if found.is_empty() {
                        ui.add_space(8.);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("No effects match")
                                    .size(11.)
                                    .color(ui.app_theme().text_muted),
                            );
                        });
                    }
                    for kind in found {
                        let icon = match kind {
                            EffectKind::Filter => egui_phosphor::fill::FUNNEL_SIMPLE,
                            EffectKind::Echo => egui_phosphor::fill::WAVES,
                            EffectKind::Spectrum => egui_phosphor::fill::CHART_LINE,
                            EffectKind::Utility => egui_phosphor::fill::SLIDERS,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tonique_engine::engine::{Engine, EngineConfig};

    /// On the effects tab, the bar searches effects, and leaves the files'
    /// search as it was.
    #[test]
    fn each_tab_has_its_own_search() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = LeftPanel::new();
        panel.tab = LeftPanelTab::Effects;
        let mut workspace = Workspace {
            left_panel_open: true,
            ..Default::default()
        };
        let mut frame = |panel: &mut LeftPanel, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200., 800.),
                )),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, &mut state, &mut workspace, &mut Commands::default())
            });
            output.textures_delta.clear();
        };
        frame(&mut panel, vec![]);
        // The bar, under the tabs.
        let at = egui::pos2(100., HEADER_SPACING + 25. + HEADER_SPACING + 11.);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(
            &mut panel,
            vec![egui::Event::PointerMoved(at), button(true), button(false)],
        );
        frame(&mut panel, vec![egui::Event::Text("delay".into())]);
        assert_eq!(panel.effect_search.query(), "delay");
        assert_eq!(panel.file_search.query(), "");

        panel.tab = LeftPanelTab::Files;
        frame(&mut panel, vec![]);
        assert_eq!(panel.effect_search.query(), "delay", "kept for later");
    }
}
