use std::sync::Arc;

use eframe::NativeOptions;

const APP_ICON: &'static [u8; 15773] = include_bytes!("../../images/logo.png");

pub fn get_native_options() -> NativeOptions {
    let mut options = NativeOptions::default();
    // App icon
    let d = eframe::icon_data::from_png_bytes(APP_ICON).expect("Invalid icon");
    options.viewport.icon = Some(Arc::new(d));
    options.window_builder = Some(Box::new(|vb| vb.with_title_shown(false)));
    options
}
