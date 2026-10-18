use crate::ui::theme::font_size;
use crate::{
    config::{Config, keymap::Action},
    core::state::{PlaybackState, ProjectState},
    ui::{
        browser::{
            items::{FileList, FolderAction},
            preview::{PREVIEW_HEIGHT, preview_ui},
        },
        commands::Commands,
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::{ThemeExt, with_alpha},
        widget::flat_button::FlatButton,
    },
};
use egui::{FontId, Rect, RichText, Stroke, Ui};
use egui_phosphor::{fill::FOLDER_PLUS, regular::HEADPHONES};
use rfd::FileDialog;
use std::path::{Path, PathBuf};

mod file_tree;
mod items;
mod preview;

/// Side of the button adding a folder.
pub const ADD_FOLDER_SIZE: f32 = 22.;

/// The files of the folders added to it, each a section of one tree,
/// searchable together or one at a time; and a preview of the audio file
/// selected.
pub struct FileBrowser {
    items: FileList,
    config: Config,
}

impl FileBrowser {
    pub fn new() -> Self {
        let config = Config::load();
        let mut items = FileList::new();
        items.set_roots(config.list_dirs(), config.collapsed_dirs());
        items.auto_preview = config.auto_preview();
        Self { items, config }
    }

    /// `focused`: the browser has the keyboard, and takes the selection
    /// moves (up and down) from `commands`.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        commands: &mut Commands,
        focused: bool,
    ) {
        let moves = take_moves(commands, focused);
        ui.spacing_mut().scroll.bar_width = 5.0;
        let area = ui.available_rect_before_wrap();
        self.folder_drop(ui, area);

        if self.config.list_dirs().is_empty() {
            self.empty_ui(ui);
            return;
        }

        if ui.response().clicked_elsewhere() {
            self.items.selected_audio = None;
        }

        let height = if self.items.selected_audio.is_some() {
            ui.available_height() - PREVIEW_HEIGHT
        } else {
            ui.available_height()
        };
        let actions = ui
            .vertical(|ui| {
                ui.set_height(height);
                self.items.ui(ui, state, focused, &moves)
            })
            .inner;
        for action in actions {
            self.act(action);
        }
        // Folders opened or closed: remembered for next time.
        if let Some(collapsed) = self.items.collapsed_roots() {
            self.config.set_collapsed_dirs(collapsed);
        }

        if let Some(audio) = &self.items.selected_audio {
            preview_ui(ui, state, audio);
        }

        if state.preview_playback_state() == PlaybackState::Playing
            && self.items.selected_audio.is_none()
        {
            state.pause_preview();
        }
    }

    /// Folders dragged in from the system's file manager are added; while
    /// they're over the browser, it says so.
    fn folder_drop(&mut self, ui: &Ui, area: Rect) {
        let over = ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|pos| area.contains(pos));
        if !over {
            return;
        }
        let (hovered, dropped) =
            ui.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        for file in dropped {
            if file.path().is_dir() {
                self.add_folder(file.path().to_path_buf());
            }
        }
        if hovered {
            let theme = ui.app_theme();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                ui.id().with("folder-drop"),
            ));
            painter.rect(
                area,
                3.,
                with_alpha(theme.accent, 25),
                Stroke::new(1., theme.accent),
                egui::StrokeKind::Inside,
            );
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Drop folders to add them",
                FontId::proportional(font_size::LABEL),
                theme.text,
            );
        }
    }

    /// No folder yet: what the browser is for, and a button to add one.
    fn empty_ui(&mut self, ui: &mut Ui) {
        let theme = ui.app_theme();
        ui.add_space(24.);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Your sample folders show here")
                    .size(font_size::LABEL)
                    .color(theme.text_muted),
            );
            ui.label(
                RichText::new("Drop one here, or")
                    .size(font_size::SMALL)
                    .color(theme.text_disabled),
            );
            ui.add_space(6.);
            if ui.add(FlatButton::new("Add a folder")).clicked() {
                self.pick_folder();
            }
        });
    }

    fn act(&mut self, action: FolderAction) {
        match action {
            FolderAction::Remove(dir) => {
                self.config.remove_dir(&dir);
                self.reload();
            }
            FolderAction::Reveal(dir) => reveal(&dir),
            FolderAction::CollapseAll(dir) => self.items.collapse_all(&dir),
            FolderAction::SearchIn(dir) => self.items.set_scope(Some(dir)),
            FolderAction::SearchEverywhere => self.items.set_scope(None),
            FolderAction::Add(dir) => self.add_folder(dir),
        }
    }

    /// Show the folders as configured.
    fn reload(&mut self) {
        self.items
            .set_roots(self.config.list_dirs(), self.config.collapsed_dirs());
    }

    /// The button adding a folder, beside the search bar.
    pub fn add_folder_button(&mut self, ui: &mut Ui) {
        let button = FlatButton::ghost(FOLDER_PLUS)
            .square(ADD_FOLDER_SIZE)
            .font(FontId::new(
                15.,
                egui::FontFamily::Name(PHOSPHOR_FILL.into()),
            ))
            .tooltip("Add a folder");
        if ui.add(button).clicked() {
            self.pick_folder();
        }
    }

    /// Whether selecting a file plays it: a switch beside the search bar.
    pub fn preview_switch(&mut self, ui: &mut Ui) {
        let on = self.items.auto_preview;
        let button = FlatButton::ghost(HEADPHONES)
            .square(ADD_FOLDER_SIZE)
            .selected(on)
            .font(FontId::new(
                14.,
                egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
            ))
            .tooltip(if on {
                "Preview on: selecting a file plays it"
            } else {
                "Preview off: play a file from its preview"
            });
        if ui.add(button).clicked() {
            self.items.auto_preview = !on;
            self.config.set_auto_preview(!on);
        }
    }

    fn pick_folder(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.add_folder(path);
        }
    }

    /// Add `dir` last, open.
    fn add_folder(&mut self, dir: PathBuf) {
        self.config.add_dir(dir);
        self.reload();
    }

    /// The name of the folder the search is kept to, if it is.
    pub fn search_scope(&self) -> Option<String> {
        self.items.scope_name()
    }

    pub fn is_searching(&self) -> bool {
        self.items.is_searching()
    }

    pub fn trigger_search(&mut self, query: &str) {
        if query.is_empty() {
            self.items.clear_search();
        } else {
            self.items.search(query);
        }
    }
}

/// Open `dir` in the system's file manager.
fn reveal(dir: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        eprintln!("Couldn't open {}: {e}", dir.display());
    }
}

/// The selection moves (up and down, bound to the arrow keys by default)
/// the browser takes while it has the keyboard; otherwise they're left for
/// the timeline, where they move clips between tracks.
fn take_moves(commands: &mut Commands, focused: bool) -> Vec<Action> {
    if !focused {
        return Vec::new();
    }
    commands.take(|a| matches!(a, Action::MoveTrackUp | Action::MoveTrackDown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_go_to_the_browser_only_while_focused() {
        let queue = || {
            let mut commands = Commands::default();
            for action in [Action::MoveTrackDown, Action::Undo, Action::MoveTrackUp] {
                commands.push(action);
            }
            commands
        };
        let mut commands = queue();
        assert!(take_moves(&mut commands, false).is_empty());
        assert_eq!(commands.take_all().len(), 3);

        let mut commands = queue();
        assert_eq!(
            take_moves(&mut commands, true),
            [Action::MoveTrackDown, Action::MoveTrackUp]
        );
        assert_eq!(commands.take_all(), [Action::Undo]);
    }
}
