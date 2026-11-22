//! An effect open in a full window, as the egui app's: over the timeline
//! and the bottom panel, up to the transport bar, with a bar holding its
//! power button, its name and a button back to the rack. Escape goes back
//! too.

use gpui::{AnyElement, Context, Window, div, prelude::*, px};

use crate::{
    app::Tonique,
    bottom::rack::editors::EditorInput,
    icons,
    theme::{hsla, size},
    widgets::{Flat, glyph},
};

const MARGIN: f32 = 12.;
const BAR_HEIGHT: f32 = 24.;
const POWER_SIZE: f32 = 20.;
const BACK_ICON: f32 = 14.;

impl Tonique {
    /// The effect open in a full window, if one is and it's still there.
    pub fn full_effect_view(
        &mut self,
        _window: &mut Window,
        context: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let plugin = self.bottom.rack.expanded?;
        let track = self.bottom.rack.track;
        let effect = track.and_then(|track| {
            self.state
                .effects(&track)
                .into_iter()
                .find(|effect| effect.plugin.id == plugin)
        });
        let (Some(track), Some(effect)) = (track, effect) else {
            self.bottom.rack.expanded = None;
            return None;
        };
        let sample_rate = self.state.engine_config().sample_rate as f32;
        // The rack isn't drawn under it: its editor is fed here.
        self.bottom.rack.animating = self.bottom.rack.refresh(&effect, sample_rate);
        let editor = self.bottom.rack.slots.get(&plugin)?.editor.clone();
        let input = EditorInput {
            effect: &effect,
            track,
            sample_rate,
            editor,
            pending: self
                .bottom
                .rack
                .pending
                .filter(|(pending, ..)| *pending == plugin)
                .map(|(_, knob, value)| (knob, value)),
            expanded: true,
        };
        let theme = self.theme.clone();
        let enabled = effect.enabled();
        let power = Flat::new("full-power", POWER_SIZE).selected(enabled).show(
            glyph(icons::POWER, 12., false),
            &theme,
            context,
            move |this, _| this.state.set_effect_enabled(&track, plugin, !enabled),
        );
        let back = Flat::ghost("full-back", BAR_HEIGHT).show(
            glyph(icons::ARROWS_IN_SIMPLE, BACK_ICON, false),
            &theme,
            context,
            |this, _| this.bottom.rack.close_full_window(),
        );
        let bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .h(px(BAR_HEIGHT))
            .child(power)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(size::HEADING))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(hsla(if enabled {
                        theme.text
                    } else {
                        theme.text_muted
                    }))
                    .child(effect.name().to_string()),
            )
            .child(back);
        let body = self.editor_body(&input, &theme, context);
        Some(
            div()
                .id("effect-full-window")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .w_full()
                .gap(px(8.))
                .p(px(MARGIN))
                .bg(hsla(theme.bg_panel))
                .border_t_1()
                .border_color(hsla(theme.separator))
                .child(bar)
                .child(div().flex_1().min_h_0().w_full().child(body))
                .into_any_element(),
        )
    }
}
