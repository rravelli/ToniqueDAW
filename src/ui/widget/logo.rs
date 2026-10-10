use egui::{Image, ImageSource, include_image};

/// Tonique's logo, sharp at any size. Needs egui_extras' image loaders.
pub fn logo<'a>() -> Image<'a> {
    Image::new(LOGO)
}

const LOGO: ImageSource<'static> = include_image!("../../../images/logo.svg");
