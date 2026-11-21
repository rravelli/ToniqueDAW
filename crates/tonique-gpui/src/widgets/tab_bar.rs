//! Tabs side by side, as the egui app's `TabBar`: the selected one's text
//! bright and underlined in the accent, the others muted on a thin line.

use gpui::{Context, MouseButton, div, prelude::*, px};

use crate::{
    app::Tonique,
    theme::{Theme, hsla, size},
};

const HEIGHT: f32 = 24.;
const UNDERLINE: f32 = 2.;

/// `tabs` sharing the width, `selected` lit; `select` takes a pressed one.
pub fn tab_bar<Tab: Copy + PartialEq + 'static>(
    tabs: &[(Tab, &'static str)],
    selected: Tab,
    theme: &Theme,
    context: &mut Context<Tonique>,
    select: fn(&mut Tonique, Tab),
) -> gpui::Div {
    let tabs = tabs.iter().enumerate().map(|(index, &(tab, name))| {
        let is_selected = tab == selected;
        let (ink, line, line_height) = if is_selected {
            (theme.text, theme.accent, UNDERLINE)
        } else {
            (theme.text_muted, theme.border, 1.)
        };
        div()
            .id(("tab", index))
            .flex_1()
            .flex()
            .flex_col()
            .h(px(HEIGHT))
            .rounded_t(px(2.))
            .text_size(px(size::BODY))
            .text_color(hsla(ink))
            .when(!is_selected, |element| {
                element
                    .cursor_pointer()
                    .hover(|style| style.bg(hsla(theme.hover_overlay)))
            })
            .child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(name),
            )
            .child(div().h(px(line_height)).bg(hsla(line)))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    select(this, tab);
                    context.notify();
                }),
            )
    });
    div().flex().items_end().children(tabs)
}
