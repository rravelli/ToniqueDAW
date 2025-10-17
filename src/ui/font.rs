use egui::{FontDefinitions, FontFamily};

pub const PHOSPHOR_REGULAR: &str = "phosphor_regular";
pub const PHOSPHOR_FILL: &str = "phosphor_fill";

pub fn get_fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        PHOSPHOR_REGULAR.into(),
        egui_phosphor::Variant::Regular.font_data().into(),
    );

    fonts.font_data.insert(
        PHOSPHOR_FILL.into(),
        egui_phosphor::Variant::Fill.font_data().into(),
    );
    fonts.families.insert(
        FontFamily::Name(PHOSPHOR_FILL.into()),
        vec![PHOSPHOR_FILL.into()],
    );
    fonts.families.insert(
        FontFamily::Name(PHOSPHOR_REGULAR.into()),
        vec![PHOSPHOR_REGULAR.into()],
    );
    if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        font_keys.insert(1, PHOSPHOR_FILL.into());
    }
    if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        font_keys.insert(1, PHOSPHOR_FILL.into());
    }

    fonts
}
