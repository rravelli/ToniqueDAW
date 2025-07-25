use egui::{Color32, Sense, Vec2};

pub fn left_aligned_selectable(ui: &mut egui::Ui, text: impl ToString) -> egui::Response {
    let (response, painter) =
        ui.allocate_painter(Vec2::new(ui.available_width(), 16.0), Sense::all());

    let bg_color = if response.hovered() {
        Color32::from_gray(60)
    } else {
        Color32::from_gray(30)
    };

    painter.rect_filled(response.rect, 0., bg_color);

    let mut font_id = egui::TextStyle::Button.resolve(ui.style());
    font_id.size = 12.;

    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id, Color32::WHITE);

    painter.galley(
        response.rect.left_top() + Vec2::new(6.0, 1.0),
        galley,
        Color32::WHITE,
    );

    // ui.painter().add(shape)

    response
}
