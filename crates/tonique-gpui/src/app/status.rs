//! The status bar: the audio output, and a reminder of the gestures.

use gpui::{div, prelude::*, px};

use crate::{
    app::Tonique,
    icons,
    theme::{hsla, icon, size},
};

const HEIGHT: f32 = 22.;
const HINTS: &str = "Space play/stop · drag files onto the timeline · Ctrl+wheel zoom · \
                     Shift+wheel scroll · Alt: no snap · right-click for menus";

impl Tonique {
    pub(super) fn status_bar(&self) -> impl IntoElement {
        let theme = &self.theme;
        let audio = match (self.state.audio(), &self.state.audio_error) {
            (Some(output), _) => format!("{} · {} Hz", output.device_name, output.sample_rate),
            (None, Some(error)) => format!("No audio: {error}"),
            (None, None) => "No audio".into(),
        };
        let speaker = if self.state.audio().is_some() {
            icons::SPEAKER_HIGH
        } else {
            icons::SPEAKER_SLASH
        };
        div()
            .flex()
            .gap_4()
            .px_3()
            .h(px(HEIGHT))
            .flex_none()
            .items_center()
            .bg(hsla(theme.bg_panel))
            .border_t_1()
            .border_color(hsla(theme.separator))
            .text_size(px(size::SMALL))
            .text_color(hsla(theme.text_muted))
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .child(icon(speaker))
                    .child(audio),
            )
            .child(div().flex_1())
            .child(HINTS)
    }
}
