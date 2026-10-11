use std::sync::Arc;

use eframe::NativeOptions;

const APP_ICON: &[u8] = include_bytes!("../../images/logo-256.png");
/// Room for the browser, a useful stretch of timeline and the effects.
const DEFAULT_SIZE: [f32; 2] = [1280., 800.];
/// Below this the transport bar's groups would overlap.
const MIN_SIZE: [f32; 2] = [900., 560.];

pub fn native_options() -> NativeOptions {
    let mut options = NativeOptions::default();
    // App icon
    let d = eframe::icon_data::from_png_bytes(APP_ICON).expect("Invalid icon");
    options.viewport.icon = Some(Arc::new(d));
    options.viewport.inner_size = Some(DEFAULT_SIZE.into());
    options.viewport.min_inner_size = Some(MIN_SIZE.into());
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
