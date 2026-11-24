//! The browser's effects, searched by name or purpose: one row each,
//! dragged onto the bottom panel's effects or onto a track.

use super::BrowserPanel;
use gpui::{AnyElement, App, Context, KeyDownEvent, Window, div, prelude::*, px};

use crate::{
    app::focus_main,
    bottom::DraggedEffect,
    browser::DragGhost,
    icons,
    theme::{Theme, hsla, icon, mix},
    widgets::FieldKey,
};
use tonique_core::core::effect::EffectKind;

const ROW_HEIGHT: f32 = 22.;
/// How much a row lightens when hovered.
const HOVER_TINT: f32 = 0.05;

impl BrowserPanel {
    pub(super) fn effect_search_bar(
        &self,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.search_bar(
            "effect-search",
            &self.effect_search,
            "Search effects",
            false,
            theme,
            window,
            context,
            Self::effect_search_key,
            |this, _| this.effect_search.text.clear(),
        )
    }

    fn effect_search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let search = &mut self.effect_search;
        match search.key(event) {
            FieldKey::Edited => {}
            FieldKey::Submit => focus_main(window, context),
            FieldKey::Cancel => {
                search.text.clear();
                focus_main(window, context);
            }
            FieldKey::Ignored => return,
        }
        context.stop_propagation();
        context.notify();
    }

    pub(super) fn effects_list(&self, context: &App) -> AnyElement {
        let theme = &*crate::theme::current(context);
        let query = self.effect_search.text.trim();
        let found: Vec<EffectKind> = EffectKind::ALL
            .into_iter()
            .filter(|kind| kind.matches(query))
            .collect();
        let list = div().flex_1().min_h_0().flex().flex_col();
        if found.is_empty() {
            return list
                .p_3()
                .items_center()
                .text_color(hsla(theme.text_muted))
                .child("No effects match")
                .into_any_element();
        }
        let rows = found.into_iter().enumerate().map(|(index, kind)| {
            let background = theme.bg_panel;
            div()
                .id(("effect-row", index))
                .h(px(ROW_HEIGHT))
                .w_full()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .bg(hsla(background))
                .hover(move |style| style.bg(hsla(mix(background, theme.text, HOVER_TINT))))
                .cursor_grab()
                // One icon for all, the tab's: they're told apart by name.
                .child(icon(icons::MAGIC_WAND).text_color(hsla(theme.accent)))
                .child(kind.name())
                .on_drag(DraggedEffect { kind }, move |_, _, _, context| {
                    context.new(|_| DragGhost {
                        name: kind.name().into(),
                    })
                })
        });
        list.children(rows).into_any_element()
    }
}
