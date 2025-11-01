use egui::{Color32, Context, FontId, Layout, MenuBar, Rect, Sense, Ui, ViewportCommand};
use egui_phosphor::{
    fill::{ARROWS_OUT_SIMPLE, MINUS, RECTANGLE},
    regular::X,
};

use crate::ui::{font::PHOSPHOR_REGULAR, widget::square_button::SquareButton};

struct UIWindowBar {
    maximized: bool,
}

impl UIWindowBar {
    fn ui(&mut self, ctx: &mut Context) {
        egui::TopBottomPanel::top("decoration")
            .resizable(false)
            .show(ctx, |ui| {
                let title_h = 20.0;

                // draw bar:
                ui.set_height(20.);
                let resp = ui.interact(
                    Rect::from_min_size(ui.next_widget_position(), ui.available_size()),
                    "sskdfjdfl".into(),
                    Sense::all(),
                );

                MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {});
                    ui.menu_button("Edit", |ui| {});
                    ui.menu_button("Preferences", |ui| {});
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let close_resp = ui.add(
                            SquareButton::new(X)
                                .fill(Color32::TRANSPARENT)
                                .sized(18.)
                                .font(FontId::new(
                                    10.,
                                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                                ))
                                .hover_color(Color32::from_gray(30))
                                .border_radius(6.),
                        );
                        if close_resp.clicked() {
                            // request the viewport to close:
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                        let maximize_resp = ui.add(
                            SquareButton::new(if self.maximized {
                                RECTANGLE
                            } else {
                                ARROWS_OUT_SIMPLE
                            })
                            .fill(Color32::TRANSPARENT)
                            .sized(18.)
                            .font(FontId::new(
                                10.,
                                egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                            ))
                            .hover_color(Color32::from_gray(30))
                            .border_radius(6.),
                        );
                        if maximize_resp.clicked() {
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(!self.maximized));
                            self.maximized = !self.maximized;
                        }
                        let minimize_resp = ui.add(
                            SquareButton::new(MINUS)
                                .fill(Color32::TRANSPARENT)
                                .sized(18.)
                                .font(FontId::new(
                                    10.,
                                    egui::FontFamily::Name(PHOSPHOR_REGULAR.into()),
                                ))
                                .hover_color(Color32::from_gray(30))
                                .border_radius(6.),
                        );
                        if minimize_resp.clicked() {
                            // request the viewport to close:
                            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
                        }
                    });
                });

                if resp.double_clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!self.maximized));
                    self.maximized = !self.maximized;
                }
                // Start a native drag when the titlebar is dragged:
                if resp.drag_started() {
                    // Tell the backend to start a native window drag operation.
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
                }
            });
    }
}
