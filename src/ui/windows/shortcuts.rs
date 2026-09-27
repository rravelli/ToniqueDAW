use crate::{
    config::keymap::{Action, FIXED_SHORTCUTS, Keymap, reserved_by, shortcut_from_press},
    core::state::ToniqueProjectState,
    ui::{
        font::PHOSPHOR_REGULAR,
        theme::ThemeExt,
        widget::{section::SectionHeader, square_button::SquareButton},
    },
};
use egui::{
    EventFilter, FontFamily, FontId, Grid, Id, Key, KeyboardShortcut, Layout, RichText, ScrollArea,
    Sense, Ui, vec2,
};
use egui_phosphor::regular::{ARROW_COUNTER_CLOCKWISE, PLUS};

const GROUPS: [&str; 4] = ["File", "Transport", "Edit", "View"];
const LABEL_WIDTH: f32 = 200.;
const CHIP_HEIGHT: f32 = 20.;
const LIST_HEIGHT: f32 = 380.;

/// The binding waiting for a key press.
#[derive(Clone, Copy, PartialEq)]
struct Recording {
    action: Action,
    /// Binding being replaced; `None` adds one.
    index: Option<usize>,
}

/// Settings tab listing every shortcut. Clicking a binding records the next
/// key press in its place; changes apply immediately.
pub struct UIShortcutsTab {
    recording: Option<Recording>,
    /// Outcome of the last change, e.g. a shortcut moved from another action.
    message: Option<String>,
}

impl UIShortcutsTab {
    pub fn new() -> Self {
        Self {
            recording: None,
            message: None,
        }
    }

    /// Stop recording, e.g. when the window closes or changes tab.
    pub fn cancel(&mut self) {
        self.recording = None;
        self.message = None;
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let mut keymap = state.settings().keymap.clone();
        self.record(ui, &mut keymap);

        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for group in GROUPS {
                    ui.add(SectionHeader::new(group));
                    Grid::new(("settings-shortcuts", group))
                        .num_columns(2)
                        .min_col_width(LABEL_WIDTH)
                        .spacing([12., 4.])
                        .show(ui, |ui| {
                            for action in Action::ALL.into_iter().filter(|a| a.group() == group) {
                                ui.label(RichText::new(action.label()).color(ui.app_theme().text));
                                self.bindings(ui, &mut keymap, action);
                                ui.end_row();
                            }
                            for fixed in FIXED_SHORTCUTS.iter().filter(|f| f.group == group) {
                                ui.label(
                                    RichText::new(fixed.label).color(ui.app_theme().text_muted),
                                )
                                .on_hover_text("Handled by the system, can't be changed.");
                                ui.horizontal(|ui| {
                                    for shortcut in fixed.shortcuts {
                                        ui.add_enabled(
                                            false,
                                            chip(ui.ctx().format_shortcut(shortcut)),
                                        );
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.);
                }
            });

        let hint = if self.recording.is_some() {
            "Press the new shortcut, or Esc to cancel."
        } else {
            "Click a shortcut to change it, right-click to remove it."
        };
        ui.label(
            RichText::new(self.message.as_deref().unwrap_or(hint))
                .small()
                .color(ui.app_theme().text_muted),
        );
        ui.separator();
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            let reset_all = SquareButton::new("Reset all")
                .size(vec2(0., 22.))
                .padding(10.)
                .font(FontId::proportional(12.))
                .border_radius(2.);
            if ui
                .add_enabled(!keymap.is_all_default(), reset_all)
                .clicked()
            {
                keymap = Keymap::default();
                self.cancel();
            }
        });

        if keymap != state.settings().keymap {
            let mut settings = state.settings().clone();
            settings.keymap = keymap;
            state.apply_settings(settings);
        }
    }

    /// The bindings of `action` as chips, then add and reset buttons.
    fn bindings(&mut self, ui: &mut Ui, keymap: &mut Keymap, action: Action) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            for (i, shortcut) in keymap.bindings(action).iter().enumerate() {
                let recording = Recording {
                    action,
                    index: Some(i),
                };
                if self.recording == Some(recording) {
                    self.recorder(ui);
                    continue;
                }
                let response = ui
                    .add(chip(ui.ctx().format_shortcut(shortcut)))
                    .on_hover_text("Click to change, right-click to remove.");
                if response.clicked() {
                    self.start(ui, recording);
                } else if response.secondary_clicked() {
                    keymap.remove(action, i);
                    self.cancel();
                }
            }

            let adding = Recording {
                action,
                index: None,
            };
            if self.recording == Some(adding) {
                self.recorder(ui);
            } else if ui
                .add(icon_button(PLUS).tooltip("Add a shortcut"))
                .clicked()
            {
                self.start(ui, adding);
            }

            if !keymap.is_default(action)
                && ui
                    .add(icon_button(ARROW_COUNTER_CLOCKWISE).tooltip("Restore the default"))
                    .clicked()
            {
                keymap.reset(action);
                self.cancel();
            }
        });
    }

    fn start(&mut self, ui: &Ui, recording: Recording) {
        self.recording = Some(recording);
        self.message = None;
        // Holding focus keeps the timeline from acting on the key pressed.
        ui.memory_mut(|m| m.request_focus(recorder_id()));
    }

    /// Placeholder shown while recording. It holds keyboard focus, and
    /// recording stops if it loses it (e.g. a click elsewhere).
    fn recorder(&mut self, ui: &mut Ui) {
        let response = ui.add(chip("Press a key…").selected(true));
        let id = recorder_id();
        ui.interact(response.rect, id, Sense::focusable_noninteractive());
        // Let arrows, Tab and Esc reach us instead of moving focus.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
        let clicked_elsewhere = ui.input(|i| i.pointer.any_pressed()) && !response.hovered();
        if clicked_elsewhere || ui.memory(|m| m.focused() != Some(id)) {
            self.recording = None;
            ui.memory_mut(|m| m.surrender_focus(id));
        }
    }

    /// Bind the key pressed this frame, if recording.
    fn record(&mut self, ui: &mut Ui, keymap: &mut Keymap) {
        let Some(recording) = self.recording else {
            return;
        };
        let Some(shortcut) = ui.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some(shortcut_from_press(*key, *modifiers)),
                _ => None,
            })
        }) else {
            return;
        };

        if shortcut == KeyboardShortcut::new(egui::Modifiers::NONE, Key::Escape) {
            self.recording = None;
        } else if let Some(fixed) = reserved_by(&shortcut) {
            self.message = Some(format!(
                "{} is reserved for {}.",
                ui.ctx().format_shortcut(&shortcut),
                fixed.label
            ));
        } else {
            let previous = keymap.assign(recording.action, recording.index, shortcut);
            self.message = previous.map(|other| {
                format!(
                    "{} was removed from “{}”.",
                    ui.ctx().format_shortcut(&shortcut),
                    other.label()
                )
            });
            self.recording = None;
        }
        if self.recording.is_none() {
            ui.memory_mut(|m| m.surrender_focus(recorder_id()));
        }
    }
}

fn recorder_id() -> Id {
    Id::new("settings-shortcut-recorder")
}

fn chip(text: impl ToString) -> SquareButton {
    SquareButton::new(text)
        .size(vec2(0., CHIP_HEIGHT))
        .padding(8.)
        .font(FontId::proportional(12.))
        .border_radius(2.)
}

fn icon_button(icon: &str) -> SquareButton {
    SquareButton::ghost(icon)
        .square(CHIP_HEIGHT)
        .font(FontId::new(12., FontFamily::Name(PHOSPHOR_REGULAR.into())))
        .border_radius(2.)
}
