use crate::ui::theme::font_size;
use crate::{
    config::{Config, keymap::Action},
    core::state::{PlaybackState, ProjectState},
    ui::{
        browser::{
            items::{FileList, FolderAction, LabelView},
            preview::{PREVIEW_HEIGHT, PreviewSettings, preview_ui},
        },
        commands::Commands,
        font::PHOSPHOR_FILL,
        theme::{ThemeExt, with_alpha},
        widget::flat_button::FlatButton,
    },
};
use egui::{Color32, FontId, Rect, RichText, Stroke, Ui};
use egui_phosphor::fill::FOLDER_PLUS;
use rfd::FileDialog;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

mod file_tree;
mod items;
mod preview;

/// Side of the button adding a folder.
pub const ADD_FOLDER_SIZE: f32 = 22.;

/// The files of the folders added to it, each a section of one tree,
/// searchable together or one at a time; the files and folders given a
/// label, by label, in a tab of their own; and a preview of the audio file
/// selected.
pub struct FileBrowser {
    items: FileList,
    /// The labels' tab.
    labelled: FileList,
    /// Its sections closed.
    labels_collapsed: Vec<PathBuf>,
    config: Config,
    preview: PreviewSettings,
    /// The preview's volume was handed to the engine.
    gain_sent: bool,
    /// The app's window had the focus last frame: getting it back, files
    /// may have changed meanwhile.
    window_focused: bool,
}

impl FileBrowser {
    pub fn new() -> Self {
        let config = Config::load();
        let mut items = FileList::new();
        items.set_roots(config.list_dirs(), config.collapsed_dirs());
        items.auto_preview = config.auto_preview();
        let mut labelled = FileList::new();
        labelled.auto_preview = config.auto_preview();
        let preview = PreviewSettings {
            auto_preview: config.auto_preview(),
            volume: config.preview_volume(),
            muted: false,
        };
        let mut browser = Self {
            items,
            labelled,
            labels_collapsed: Vec::new(),
            config,
            preview,
            gain_sent: false,
            window_focused: true,
        };
        browser.refresh_labels();
        browser
    }

    /// Read the folders from disk again when the app's window gets the
    /// focus back (files are often added from elsewhere meanwhile), or on
    /// F5 while the browser has the keyboard.
    fn refresh_when_due(&mut self, ui: &Ui, focused: bool) {
        let window_focused = ui.input(|i| i.focused);
        let regained = window_focused && !self.window_focused;
        self.window_focused = window_focused;
        let asked = focused
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F5));
        if regained || asked {
            self.refresh();
        }
    }

    /// Read both tabs' folders from disk again; labelled files gone are
    /// left out.
    fn refresh(&mut self) {
        self.items.refresh();
        self.refresh_labels();
        self.labelled.refresh();
    }

    /// Show the labels as configured, in both tabs.
    fn refresh_labels(&mut self) {
        let labels = self.config.labels();
        let views: Vec<LabelView> = labels
            .all()
            .iter()
            .map(|label| LabelView {
                name: label.name.clone(),
                color: Color32::from_hex(&label.color).unwrap_or(Color32::GRAY),
            })
            .collect();
        let assigned: HashMap<PathBuf, usize> = labels
            .assigned()
            .map(|(path, label)| (path.clone(), label))
            .collect();
        let sections = (0..views.len()).map(|i| (i, labels.items(i))).collect();
        self.items.set_labels(views.clone(), assigned.clone());
        self.labelled.set_labels(views, assigned);
        self.labelled.set_sections(sections, &self.labels_collapsed);
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
        self.refresh_when_due(ui, focused);
        ui.spacing_mut().scroll.bar_width = 5.0;
        let area = ui.available_rect_before_wrap();
        self.folder_drop(ui, area);

        if self.config.list_dirs().is_empty() {
            self.empty_ui(ui);
            return;
        }

        let mut preview = self.preview;
        let actions = list_ui(&mut self.items, ui, state, focused, &moves, &mut preview);
        self.set_preview(ui, state, preview);
        for action in actions {
            self.act(action);
        }
        // Folders opened or closed: remembered for next time.
        if let Some(collapsed) = self.items.collapsed_roots() {
            self.config.set_collapsed_dirs(collapsed);
        }
    }

    /// The labels' tab: a section for each label, with what has it.
    pub fn labels_ui(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        commands: &mut Commands,
        focused: bool,
    ) {
        let moves = take_moves(commands, focused);
        self.refresh_when_due(ui, focused);
        ui.spacing_mut().scroll.bar_width = 5.0;
        let mut preview = self.preview;
        let actions = list_ui(&mut self.labelled, ui, state, focused, &moves, &mut preview);
        self.set_preview(ui, state, preview);
        for action in actions {
            self.act(action);
        }
        if let Some(collapsed) = self.labelled.collapsed_roots() {
            self.labels_collapsed = collapsed;
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
            FolderAction::Refresh => self.refresh(),
            FolderAction::Add(dir) => self.add_folder(dir),
            FolderAction::SetLabel(path, label) => {
                self.config.edit_labels(|labels| labels.set(path, label));
                self.refresh_labels();
            }
            FolderAction::RenameLabel(index, name) => {
                self.config.edit_labels(|labels| labels.rename(index, name));
                self.refresh_labels();
            }
            FolderAction::RecolorLabel(index, color) => {
                self.config
                    .edit_labels(|labels| labels.recolor(index, color));
                self.refresh_labels();
            }
            FolderAction::ClearLabel(index) => {
                self.config.edit_labels(|labels| labels.clear(index));
                self.refresh_labels();
            }
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

    /// Keep what the preview changed, and hand its volume to the engine.
    fn set_preview(&mut self, ui: &Ui, state: &mut ProjectState, preview: PreviewSettings) {
        if preview.auto_preview != self.preview.auto_preview {
            self.items.auto_preview = preview.auto_preview;
            self.labelled.auto_preview = preview.auto_preview;
            self.config.set_auto_preview(preview.auto_preview);
        }
        if preview.gain() != self.preview.gain() || !self.gain_sent {
            state.set_preview_gain(preview.gain());
            self.gain_sent = true;
        }
        self.preview = preview;
        // Saved once the slider is let go, not on every step of a drag.
        let held = ui.input(|i| i.pointer.any_down());
        if !held && self.config.preview_volume() != preview.volume {
            self.config.set_preview_volume(preview.volume);
        }
    }

    fn pick_folder(&mut self) {
        // Tests get no dialog: it would wait for a click.
        if cfg!(test) {
            return;
        }
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
        search(&mut self.items, query);
    }

    pub fn is_searching_labels(&self) -> bool {
        self.labelled.is_searching()
    }

    /// Search what has a label.
    pub fn trigger_label_search(&mut self, query: &str) {
        search(&mut self.labelled, query);
    }
}

fn search(list: &mut FileList, query: &str) {
    if query.is_empty() {
        list.clear_search();
    } else {
        list.search(query);
    }
}

/// `list`, and under it a preview of its file selected. Returns what was
/// asked of its sections.
fn list_ui(
    list: &mut FileList,
    ui: &mut Ui,
    state: &mut ProjectState,
    focused: bool,
    moves: &[Action],
    preview: &mut PreviewSettings,
) -> Vec<FolderAction> {
    if ui.response().clicked_elsewhere() {
        list.selected_audio = None;
    }
    // A short browser (a small window, a tall effects panel) leaves the
    // list no room under the preview.
    let height = if list.selected_audio.is_some() {
        (ui.available_height() - PREVIEW_HEIGHT).max(0.)
    } else {
        ui.available_height()
    };
    let actions = ui
        .vertical(|ui| {
            ui.set_height(height);
            list.ui(ui, state, focused, moves)
        })
        .inner;
    if let Some(audio) = &list.selected_audio {
        preview_ui(ui, state, audio, preview);
    }
    if state.preview_playback_state() == PlaybackState::Playing && list.selected_audio.is_none() {
        state.pause_preview();
    }
    actions
}

/// Open `dir` in the system's file manager.
fn reveal(dir: &Path) {
    if cfg!(test) {
        return;
    }
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
