use crate::ui::theme::font_size;
use crate::{
    core::{effect::EffectKind, state::ProjectState},
    ui::{
        browser::{ADD_FOLDER_SIZE, FileBrowser},
        commands::Commands,
        dnd::DragPayload,
        theme::ThemeExt,
        widget::{icon_rail::IconRail, list_row::ListRow, search_bar::SearchBar},
        workspace::Workspace,
    },
};
use egui::{Frame, Margin, RichText, Stroke, Ui, UiBuilder, vec2};
use egui_phosphor::regular as icons;

/// Space around the search bar, and above the rail.
const HEADER_SPACING: f32 = 4.;
/// Side of the rail's buttons, and so its width.
const RAIL_WIDTH: f32 = 28.;
/// Gap between the rail's divider and the content.
const RAIL_GAP: f32 = 2.;

/// What the panel browses, picked on the rail.
#[derive(Clone, Copy, PartialEq)]
pub enum BrowserSource {
    Files,
    /// What's been given a label, by label.
    Labels,
    Effects,
}

impl BrowserSource {
    /// In the rail's order.
    pub const ALL: [Self; 3] = [Self::Files, Self::Labels, Self::Effects];

    pub fn name(self) -> &'static str {
        match self {
            Self::Files => "Files",
            Self::Labels => "Labels",
            Self::Effects => "Effects",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Files => icons::FOLDER_SIMPLE,
            Self::Labels => icons::TAG,
            Self::Effects => icons::MAGIC_WAND,
        }
    }
}

pub struct LeftPanel {
    pub file_browser: FileBrowser,
    /// The last click was in the panel: the browser has the keyboard.
    focused: bool,
    source: BrowserSource,
    /// Each source its own, so searching one leaves the others' as they were.
    file_search: SearchBar,
    label_search: SearchBar,
    effect_search: SearchBar,
}

impl LeftPanel {
    pub fn new() -> Self {
        Self {
            file_browser: FileBrowser::new(),
            focused: false,
            source: BrowserSource::Files,
            file_search: SearchBar::new("Search all folders"),
            label_search: SearchBar::new("Search labelled files"),
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
        let full = ui.available_rect_before_wrap();
        let (rail, content) = full.split_left_right_at_x(full.left() + RAIL_WIDTH);
        ui.scope_builder(UiBuilder::new().max_rect(rail), |ui| {
            ui.add_space(HEADER_SPACING);
            ui.add(
                IconRail::new(
                    &mut self.source,
                    BrowserSource::ALL.map(|s| (s, s.icon(), s.name())),
                )
                .size(RAIL_WIDTH),
            );
        });
        ui.painter().vline(
            rail.right() + 0.5,
            full.y_range(),
            Stroke::new(1., ui.app_theme().separator),
        );
        let content = content.with_min_x(content.left() + 1. + RAIL_GAP);
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            self.content_ui(ui, state, commands)
        });
    }

    fn content_ui(&mut self, ui: &mut Ui, state: &mut ProjectState, commands: &mut Commands) {
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            Frame::new()
                .inner_margin(Margin::symmetric(2, HEADER_SPACING as i8))
                .show(ui, |ui| match self.source {
                    // The search bar, and the button adding a folder.
                    BrowserSource::Files => {
                        self.file_search
                            .set_hint(match self.file_browser.search_scope() {
                                Some(folder) => format!("Search in {folder}"),
                                None => "Search all folders".into(),
                            });
                        self.file_search.set_busy(self.file_browser.is_searching());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.;
                            let width = ui.available_width() - ADD_FOLDER_SIZE - 2.;
                            let query = ui
                                .allocate_ui(vec2(width, ADD_FOLDER_SIZE), |ui| {
                                    ui.push_id("file-search", |ui| {
                                        self.file_search.ui(ui).map(str::to_owned)
                                    })
                                    .inner
                                })
                                .inner;
                            self.file_browser.add_folder_button(ui);
                            if let Some(query) = query {
                                self.file_browser.trigger_search(&query);
                            }
                        });
                    }
                    BrowserSource::Labels => {
                        self.label_search
                            .set_busy(self.file_browser.is_searching_labels());
                        let query = ui
                            .push_id("label-search", |ui| {
                                self.label_search.ui(ui).map(str::to_owned)
                            })
                            .inner;
                        if let Some(query) = query {
                            self.file_browser.trigger_label_search(&query);
                        }
                    }
                    // Filtered as typed, below.
                    BrowserSource::Effects => {
                        ui.push_id("effect-search", |ui| self.effect_search.ui(ui));
                    }
                });

            match self.source {
                BrowserSource::Files => {
                    self.file_browser.ui(ui, state, commands, self.focused);
                }
                BrowserSource::Labels => {
                    self.file_browser
                        .labels_ui(ui, state, commands, self.focused);
                }
                BrowserSource::Effects => {
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
                                    .size(font_size::LABEL)
                                    .color(ui.app_theme().text_muted),
                            );
                        });
                    }
                    // One icon for all, the tab's: they're told apart by name.
                    let icon = BrowserSource::Effects.icon();
                    for kind in found {
                        let res = ui
                            .add(ListRow::new(kind.name()).icon(icon))
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

    /// On the effects source, the bar searches effects, and leaves the files'
    /// search as it was.
    #[test]
    fn each_source_has_its_own_search() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = LeftPanel::new();
        panel.source = BrowserSource::Effects;
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
        // The bar, right of the rail.
        let at = egui::pos2(RAIL_WIDTH + 100., HEADER_SPACING + 11.);
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

        panel.source = BrowserSource::Files;
        frame(&mut panel, vec![]);
        assert_eq!(panel.effect_search.query(), "delay", "kept for later");
    }
}
