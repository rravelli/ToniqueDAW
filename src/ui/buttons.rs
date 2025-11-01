use egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};

pub fn paint_circle_button(
    ui: &mut Ui,
    painter: &Painter,
    center: Pos2,
    value: &mut f32,
    id: String,
    name: String,
    label: Option<String>,
    min: f32,
    max: f32,
    log: bool,
) -> Response {
    let radius = 10.;

    let response = ui.interact(
        Rect::from_center_size(center, Vec2::new(2. * radius, 2. * radius)),
        format!("{id}-{name}").into(),
        Sense::click_and_drag(),
    );

    let mut ratio = if log {
        (*value / min).log10() / (max / min).log10()
    } else {
        (*value - min) / (max - min)
    };

    if response.dragged() {
        ratio += response.drag_delta().y / 50.;
        ratio = ratio.clamp(0., 1.);

        if log {
            *value = min * (max / min).powf(ratio);
        } else {
            *value = min + (max - min) * ratio;
        }
    }

    // Draw base circle
    painter.circle_stroke(center, radius, Stroke::new(2.0, Color32::DARK_GRAY));

    // Arc fill from top clockwise
    if ratio > 0.01 {
        let segments = 64;
        let angle_range = std::f32::consts::TAU * ratio.min(0.99);
        let start_angle = -std::f32::consts::FRAC_PI_2; // start at top

        let mut points = Vec::with_capacity(segments + 2);
        points.push(center); // center of the circle

        for i in 0..=segments {
            let t = i as f32 / segments as f32;
            let angle = start_angle + t * angle_range;
            let x = center.x + angle.cos() * radius;
            let y = center.y + angle.sin() * radius;
            points.push(Pos2::new(x, y));
        }

        painter.add(Shape::convex_polygon(
            points,
            Color32::from_rgb(80, 160, 240),
            Stroke::NONE,
        ));
    }
    painter.text(
        Pos2::new(center.x, center.y - radius - 2.0),
        Align2::CENTER_BOTTOM,
        name,
        FontId::new(10., egui::FontFamily::Proportional),
        Color32::DARK_GRAY,
    );
    if let Some(label) = label {
        painter.text(
            Pos2::new(center.x, center.y + radius + 2.0),
            Align2::CENTER_TOP,
            label,
            FontId::new(10., egui::FontFamily::Proportional),
            Color32::DARK_GRAY,
        );
    }
    // Optional: draw inner circle for contrast
    painter.circle_filled(center, radius - 3.0, Color32::DARK_GRAY);

    response
}
