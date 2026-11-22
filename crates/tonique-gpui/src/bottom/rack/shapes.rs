//! Filter shapes drawn as small icons, as the egui app's: each is the
//! shape's real response, so it reads like the curve it gives. A picker
//! opens a menu of them by name; a row of them picks with one click.

use std::{cell::Cell, rc::Rc};

use gpui::{
    Anchor, Bounds, Context, MouseButton, Pixels, Stateful, canvas, div, point, prelude::*, px,
};
use tonique_engine::{dsp::Biquad, nodes::FilterMode};

use crate::{
    app::{PopupKind, Tonique},
    bottom::paint::polyline,
    geometry::Rect,
    icons,
    theme::{Color, Theme, hsla, icon_fill, size},
    widgets::{Flat, Menu, MenuItem},
};

/// Frequencies an icon spans, in Hz, around its shape's at 1 kHz.
const ICON_HZ: (f32, f32) = (40., 25_000.);
/// Gains an icon spans, in dB: room above 0 for boosts, more below for
/// cuts.
const ICON_DB: (f32, f32) = (14., -30.);
const ICON_SAMPLE_RATE: f32 = 48_000.;
/// A shape's button in a row of them.
pub const SHAPE_BUTTON: (f32, f32) = (19., 18.);
const PICKER_HEIGHT: f32 = 20.;
/// The icon in a picker, and in a menu's row.
const PICKER_ICON: (f32, f32) = (18., 12.);
const MENU_ICON: (f32, f32) = (13., 10.);
/// Between a picker and its menu.
const MENU_OFFSET: f32 = 2.;

/// `mode`'s response drawn across `rect`.
pub fn paint_shape_icon(
    window: &mut gpui::Window,
    rect: Rect,
    mode: FilterMode,
    width: f32,
    color: Color,
) {
    let (q, gain) = match mode {
        FilterMode::Peak => (1.4, 10.),
        FilterMode::LowShelf | FilterMode::HighShelf => (0.8, 10.),
        FilterMode::Notch => (2.5, 0.),
        FilterMode::BandPass => (1.2, 0.),
        FilterMode::LowPass | FilterMode::HighPass => (0.9, 0.),
    };
    let filter = Biquad::new(mode, 1000., q, gain, ICON_SAMPLE_RATE);
    let (low, high) = ICON_HZ;
    let steps = (rect.width * 2.).ceil().max(8.) as usize;
    let points: Vec<(f32, f32)> = (0..=steps)
        .map(|step| {
            let along = step as f32 / steps as f32;
            let hz = low * (high / low).powf(along);
            let db = filter
                .response_db(hz, ICON_SAMPLE_RATE)
                .clamp(ICON_DB.1, ICON_DB.0);
            let down = (ICON_DB.0 - db) / (ICON_DB.0 - ICON_DB.1);
            (
                rect.left + along * rect.width,
                rect.top + down * rect.height,
            )
        })
        .collect();
    polyline(window, &points, width, color);
}

/// An element `(width, height)` big showing `mode`'s icon.
pub fn shape_icon(
    mode: FilterMode,
    (width, height): (f32, f32),
    stroke: f32,
    color: Color,
) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            paint_shape_icon(window, Rect::of(bounds), mode, stroke, color);
        },
    )
    .flex_none()
    .w(px(width))
    .h(px(height))
}

/// `menu` with an item for each of `modes`, by name and icon, `current`
/// in `tint`; `pick` run with the one pressed.
pub fn shape_menu(
    menu: Menu,
    modes: &[FilterMode],
    current: FilterMode,
    name: fn(FilterMode) -> &'static str,
    tint: Color,
    context: &mut Context<Tonique>,
    pick: impl Fn(&mut Tonique, FilterMode) + Clone + 'static,
) -> Menu {
    let muted = menu.theme().text_muted;
    modes.iter().fold(menu, |menu, &mode| {
        let on = mode == current;
        let ink = if on { tint } else { muted };
        let pick = pick.clone();
        menu.item(
            MenuItem::new(name(mode)).leading(shape_icon(mode, MENU_ICON, 1.2, ink)),
            context,
            move |this, _, _| {
                if !on {
                    pick(this, mode);
                }
            },
        )
    })
}

/// A row of a button per shape of `modes`, `current` filled with
/// `color`; `pick` run with the one pressed.
pub fn shape_row(
    id: impl Into<gpui::ElementId>,
    modes: &[FilterMode],
    current: FilterMode,
    color: Color,
    theme: &Theme,
    context: &mut Context<Tonique>,
    pick: impl Fn(&mut Tonique, FilterMode) + Clone + 'static,
) -> Stateful<gpui::Div> {
    let buttons = modes.iter().map(|&mode| {
        let on = mode == current;
        let ink = if on { theme.bg_deep } else { theme.text_muted };
        let button = Flat::new(("shape", mode as usize), SHAPE_BUTTON.1).width(SHAPE_BUTTON.0);
        let button = if on { button.fill(color) } else { button };
        let pick = pick.clone();
        let icon = shape_icon(mode, (SHAPE_BUTTON.0 - 6., SHAPE_BUTTON.1 - 8.), 1.3, ink);
        button.show(icon, theme, context, move |this, _| {
            if !on {
                pick(this, mode);
            }
        })
    });
    div().id(id.into()).flex().gap(px(2.)).children(buttons)
}

impl Tonique {
    /// A box showing `mode`'s icon in `tint` and its name, opening
    /// `kind`'s menu below it; `width` wide.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn shape_picker(
        &self,
        id: impl Into<gpui::ElementId>,
        mode: FilterMode,
        name: fn(FilterMode) -> &'static str,
        width: f32,
        tint: Color,
        kind: PopupKind,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let open = self.popup.as_ref().is_some_and(|popup| popup.kind == kind);
        let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
        let slot = bounds.clone();
        let (fill, border) = if open {
            (theme.bg_control_hover, theme.accent)
        } else {
            (theme.bg_control, theme.control_border)
        };
        div()
            .id(id.into())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(5.))
            .w(px(width))
            .h(px(PICKER_HEIGHT))
            .px(px(5.))
            .rounded(px(3.))
            .bg(hsla(fill))
            .border_1()
            .border_color(hsla(border))
            .hover(|style| style.bg(hsla(theme.bg_control_hover)))
            .cursor_pointer()
            .child(shape_icon(mode, PICKER_ICON, 1.5, tint))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(size::LABEL))
                    .child(name(mode)),
            )
            .child(
                icon_fill(icons::CARET_DOWN)
                    .text_size(px(9.))
                    .text_color(hsla(theme.text_muted)),
            )
            .child(
                canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    let below = bounds.get();
                    let at = point(below.left(), below.bottom() + px(MENU_OFFSET));
                    this.toggle_popup(kind, at, Anchor::TopLeft);
                    context.notify();
                }),
            )
    }
}
