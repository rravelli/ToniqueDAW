use egui::{FontData, FontDefinitions, FontFamily, FontId};

pub const PHOSPHOR_REGULAR: &str = "phosphor_regular";
pub const PHOSPHOR_FILL: &str = "phosphor_fill";
/// Inter SemiBold, for titles and names that lead (see [`semibold`]).
pub const SEMIBOLD: &str = "inter_semibold";
const INTER: &str = "inter";
const JETBRAINS_MONO: &str = "jetbrains_mono";

/// Text at `size` in Inter SemiBold: titles, track and effect names.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        INTER.into(),
        FontData::from_static(include_bytes!("../../fonts/Inter-Regular.ttf")).into(),
    );
    fonts.font_data.insert(
        SEMIBOLD.into(),
        FontData::from_static(include_bytes!("../../fonts/Inter-SemiBold.ttf")).into(),
    );
    fonts.font_data.insert(
        JETBRAINS_MONO.into(),
        FontData::from_static(include_bytes!("../../fonts/JetBrainsMonoNL-Regular.ttf")).into(),
    );

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
        // Inter first: sharper than egui's default at small sizes, which stays
        // a fallback. The bundled Inter has its private-use glyphs removed so
        // it doesn't hide Phosphor's icons, and its tabular digits (all the
        // same width) as its digits: egui can't turn on font features, and
        // values that change (levels, times, tempo) mustn't shift.
        font_keys.insert(0, INTER.into());
        font_keys.insert(1, PHOSPHOR_REGULAR.into());
    }
    if let Some(font_keys) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        // JetBrains Mono first, its private-use glyphs removed too.
        font_keys.insert(0, JETBRAINS_MONO.into());
        font_keys.insert(1, PHOSPHOR_REGULAR.into());
    }
    // SemiBold, then what Proportional falls back on.
    let mut semibold = vec![SEMIBOLD.to_string()];
    semibold.extend(
        fonts.families[&FontFamily::Proportional]
            .iter()
            .filter(|f| *f != INTER)
            .cloned(),
    );
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);

    fonts
}
