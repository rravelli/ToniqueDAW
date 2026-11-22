//! A column of square icon buttons, one per item, as the egui app's
//! `IconRail`: the current one bright and marked by an accent bar on its
//! left edge, the others muted. Each shows its name on hover.

use gpui::{Context, MouseButton, div, prelude::*, px};

use crate::{
    app::Tonique,
    theme::{Theme, hsla, icon, size},
    widgets::tooltip,
};

/// Thickness of the bar beside the current item.
const INDICATOR: f32 = 2.;

/// `items` (value, icon, name) one under the other, `button` wide,
/// `selected` marked; `select` takes another one pressed.
pub fn icon_rail<Item: Copy + PartialEq + 'static>(
    items: &[(Item, &'static str, &'static str)],
    selected: Item,
    button: f32,
    theme: &Theme,
    context: &mut Context<Tonique>,
    select: fn(&mut Tonique, Item),
) -> gpui::Div {
    let buttons = items
        .iter()
        .enumerate()
        .map(|(index, &(item, glyph, name))| {
            let is_selected = item == selected;
            div()
                .id(("rail", index))
                .relative()
                .size(px(button))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.))
                .text_size(px(size::HEADING + 2.))
                .text_color(hsla(if is_selected {
                    theme.text
                } else {
                    theme.text_muted
                }))
                .cursor_pointer()
                .when(!is_selected, |button| {
                    button.hover(|style| style.bg(hsla(theme.hover_overlay)))
                })
                .when(is_selected, |button| {
                    button.child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(px(INDICATOR))
                            .bg(hsla(theme.accent)),
                    )
                })
                .child(icon(glyph))
                .tooltip(tooltip(name, theme))
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, _, context| {
                        if !is_selected {
                            select(this, item);
                            context.notify();
                        }
                    }),
                )
        });
    div().flex_none().flex().flex_col().children(buttons)
}
