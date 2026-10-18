//! The selected file's preview, under the browser: a play button, its name
//! and where the preview is in it, its waveform to seek in, and its format.

use crate::ui::theme::font_size;
use egui::{
    Align, Align2, CursorIcon, FontFamily, FontId, Label, Layout, Rect, RichText, Sense, Stroke,
    Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::fill::{PLAY, STOP};

use crate::{
    analysis::AudioInfo,
    core::state::{PlaybackState, ProjectState},
    ui::{
        font::PHOSPHOR_FILL,
        theme::{ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::flat_button::FlatButton,
    },
};

/// Height of the preview.
pub const PREVIEW_HEIGHT: f32 = 78.;
const PADDING: f32 = 6.;
/// Height of the row with the play button and the name.
const TOP_ROW: f32 = 20.;
const WAVEFORM_HEIGHT: f32 = 30.;
/// How opaque the part of the waveform not played yet is.
const UNPLAYED_ALPHA: u8 = 110;

/// `audio`'s format: `44.1 kHz · 24-bit · Stereo`.
pub fn format(audio: &AudioInfo) -> String {
    let channels = match audio.channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        n => format!("{n} channels"),
    };
    let bits = audio
        .bit_depth
        .map(|b| format!(" · {b}-bit"))
        .unwrap_or_default();
    format!(
        "{} kHz{bits} · {channels}",
        audio.sample_rate as f32 / 1000.
    )
}

/// `audio`'s format and length, for a tooltip.
pub fn details(audio: &AudioInfo) -> String {
    match length(audio) {
        Some(length) => format!("{} · {}", format(audio), seconds(length.ceil())),
        None => format(audio),
    }
}

/// `m:ss`: previews don't need finer.
fn seconds(seconds: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// In seconds; some files don't say how long they are until decoded.
fn length(audio: &AudioInfo) -> Option<f64> {
    audio
        .total_frames()
        .map(|frames| frames / audio.sample_rate as f64)
}

pub fn preview_ui(ui: &mut Ui, state: &mut ProjectState, audio: &AudioInfo) {
    let theme = ui.app_theme();
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_HEIGHT), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1., theme.separator),
    );
    let inner = rect.shrink(PADDING);
    let top = Rect::from_min_size(inner.min, vec2(inner.width(), TOP_ROW));
    let waveform = Rect::from_min_size(
        pos2(inner.left(), top.bottom() + 4.),
        vec2(inner.width(), WAVEFORM_HEIGHT),
    );
    let bottom = Rect::from_min_max(pos2(inner.left(), waveform.bottom() + 2.), inner.max);

    let playing = state.preview_playback_state() == PlaybackState::Playing
        && state.previewing() == Some(audio.path.as_path());
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(top)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            let button = FlatButton::new(if playing { STOP } else { PLAY })
                .square(TOP_ROW)
                .selected(playing)
                .font(FontId::new(11., FontFamily::Name(PHOSPHOR_FILL.into())))
                .tooltip(if playing { "Stop" } else { "Play" });
            if ui.add(button).clicked() {
                if playing {
                    state.pause_preview();
                } else {
                    state.play_preview(audio.path.clone());
                }
            }
            // The name from the left, cut short before where it is, at
            // the right.
            let time = length(audio).map(|length| {
                let at = if playing {
                    state.preview_position() as f64 / audio.sample_rate as f64
                } else {
                    0.
                };
                ui.painter().layout_no_wrap(
                    format!("{} / {}", seconds(at.floor()), seconds(length.ceil())),
                    FontId::monospace(font_size::SMALL),
                    theme.text_muted,
                )
            });
            let time_width = time.as_ref().map_or(0., |t| t.size().x + 6.);
            let name_width = (ui.available_width() - time_width).max(0.);
            ui.allocate_ui_with_layout(
                vec2(name_width, TOP_ROW),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.add(
                        Label::new(
                            RichText::new(&audio.name)
                                .strong()
                                .size(font_size::LABEL)
                                .color(theme.text),
                        )
                        .truncate()
                        .selectable(false),
                    );
                },
            );
            if let Some(time) = time {
                let pos = pos2(
                    top.right() - time.size().x,
                    top.center().y - time.size().y / 2.,
                );
                ui.painter().galley(pos, time, theme.text_muted);
            }
        },
    );

    waveform_ui(ui, waveform, audio, state, playing);

    ui.painter().text(
        pos2(bottom.left(), bottom.center().y),
        Align2::LEFT_CENTER,
        format(audio),
        FontId::proportional(font_size::SMALL),
        theme.text_muted,
    );
}

/// The file's waveform in a well: what's played in full colour, the rest
/// fainter. Clicking or dragging plays from there.
fn waveform_ui(
    ui: &mut Ui,
    rect: Rect,
    audio: &AudioInfo,
    state: &mut ProjectState,
    playing: bool,
) {
    let theme = ui.app_theme();
    let response = ui
        .interact(
            rect,
            ui.id().with("preview-waveform"),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(CursorIcon::PointingHand);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3., theme.bg_deep);
    // Only its waveform is decoded for it: not its samples.
    audio.load_peaks();
    if !audio.data.peaks_complete() {
        // The waveform grows while the file decodes.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let Some(frames) = audio.total_frames() else {
        return;
    };
    let lane = rect.shrink2(vec2(0., 2.));
    let to_x = |frame: f64| lane.left() + (frame / frames) as f32 * lane.width();

    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let frame = ((pointer.x - lane.left()) / lane.width()).clamp(0., 1.) as f64 * frames;
        if !playing {
            state.play_preview(audio.path.clone());
        }
        state.seek_preview(frame.round() as usize);
    }

    let at = if playing {
        to_x(state.preview_position() as f64)
    } else {
        lane.left()
    };
    let stereo = audio.channels >= 2;
    let draw = |visible: Rect, color| {
        paint_waveform(
            &painter,
            lane,
            visible,
            &audio.data,
            0.0..frames,
            stereo,
            color,
            1.,
            0.,
        );
    };
    draw(
        Rect::from_x_y_ranges(at..=lane.right(), lane.y_range()),
        with_alpha(theme.accent, UNPLAYED_ALPHA),
    );
    draw(
        Rect::from_x_y_ranges(lane.left()..=at, lane.y_range()),
        theme.accent,
    );
    if playing {
        painter.vline(at, rect.y_range(), Stroke::new(1., theme.playhead));
    }
    if let Some(pointer) = response.hover_pos() {
        painter.vline(
            pointer.x,
            rect.y_range(),
            Stroke::new(1., with_alpha(theme.text, 120)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_tell_rate_depth_channels_and_length() {
        let mut audio = AudioInfo::silent(1.);
        audio.sample_rate = 44100;
        audio.bit_depth = Some(24);
        assert_eq!(format(&audio), "44.1 kHz · 24-bit · Stereo");
        audio.channels = 1;
        audio.bit_depth = None;
        assert_eq!(format(&audio), "44.1 kHz · Mono", "a depth only if known");
        // 480 frames at 44.1 kHz: a part of a second, counted as one.
        assert_eq!(details(&audio), "44.1 kHz · Mono · 0:01");
        assert_eq!(seconds(75.), "1:15");
    }
}
