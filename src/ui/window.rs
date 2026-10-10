use std::sync::Arc;

use eframe::NativeOptions;

const APP_ICON: &[u8] = include_bytes!("../../images/logo-256.png");

pub fn native_options() -> NativeOptions {
    let mut options = NativeOptions::default();
    // App icon
    let d = eframe::icon_data::from_png_bytes(APP_ICON).expect("Invalid icon");
    options.viewport.icon = Some(Arc::new(d));
    options.window_builder = Some(Box::new(|vb| vb.with_title_shown(false)));
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app icon decodes: a bad file would panic at startup.
    #[test]
    fn app_icon_decodes() {
        let icon = eframe::icon_data::from_png_bytes(APP_ICON).unwrap();
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
