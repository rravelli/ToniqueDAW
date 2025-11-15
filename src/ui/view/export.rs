use std::path::PathBuf;

use egui::{
    CentralPanel, Color32, Context, FontId, Label, Layout, ProgressBar, TextEdit, Ui,
    ViewportBuilder, ViewportId, Widget, vec2,
};
use egui_phosphor::fill::FOLDER;
use rfd::FileDialog;

use crate::{
    core::{export::ExportStatus, state::ToniqueProjectState},
    ui::{font::PHOSPHOR_FILL, theme::PRIMARY_COLOR, widget::square_button::SquareButton},
};

pub struct ExporWindow {
    name: String,
    folder: PathBuf,
}

impl ExporWindow {
    pub fn new() -> Self {
        Self {
            name: "Untitled.wav".into(),
            folder: PathBuf::new(),
        }
    }

    pub fn show(&mut self, ctx: &Context, state: &mut ToniqueProjectState) {
        ctx.show_viewport_immediate(
            ViewportId("EXPORT_VIEWPORT".into()),
            ViewportBuilder::default()
                .with_title("Export project")
                .with_inner_size(vec2(300., 400.))
                .with_resizable(false)
                .with_close_button(false)
                .with_window_type(egui::X11WindowType::Utility),
            |ctx, viewport_class| {
                if ctx.input(|r| r.viewport().close_requested()) {
                    state.show_export = false;
                }
                CentralPanel::default().show(ctx, |ui| {
                    self.ui(ui, state);
                });
            },
        );
    }

    pub fn ui(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.visuals_mut().selection.bg_fill = Color32::from_white_alpha(100);
        if let ExportStatus::PROCESSING(progress) = state.export_status() {
            ui.ctx().request_repaint();
            ui.label("Rendering audio");
            ProgressBar::new(*progress)
                .fill(PRIMARY_COLOR)
                .desired_height(20.)
                .corner_radius(2.0)
                .show_percentage()
                .ui(ui);
            return;
        }

        ui.horizontal(|ui| {
            ui.scope(|ui| {
                ui.set_width(45.);
                ui.label("Name");
            });
            TextEdit::singleline(&mut self.name).ui(ui);
        });

        ui.horizontal(|ui| {
            ui.scope(|ui| {
                ui.set_width(45.);
                ui.label("Path");
            });
            ui.label(self.folder.to_str().unwrap_or(""));
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(SquareButton::ghost(FOLDER).square(20.).font(FontId::new(
                        15.,
                        egui::FontFamily::Name(PHOSPHOR_FILL.into()),
                    )))
                    .clicked()
                {
                    let picked_dir = FileDialog::new().pick_folder();
                    if let Some(path) = picked_dir {
                        self.folder = path;
                    }
                };
            });
        });

        let enabled = self.folder.exists();

        if ui
            .add_enabled(
                enabled,
                SquareButton::new("Export")
                    .fill(PRIMARY_COLOR)
                    .size(vec2(70., 20.)),
            )
            .clicked()
        {
            state.export(self.folder.join(self.name.clone()));
        };
    }
}
