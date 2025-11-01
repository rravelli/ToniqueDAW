use egui::{
    Color32, CornerRadius, FontId, Margin, Shadow, Spacing, Stroke, Style, TextStyle, Vec2, Visuals,
};

// For now stored in a constant
// pub const PRIMARY_COLOR: Color32 = Color32::from_rgb(218, 131, 113);
pub const PRIMARY_COLOR: Color32 = Color32::from_rgb(0, 200, 150);

pub fn get_app_style() -> Style {
    let mut style = Style::default();
    style.visuals = get_app_visuals();
    style.spacing = get_app_spacing();
    style.text_styles.insert(
        TextStyle::Body,
        FontId::proportional(12.0), // <— font size in points
    );
    style
}

fn get_app_visuals() -> Visuals {
    let mut visuals = Visuals::dark();

    visuals.panel_fill = Color32::from_gray(40);
    visuals.window_corner_radius = 1.into();
    visuals.menu_corner_radius = CornerRadius::same(2);
    visuals.popup_shadow = Shadow::NONE;
    visuals.window_stroke = Stroke::new(0.5, Color32::from_white_alpha(200));
    visuals.selection.bg_fill = PRIMARY_COLOR;
    visuals
}

fn get_app_spacing() -> Spacing {
    let mut spacing = Spacing::default();
    spacing.item_spacing = Vec2::ZERO;
    spacing.window_margin = Margin::ZERO;
    spacing.menu_margin = Margin::same(4);
    spacing
}
