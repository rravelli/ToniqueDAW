//! What can be done with a folder, a label or a file, from its menu.

use crate::{
    config::labels as label_colors,
    ui::{
        theme::ThemeExt,
        widget::{context_menu::ContextMenuButton, text_edit::select_all},
    },
};
use egui::{Key, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular::{ARROW_SQUARE_OUT, ARROWS_IN_LINE_VERTICAL, MAGNIFYING_GLASS};
use std::path::Path;

use super::{FileList, FolderAction, LABEL_MENU_WIDTH};

impl FileList {
    /// What can be done with one of the browser's folders.
    pub(super) fn folder_menu(
        &self,
        ui: &mut Ui,
        path: &Path,
        scoped: bool,
        actions: &mut Vec<FolderAction>,
    ) {
        let search = if scoped {
            ("Search all folders", FolderAction::SearchEverywhere)
        } else {
            (
                "Search in this folder",
                FolderAction::SearchIn(path.to_path_buf()),
            )
        };
        for (icon, text, action) in [
            (MAGNIFYING_GLASS, search.0, search.1),
            (
                egui_phosphor::regular::ARROWS_CLOCKWISE,
                "Refresh (F5)",
                FolderAction::Refresh,
            ),
            (
                ARROWS_IN_LINE_VERTICAL,
                "Collapse all",
                FolderAction::CollapseAll(path.to_path_buf()),
            ),
            (
                ARROW_SQUARE_OUT,
                "Show in file manager",
                FolderAction::Reveal(path.to_path_buf()),
            ),
        ] {
            if ui.add(ContextMenuButton::new(icon, text)).clicked() {
                actions.push(action);
            }
        }
        self.label_submenu(ui, path, actions);
        ui.separator();
        if ui
            .add(
                ContextMenuButton::new(egui_phosphor::regular::FOLDER_MINUS, "Remove from browser")
                    .text_color(ui.app_theme().danger),
            )
            .clicked()
        {
            actions.push(FolderAction::Remove(path.to_path_buf()));
        }
    }

    /// What can be done with label `index`: renaming it (typed in the
    /// menu), recolouring it, taking it off everything.
    pub(super) fn section_menu(&self, ui: &mut Ui, index: usize, actions: &mut Vec<FolderAction>) {
        let Some(label) = self.labels.get(index) else {
            return;
        };
        // As narrow as other menus: the field and swatches would take all
        // the room they're given.
        ui.set_max_width(LABEL_MENU_WIDTH);
        let id = ui.id().with(("label-name", index));
        // Drawn every pass while open, so a gap means it was just opened:
        // start from the current name, selected, ready to be typed over.
        let pass = ui.ctx().cumulative_pass_nr();
        let last_pass = id.with("last-pass");
        let opened = ui
            .data(|d| d.get_temp::<u64>(last_pass))
            .is_none_or(|last| last + 1 < pass);
        ui.data_mut(|d| {
            d.insert_temp(last_pass, pass);
            if opened {
                d.remove::<String>(id);
            }
        });
        let mut name = ui
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| label.name.clone());
        let field = ui.add(
            egui::TextEdit::singleline(&mut name)
                .hint_text("Name")
                .desired_width(ui.available_width()),
        );
        if opened {
            field.request_focus();
            select_all(ui.ctx(), field.id, &name);
        }
        if field.changed() {
            ui.data_mut(|d| d.insert_temp(id, name.clone()));
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            ui.data_mut(|d| d.remove::<String>(id));
            if !name.trim().is_empty() {
                actions.push(FolderAction::RenameLabel(index, name.trim().to_string()));
            }
            ui.close();
        }
        // Its colour: one of a few.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(2., 2.);
            for hex in label_colors::COLORS {
                let Ok(color) = egui::Color32::from_hex(hex) else {
                    continue;
                };
                let (rect, swatch) = ui.allocate_exact_size(vec2(16., 16.), Sense::click());
                let swatch = swatch.on_hover_cursor(egui::CursorIcon::PointingHand);
                ui.painter().circle_filled(rect.center(), 6., color);
                if color == label.color || swatch.hovered() {
                    ui.painter().circle_stroke(
                        rect.center(),
                        7.5,
                        Stroke::new(1., ui.app_theme().text),
                    );
                }
                if swatch.clicked() {
                    actions.push(FolderAction::RecolorLabel(index, hex.to_string()));
                }
            }
        });
        ui.separator();
        if ui
            .add(
                ContextMenuButton::new(egui_phosphor::regular::ERASER, "Clear label")
                    .text_color(ui.app_theme().danger),
            )
            .clicked()
        {
            actions.push(FolderAction::ClearLabel(index));
        }
    }

    /// "Label": a submenu of the labels, to give `path` one (shortcuts 1
    /// to 7) or none (0).
    pub(super) fn label_submenu(&self, ui: &mut Ui, path: &Path, actions: &mut Vec<FolderAction>) {
        let current = self.item_labels.get(path).copied();
        ContextMenuButton::new(egui_phosphor::regular::TAG, "Label").submenu(ui, |ui| {
            for (index, label) in self.labels.iter().enumerate() {
                let chosen = current == Some(index);
                let item = ContextMenuButton::swatch(label.color, &label.name).checked(chosen);
                if ui.add(item).clicked() {
                    // Its own label again: taken off.
                    let label = (!chosen).then_some(index);
                    actions.push(FolderAction::SetLabel(path.to_path_buf(), label));
                    ui.close();
                }
            }
            ui.separator();
            if ui
                .add(ContextMenuButton::no_swatch("No label").checked(current.is_none()))
                .clicked()
            {
                actions.push(FolderAction::SetLabel(path.to_path_buf(), None));
                ui.close();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::browser::items::LabelView;

    /// Opened, a label's menu has its name selected: typing replaces it.
    /// What was typed last time it was open is gone.
    #[test]
    fn a_labels_menu_starts_with_its_name_selected() {
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        crate::ui::theme::Theme::default().install(&ctx);
        let mut list = FileList::new();
        list.labels = vec![LabelView {
            name: "Drums".into(),
            color: egui::Color32::RED,
        }];
        let frame = |open: bool, events: Vec<egui::Event>| {
            let mut actions = Vec::new();
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                if open {
                    list.section_menu(ui, 0, &mut actions);
                }
            });
            output.textures_delta.clear();
            actions
        };
        let enter = || egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        let renamed = |actions: Vec<FolderAction>| {
            actions.into_iter().find_map(|action| match action {
                FolderAction::RenameLabel(0, name) => Some(name),
                _ => None,
            })
        };

        frame(true, vec![]);
        frame(true, vec![egui::Event::Text("Lost".into())]);
        frame(false, vec![]);
        frame(false, vec![]);

        frame(true, vec![]);
        frame(true, vec![egui::Event::Text("Bass".into())]);
        let actions = frame(true, vec![enter()]);
        assert_eq!(renamed(actions).as_deref(), Some("Bass"));
    }
}
