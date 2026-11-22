//! The controls the views share: buttons, menus, fields, sliders, panel
//! edges, scrollbars, and text painted on a canvas. Each is styled as its egui
//! counterpart in `ui/widget`.

mod arrow_keys;
mod button;
mod dialog;
mod icon_rail;
mod menu;
mod number_field;
mod param_slider;
mod scrollbar;
mod select;
mod slider;
mod splitter;
mod tab_bar;
mod text;
mod text_field;

pub use arrow_keys::arrow_press;
pub use button::{Flat, glyph, on_press, toggle_button};
pub use dialog::{dialog_body, dialog_button, dialog_frame, dialog_title};
pub use icon_rail::icon_rail;
pub use menu::{Menu, MenuItem};
pub use number_field::{NumberField, NumberSpec};
pub use param_slider::{Scale, SliderDrag, paint_param_slider};
pub use scrollbar::{
    Axis, SCROLLBAR_WIDTH, ScrollSpan, ScrollbarGrab, handle_scrollbar, scrollbar,
};
pub use select::select;
pub use slider::paint_slider;
pub use splitter::{Edge, HANDLE_WIDTH, Splitter, splitter_handle};
pub use tab_bar::tab_bar;
pub use text::{paint_line, paint_text, shape};
pub use text_field::{FieldKey, TextField, typing_view};
