use crate::ui::{
    theme::{ThemeExt, font_size},
    widget::{dialog, logo::logo},
};
use egui::{RichText, Ui, vec2};

const LOGO_SIZE: f32 = 72.;

/// Third-party work shipped with the app, credited as their licences ask.
const CREDITS: &[(&str, &str)] = &[
    (
        "Inter typeface",
        "The Inter Project Authors, SIL Open Font License 1.1",
    ),
    ("Phosphor Icons", "Phosphor Icons, MIT License"),
];

/// The app's name, version and credits, opened from the Help menu.
#[derive(Default)]
pub struct AboutWindow {
    open: bool,
}

impl AboutWindow {
    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn show(&mut self, ui: &Ui) {
        if !self.open {
            return;
        }
        let theme = ui.app_theme();
        let mut close = false;
        let modal = dialog::modal(ui, "about").show(ui.ctx(), |ui| {
            ui.set_width(300.);
            ui.vertical_centered(|ui| {
                ui.add(logo().fit_to_exact_size(vec2(LOGO_SIZE, LOGO_SIZE)));
                ui.add_space(8.);
                dialog::title(ui, "Tonique");
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .color(theme.text_muted),
                );
                ui.add_space(12.);
                for (work, credit) in CREDITS {
                    ui.label(RichText::new(*work).size(font_size::LABEL));
                    ui.label(
                        RichText::new(*credit)
                            .size(font_size::SMALL)
                            .color(theme.text_muted),
                    );
                    ui.add_space(4.);
                }
                ui.add_space(8.);
                close = ui.add(dialog::button("Close").selected(true)).clicked();
            });
        });
        if close || modal.should_close() {
            self.open = false;
        }
    }
}
