//! The controls the views share: buttons, menus, fields, sliders, panel
//! edges, and text painted on a canvas. Each is styled as its egui
//! counterpart in `ui/widget`.

mod button;
mod dialog;
mod icon_rail;
mod menu;
mod number_field;
mod param_slider;
mod select;
mod slider;
mod splitter;
mod tab_bar;
mod text;
mod text_field;

pub use button::{Flat, glyph, on_press, toggle_button};
pub use dialog::{dialog_body, dialog_button, dialog_frame, dialog_title};
pub use icon_rail::icon_rail;
pub use menu::{Menu, MenuItem};
pub use number_field::{NumberField, NumberSpec};
pub use param_slider::{Scale, SliderDrag, paint_param_slider};
pub use select::select;
pub use slider::paint_slider;
pub use splitter::{Edge, Splitter, splitter_handle};
pub use tab_bar::tab_bar;
pub use text::{paint_line, paint_text, shape};
pub use text_field::{FieldKey, TextField, typing_view};
