//! A dropdown, as the egui app's `Select`: the chosen option's label and a
//! caret in a control-coloured box, outlined in the accent while its menu
//! is open. Pressed, it opens `kind`'s menu below it, whose items set the
//! value.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, Bounds, Context, MouseButton, Pixels, SharedString, Stateful, canvas, div, point,
    prelude::*, px,
};

use crate::{
    app::PopupKind,
    icons,
    popup::{self, MenuBuilder},
    theme::{Theme, hsla, icon_fill, size},
};

const HEIGHT: f32 = 22.;
const PADDING: f32 = 6.;
const CARET_SIZE: f32 = 9.;
/// Between the box and its menu.
const MENU_OFFSET: f32 = 2.;

/// The box, `width` wide, showing `label`; `open` while its menu, `kind`
/// built by `build`, is.
#[allow(clippy::too_many_arguments)]
pub fn select<V: 'static>(
    id: &'static str,
    label: impl Into<SharedString>,
    width: f32,
    kind: PopupKind,
    build: MenuBuilder,
    open: bool,
    theme: &Theme,
    context: &mut Context<V>,
) -> Stateful<gpui::Div> {
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
    let slot = bounds.clone();
    let border = if open { theme.accent } else { theme.border };
    let fill = if open {
        theme.bg_control_hover
    } else {
        theme.bg_control
    };
    div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(PADDING))
        .w(px(width))
        .h(px(HEIGHT))
        .px(px(PADDING))
        .rounded(px(2.))
        .bg(hsla(fill))
        .border_1()
        .border_color(hsla(border))
        .hover(|style| style.bg(hsla(theme.bg_control_hover)))
        .cursor_pointer()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(label.into()),
        )
        .child(
            icon_fill(icons::CARET_DOWN)
                .text_size(px(CARET_SIZE))
                .text_color(hsla(theme.text)),
        )
        .child(
            canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .text_size(px(size::BODY))
        .on_mouse_down(
            MouseButton::Left,
            context.listener(move |_, _, _, context| {
                context.stop_propagation();
                let below = bounds.get();
                let at = point(below.left(), below.bottom() + px(MENU_OFFSET));
                popup::toggle(kind, at, Anchor::TopLeft, build.clone(), context);
            }),
        )
}
