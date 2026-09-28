use crate::{
    config::Config,
    core::state::{PlaybackState, ProjectState},
    ui::{
        browser::{items::FileList, preview::PreviewPanel},
        font::PHOSPHOR_FILL,
        theme::ThemeExt,
        widget::{context_menu::ContextMenuButton, flat_button::FlatButton},
    },
};
use egui::{Button, FontId, RichText, ScrollArea, Ui, Widget};
use egui_phosphor::{
    fill::{FOLDER_PLUS, TRASH},
    regular::FOLDER,
};
use rfd::FileDialog;
use std::{ffi::OsStr, path::PathBuf};

mod file_tree;
mod items;
mod preview;

const PREVIEW_WINDOW_HEIGHT: f32 = 60.;

pub struct FileBrowser {
    root: Option<PathBuf>,
    preview: PreviewPanel,
    items: FileList,
    config: Config,
}

impl FileBrowser {
    pub fn new() -> Self {
        let config = Config::load();
        let mut items = FileList::new();
        let dirs = config.list_dirs();
        let root = if !dirs.is_empty() {
            items.init(dirs[0].clone());
            Some(dirs[0].clone())
        } else {
            None
        };

        Self {
            root,
            preview: PreviewPanel::new(),
            items,
            config,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ProjectState) {
        ui.spacing_mut().scroll.bar_width = 5.0;
        ScrollArea::horizontal().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                self.choose_dir_button(ui);
                let paths = self.config.list_dirs();
                for path in paths {
                    let name = path.file_name().unwrap_or(OsStr::new("")).to_string_lossy();

                    let res = Button::new(RichText::new(format!("{} {}", FOLDER, name)).size(10.))
                        .selected(self.root.as_ref().is_some_and(|f| *f == path))
                        .ui(ui);

                    if res.clicked() {
                        self.set_dir(path.clone());
                        res.scroll_to_me(None);
                    };

                    res.context_menu(|ui| {
                        if ui
                            .add(
                                ContextMenuButton::new(TRASH, "Delete")
                                    .text_color(ui.app_theme().danger),
                            )
                            .clicked()
                        {
                            self.config.remove_dir(&path);
                        }
                    });
                }
            });
            ui.add_space(5.0);
        });

        ui.add_space(5.0);

        if ui.response().clicked_elsewhere() {
            self.items.selected_audio = None;
        }

        let height = if self.items.selected_audio.is_some() {
            ui.available_height() - PREVIEW_WINDOW_HEIGHT
        } else {
            ui.available_height()
        };
        ui.vertical(|ui| {
            ui.set_height(height);

            self.items.ui(ui, state);
        });

        if let Some(audio) = &self.items.selected_audio {
            self.preview.ui(ui, state, audio);
        }

        if state.preview_playback_state() == PlaybackState::Playing
            && self.items.selected_audio.is_none()
        {
            state.pause_preview();
        }
    }

    fn set_dir(&mut self, dir: PathBuf) {
        let root = dir;
        self.root = Some(root.clone());
        self.items.init(root);
    }

    fn choose_dir_button(&mut self, ui: &mut Ui) {
        if ui
            .add(FlatButton::ghost(FOLDER_PLUS).square(20.).font(FontId::new(
                15.,
                egui::FontFamily::Name(PHOSPHOR_FILL.into()),
            )))
            .clicked()
        {
            let picked_dir = FileDialog::new().pick_folder();
            if let Some(path) = picked_dir {
                self.config.add_dir(path.clone());
                self.set_dir(path);
            }
        };
    }

    pub fn trigger_search(&mut self, query: &str) {
        let Some(root) = self.root.clone() else {
            return;
        };
        if query.is_empty() {
            self.items.clear_search(root);
        } else {
            self.items.search(query, root);
        }
    }
}
