//! The list's rows: the sections' headers, and their files and folders.

use crate::ui::theme::font_size;
use crate::{
    cache::AUDIO_ANALYSIS_CACHE,
    core::state::ProjectState,
    ui::{
        browser::file_tree::FileNode,
        dnd::DragPayload,
        font::PHOSPHOR_REGULAR,
        theme::{ThemeExt, with_alpha},
        widget::{
            context_menu::ContextMenuButton,
            flat_button::FlatButton,
            list_row::{ListRow, SLOT, dot_center},
        },
    },
};
use egui::{Align2, FontId, Key, Popup, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular::{ARROW_SQUARE_OUT, DOTS_THREE, MAGNIFYING_GLASS, X};
use std::ffi::OsStr;

use super::{FileList, FolderAction, PLAYABLE_FORMAT, ROW_HEIGHT};

impl FileList {
    /// A section's header, a band across the list to set it apart from the
    /// rows under it: one of the browser's folders (its name in small
    /// capitals, its path on hover) or a label's (its colour, name and how
    /// many items it has), with a menu of what can be done with it on its
    /// right while hovered, or on a right-click. Returns whether it was
    /// clicked, to open or close.
    pub(super) fn root_ui(
        &mut self,
        ui: &mut Ui,
        index: usize,
        file: &FileNode,
        open: bool,
        actions: &mut Vec<FolderAction>,
    ) -> bool {
        let theme = ui.app_theme();
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let selected = self.selected == Some(index);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., theme.bg_raised);
        if selected {
            painter.rect_filled(rect, 0., with_alpha(theme.accent, 60));
        } else if response.hovered() {
            painter.rect_filled(rect, 0., theme.hover_overlay);
        }
        // Between sections.
        if index > 0 {
            painter.hline(
                rect.x_range(),
                rect.top() + 0.5,
                Stroke::new(1., theme.separator),
            );
        }
        let caret = if open {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };
        let x = rect.left() + 6.;
        painter.text(
            pos2(x + SLOT / 2., rect.center().y),
            Align2::CENTER_CENTER,
            caret,
            FontId::proportional(12.),
            theme.text,
        );
        let section = self.sections.get(&file.path).copied();
        let menu_width = ROW_HEIGHT;
        // The folder searched in alone: a magnifier before its name, and a
        // button to search them all again before its menu.
        let scoped = self.scope.as_ref() == Some(&file.path);
        let mut name_x = x + SLOT;
        if let Some((label, _)) = section
            && let Some(label) = self.labels.get(label)
        {
            painter.circle_filled(pos2(name_x + 4., rect.center().y), 4., label.color);
            name_x += 12.;
        }
        if scoped {
            painter.text(
                pos2(name_x + SLOT / 2., rect.center().y),
                Align2::CENTER_CENTER,
                MAGNIFYING_GLASS,
                FontId::proportional(11.),
                theme.accent,
            );
            name_x += SLOT;
        }
        let name = match section {
            Some((label, _)) => self.labels.get(label).map(|l| l.name.clone()),
            None => self.names.get(&file.path).cloned(),
        }
        .unwrap_or_default()
        .to_uppercase();
        // A label's count, right.
        let count = section.map(|(_, count)| {
            painter.layout_no_wrap(
                count.to_string(),
                FontId::proportional(font_size::SMALL),
                theme.text_disabled,
            )
        });
        let buttons = if scoped { 2. } else { 1. } * menu_width;
        let count_width = count.as_ref().map_or(0., |c| c.size().x + 6.);
        // A labelled folder: a dot after its name.
        let folder_label = section
            .is_none()
            .then(|| self.item_labels.get(&file.path))
            .flatten()
            .and_then(|&l| self.labels.get(l));
        // The menu's button, while the header's hovered or its menu open;
        // the dot's place otherwise.
        let button_rect = Rect::from_min_size(
            pos2(rect.right() - menu_width - 2., rect.top()),
            vec2(menu_width, ROW_HEIGHT),
        );
        let menu_id = ui.id().with(("folder-menu", &file.path));
        let menu_open = Popup::is_id_open(ui.ctx(), menu_id);
        let show_button = response.hovered() || menu_open || ui.rect_contains_pointer(button_rect);
        let dot_width = if folder_label.is_some() { 12. } else { 0. };
        let galley = egui::WidgetText::from(RichText::new(name).strong()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (rect.right() - name_x - buttons - count_width - dot_width - 4.).max(0.),
            FontId::proportional(font_size::SMALL),
        );
        let empty = section.is_some_and(|(_, count)| count == 0);
        painter.galley(
            pos2(name_x, rect.center().y - galley.size().y / 2.),
            galley,
            if empty {
                theme.text_disabled
            } else {
                theme.text
            },
        );
        if let Some(label) = folder_label
            && !show_button
        {
            painter.circle_filled(dot_center(rect), 3., label.color);
        }
        if let Some(count) = count {
            let pos = pos2(
                rect.right() - buttons - 4. - count.size().x,
                rect.center().y - count.size().y / 2.,
            );
            painter.galley(pos, count, theme.text_disabled);
        }

        let path = file.path.clone();
        let menu = |ui: &mut Ui, actions: &mut Vec<FolderAction>| match section {
            Some((label, _)) => self.section_menu(ui, label, actions),
            None => self.folder_menu(ui, &path, scoped, actions),
        };
        let mut over_button = false;
        if show_button {
            let button = ui.put(
                button_rect,
                FlatButton::ghost(DOTS_THREE)
                    .square(menu_width)
                    .font(FontId::new(
                        12.,
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                    )),
            );
            over_button = button.hovered();
            Popup::menu(&button)
                .id(menu_id)
                .show(|ui| menu(ui, actions));
        }
        if scoped {
            let clear = ui.put(
                button_rect.translate(vec2(-menu_width, 0.)),
                FlatButton::ghost(X)
                    .square(menu_width)
                    .font(FontId::new(
                        11.,
                        egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                    ))
                    .tooltip("Search all folders"),
            );
            over_button |= clear.hovered();
            if clear.clicked() {
                actions.push(FolderAction::SearchEverywhere);
            }
        }
        response.context_menu(|ui| menu(ui, actions));
        let pressed = !over_button
            && (response.clicked()
                || (selected && self.focused && ui.input(|i| i.key_pressed(Key::Enter))));
        if pressed {
            self.selected = Some(index);
        }
        pressed
    }

    /// `pinned`: drawn pinned at the top, over the list.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn item_ui(
        &mut self,
        ui: &mut Ui,
        index: usize,
        file: &FileNode,
        open: bool,
        state: &mut ProjectState,
        actions: &mut Vec<FolderAction>,
        pinned: bool,
    ) -> bool {
        let is_dir = file.is_dir;
        let selected = self.selected.is_some_and(|idx| idx == index);
        let extension = file.path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_audio = PLAYABLE_FORMAT.contains(&extension);

        let (caret, icon) = match (is_dir, open) {
            (true, true) => (
                Some(egui_phosphor::regular::CARET_DOWN),
                egui_phosphor::regular::FOLDER_OPEN,
            ),
            (true, false) => (
                Some(egui_phosphor::regular::CARET_RIGHT),
                egui_phosphor::regular::FOLDER,
            ),
            (false, _) if is_audio => (None, egui_phosphor::regular::FILE_AUDIO),
            (false, _) => (None, egui_phosphor::regular::FILE),
        };
        // The file the preview plays: a speaker, in the accent.
        let playing = state.previewing() == Some(file.path.as_path());
        let icon = if playing {
            egui_phosphor::regular::SPEAKER_HIGH
        } else {
            icon
        };

        let name = file
            .path
            .file_name()
            .unwrap_or(OsStr::new(""))
            .to_string_lossy();

        let mut row = ListRow::new(name)
            .indent((file.depth - 1) as f32 * SLOT)
            .guides(file.depth - 1)
            .caret(caret)
            .icon(icon)
            .dot(
                self.item_labels
                    .get(&file.path)
                    .and_then(|&l| self.labels.get(l))
                    .map(|l| l.color),
            )
            .selected(selected);
        if playing {
            row = row.icon_color(ui.app_theme().accent);
        }
        let res = ui.add(row);
        // What an item can be given, a right-click away.
        let in_section = file.depth == 1 && !self.sections.is_empty();
        res.context_menu(|ui| {
            self.label_submenu(ui, &file.path, actions);
            if is_dir
                && !in_section
                && ui
                    .add(ContextMenuButton::new(
                        egui_phosphor::regular::FOLDER_PLUS,
                        "Add as browser folder",
                    ))
                    .clicked()
            {
                actions.push(FolderAction::Add(file.path.clone()));
            }
            let shown = if is_dir {
                Some(file.path.as_path())
            } else {
                file.path.parent()
            };
            if let Some(shown) = shown
                && ui
                    .add(ContextMenuButton::new(
                        ARROW_SQUARE_OUT,
                        "Show in file manager",
                    ))
                    .clicked()
            {
                actions.push(FolderAction::Reveal(shown.to_path_buf()));
            }
        });

        let entered = selected && self.focused && ui.input(|i| i.key_pressed(Key::Enter));
        let pressed = res.clicked() || entered;

        if pressed {
            self.selected = Some(index);
        }

        if is_audio && pressed {
            self.selected_audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone());
            if entered {
                // Enter plays or stops it, whether or not selecting plays.
                if state.previewing() == Some(file.path.as_path()) {
                    state.pause_preview();
                } else {
                    state.play_preview(file.path.clone());
                }
            } else {
                self.preview(state, file.path.clone());
            }
        }

        let toggle_folder = is_dir && pressed;

        if res.dragged()
            && is_audio
            && let Some(audio_info) = AUDIO_ANALYSIS_CACHE.get_or_analyze(file.path.clone())
        {
            // Drawn on the timeline while dragged.
            audio_info.load_peaks();
            res.dnd_set_drag_payload(DragPayload::File(audio_info));
        }

        // A pinned row is in view already: scrolling to it would drag the
        // list along.
        if selected && !pinned {
            ui.scroll_to_rect(
                Rect::from_min_max(
                    pos2(res.rect.left(), res.rect.top() - 20.),
                    pos2(res.rect.right(), res.rect.bottom() + 20.),
                ),
                None,
            );
        }
        toggle_folder
    }
}
