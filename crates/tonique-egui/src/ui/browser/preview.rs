//! The selected file's preview, under the browser: its name and format;
//! its waveform to seek in, with where the preview is in it and how long it
//! is; and a row like the transport's, with play, the switch that plays
//! files as they're selected, and the preview's volume.

use crate::ui::theme::font_size;
use egui::{
    Align, Align2, CursorIcon, FontFamily, FontId, Label, Layout, Rect, RichText, Sense, Stroke,
    Ui, UiBuilder, pos2, vec2,
};
use egui_phosphor::{
    fill::{PLAY, STOP},
    regular::{HEADPHONES, SPEAKER_LOW, SPEAKER_SLASH},
};

use crate::{
    analysis::AudioInfo,
    core::state::{PlaybackState, ProjectState},
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::{ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::flat_button::FlatButton,
    },
};

/// Height of the preview.
pub const PREVIEW_HEIGHT: f32 = 102.;
const PADDING: f32 = 6.;
const GAP: f32 = 4.;
/// Height of the row with the file's name and format.
const HEADER: f32 = 18.;
const WAVEFORM_HEIGHT: f32 = 42.;
/// Side of the buttons, and height of the row they're in.
const BUTTON: f32 = 22.;
const BUTTON_GAP: f32 = 2.;
const VOLUME_WIDTH: f32 = 96.;
/// Between the groups of the bottom row.
const GROUP_GAP: f32 = 8.;
/// The volume slider's track.
const TRACK_HEIGHT: f32 = 4.;
const HANDLE_RADIUS: f32 = 5.;
/// How opaque the part of the waveform not played yet is.
const UNPLAYED_ALPHA: u8 = 110;

/// What the preview remembers between files.
#[derive(Clone, Copy, PartialEq)]
pub struct PreviewSettings {
    /// Selecting a file plays it.
    pub auto_preview: bool,
    /// Where the volume slider is, from 0 to 1.
    pub volume: f32,
    /// Silenced from its speaker, the slider kept where it is.
    pub muted: bool,
}

impl PreviewSettings {
    /// The gain the preview plays at.
    pub fn gain(&self) -> f32 {
        if self.muted { 0. } else { gain(self.volume) }
    }
}

/// The preview's gain for the slider at `volume`: a square law, so the
/// slider's travel is spent where it's heard (half way is -12 dB).
pub fn gain(volume: f32) -> f32 {
    volume.clamp(0., 1.).powi(2)
}

/// `audio`'s format: `44.1 kHz · 24-bit · Stereo`.
pub fn format(audio: &AudioInfo) -> String {
    let bits = audio
        .bit_depth
        .map(|b| format!(" · {b}-bit"))
        .unwrap_or_default();
    format!(
        "{} kHz{bits} · {}",
        audio.sample_rate as f32 / 1000.,
        channels(audio.channels)
    )
}

fn channels(n: u16) -> String {
    match n {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        n => format!("{n} channels"),
    }
}

/// `m:ss`.
fn minutes(seconds: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// To the tenth of a second under ten (`0.7 s`), so one-shots don't all
/// read the same; `m:ss` past it.
fn seconds(seconds: f64) -> String {
    let seconds = seconds.max(0.);
    if seconds < 10. {
        format!("{seconds:.1} s")
    } else {
        minutes(seconds.floor())
    }
}

/// `44.1k · 24-bit · Stereo`: short, to leave room for the name.
fn short_format(audio: &AudioInfo) -> String {
    let khz = format!("{:.1}", audio.sample_rate as f32 / 1000.);
    let bits = audio
        .bit_depth
        .map(|b| format!(" · {b}-bit"))
        .unwrap_or_default();
    format!(
        "{}k{bits} · {}",
        khz.trim_end_matches(".0"),
        channels(audio.channels)
    )
}

pub fn preview_ui(
    ui: &mut Ui,
    state: &mut ProjectState,
    audio: &AudioInfo,
    settings: &mut PreviewSettings,
) {
    let theme = ui.app_theme();
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), PREVIEW_HEIGHT), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1., theme.separator),
    );
    let inner = rect.shrink(PADDING);
    let header = Rect::from_min_size(inner.min, vec2(inner.width(), HEADER));
    let waveform = Rect::from_min_size(
        pos2(inner.left(), header.bottom() + GAP),
        vec2(inner.width(), WAVEFORM_HEIGHT),
    );
    let controls = Rect::from_min_max(pos2(inner.left(), inner.bottom() - BUTTON), inner.max);

    let playing = state.preview_playback_state() == PlaybackState::Playing
        && state.previewing() == Some(audio.path.as_path());
    header_ui(ui, header, audio);
    waveform_ui(ui, waveform, audio, state, playing);
    controls_ui(ui, controls, state, audio, settings, playing);
}

/// The name from the left, cut short before the format, at the right.
fn header_ui(ui: &mut Ui, rect: Rect, audio: &AudioInfo) {
    let theme = ui.app_theme();
    let format = ui.painter().layout_no_wrap(
        short_format(audio),
        FontId::proportional(font_size::SMALL),
        theme.text_muted,
    );
    let format_rect = Align2::RIGHT_CENTER.anchor_size(rect.right_center(), format.size());
    ui.painter()
        .galley(format_rect.min, format, theme.text_muted);
    ui.interact(format_rect, ui.id().with("preview-format"), Sense::hover())
        .on_hover_text(self::format(audio));

    let name = Rect::from_min_max(
        rect.min,
        pos2(format_rect.left() - GROUP_GAP, rect.bottom()),
    );
    let layout = Layout::left_to_right(Align::Center);
    ui.scope_builder(UiBuilder::new().max_rect(name).layout(layout), |ui| {
        // Cut short, the label shows the whole name when hovered.
        ui.add(
            Label::new(
                RichText::new(&audio.name)
                    .strong()
                    .size(font_size::BODY)
                    .color(theme.text),
            )
            .truncate()
            .selectable(false),
        );
    });
}

/// Play, the switch, the readout and the volume, from the left.
fn controls_ui(
    ui: &mut Ui,
    rect: Rect,
    state: &mut ProjectState,
    audio: &AudioInfo,
    settings: &mut PreviewSettings,
    playing: bool,
) {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = BUTTON_GAP;
            let button = FlatButton::new(if playing { STOP } else { PLAY })
                .square(BUTTON)
                .selected(playing)
                .border_radius(2.)
                .font(FontId::new(12., FontFamily::Name(PHOSPHOR_FILL.into())))
                .tooltip(if playing { "Stop" } else { "Play" });
            if ui.add(button).clicked() {
                if playing {
                    state.pause_preview();
                } else {
                    state.play_preview(audio.path.clone());
                }
            }
            let on = settings.auto_preview;
            let button = FlatButton::new(HEADPHONES)
                .square(BUTTON)
                .selected(on)
                .border_radius(2.)
                .font(FontId::new(13., FontFamily::Name(PHOSPHOR_REGULAR.into())))
                .tooltip("Auto-play");
            if ui.add(button).clicked() {
                settings.auto_preview = !on;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                volume_ui(ui, settings);
            });
        },
    );
}

/// From the right, in a right-to-left layout: a slider for the preview's
/// volume, then a speaker that mutes it. Double-click the slider for full
/// volume.
fn volume_ui(ui: &mut Ui, settings: &mut PreviewSettings) {
    let theme = ui.app_theme();
    // None in a browser too narrow for it.
    let width = VOLUME_WIDTH
        .min(ui.available_width() - BUTTON - BUTTON_GAP)
        .max(0.);
    let (rect, response) = ui.allocate_exact_size(vec2(width, BUTTON), Sense::click_and_drag());
    let silent = settings.muted || settings.volume <= 0.;
    let speaker = FlatButton::ghost(if silent { SPEAKER_SLASH } else { SPEAKER_LOW })
        .square(BUTTON)
        .border_radius(2.)
        .font(FontId::new(13., FontFamily::Name(PHOSPHOR_REGULAR.into())))
        .tooltip(if silent { "Unmute" } else { "Mute" });
    if ui.add(speaker).clicked() {
        if silent {
            settings.muted = false;
            // Unmuted at no volume: back to full, to be heard.
            if settings.volume <= 0. {
                settings.volume = 1.;
            }
        } else {
            settings.muted = true;
        }
    }

    // The handle, at its largest, stays inside the slider: at full volume
    // its edge lines up with the waveform's.
    let inset = HANDLE_RADIUS + 1.;
    let track = Rect::from_min_max(
        pos2(rect.left() + inset, rect.center().y - TRACK_HEIGHT / 2.),
        pos2(rect.right() - inset, rect.center().y + TRACK_HEIGHT / 2.),
    );
    if track.width() <= 0. {
        return;
    }
    if response.double_clicked() {
        settings.volume = 1.;
        settings.muted = false;
    } else if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        settings.volume = ((pointer.x - track.left()) / track.width()).clamp(0., 1.);
        settings.muted = false;
    }

    let active = response.dragged() || response.hovered();
    let x = track.left() + settings.volume * track.width();
    let fill = match (settings.muted, active) {
        (true, _) => theme.text_disabled,
        (false, true) => theme.accent_hover,
        (false, false) => theme.accent,
    };
    let painter = ui.painter();
    painter.rect_filled(track, TRACK_HEIGHT / 2., theme.bg_deep);
    painter.rect_filled(track.with_max_x(x), TRACK_HEIGHT / 2., fill);
    painter.circle_filled(
        pos2(x, track.center().y),
        if active {
            HANDLE_RADIUS + 1.
        } else {
            HANDLE_RADIUS
        },
        if settings.muted {
            theme.text_muted
        } else {
            theme.text
        },
    );
    let db = 20. * gain(settings.volume).log10();
    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(match (settings.muted, db.is_finite()) {
            (true, _) => "Preview volume: muted".into(),
            (false, true) => format!("Preview volume: {db:.1} dB"),
            (false, false) => "Preview volume: silent".into(),
        });
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
    painter.rect_filled(rect, 2., theme.bg_deep);
    // Only its waveform is decoded for it: not its samples.
    audio.load_peaks();
    if audio.data.peaks_pending() {
        // The waveform grows while the file decodes.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let Some(frames) = audio.total_frames() else {
        return;
    };
    let lane = rect.shrink2(vec2(0., 2.));
    let to_x = |frame: f64| lane.left() + (frame / frames) as f32 * lane.width();

    // Held still, the pointer doesn't seek again: that would replay the
    // same instant every frame.
    let moved = response.dragged() && response.drag_delta().x != 0.;
    if (response.clicked() || response.drag_started() || moved)
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let frame = ((pointer.x - lane.left()) / lane.width()).clamp(0., 1.) as f64 * frames;
        if !playing {
            state.play_preview(audio.path.clone());
        }
        // Not started (the transport plays): nothing of this file to seek.
        if state.previewing() == Some(audio.path.as_path()) {
            state.seek_preview(frame.round() as usize);
        }
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

    // Where the preview is, in the bottom left corner; how long the file
    // is, in the bottom right. Each on a veil, to read over the waveform.
    let played = at_seconds(state, audio, playing);
    let corner = |text: String, align: Align2, color| {
        let galley = painter.layout_no_wrap(text, FontId::monospace(font_size::SMALL - 1.), color);
        let anchor = match align.x() {
            Align::Min => rect.left_bottom() + vec2(3., -2.),
            _ => rect.right_bottom() + vec2(-3., -2.),
        };
        let text_rect = align.anchor_size(anchor, galley.size());
        painter.rect_filled(text_rect.expand2(vec2(3., 1.)), 2., theme.shadow);
        painter.galley(text_rect.min, galley, color);
    };
    corner(
        seconds(played),
        Align2::LEFT_BOTTOM,
        if playing {
            theme.text
        } else {
            theme.text_muted
        },
    );
    corner(
        seconds(frames / audio.sample_rate as f64),
        Align2::RIGHT_BOTTOM,
        theme.text_muted,
    );
}

/// Where the preview is in `audio`, in seconds: its start when it's not
/// playing.
fn at_seconds(state: &ProjectState, audio: &AudioInfo, playing: bool) -> f64 {
    if playing {
        state.preview_position() as f64 / audio.sample_rate as f64
    } else {
        0.
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
        assert_eq!(short_format(&audio), "44.1k · 24-bit · Stereo");
        audio.channels = 1;
        audio.bit_depth = None;
        audio.sample_rate = 48000;
        assert_eq!(format(&audio), "48 kHz · Mono", "a depth only if known");
        assert_eq!(short_format(&audio), "48k · Mono");
        assert_eq!(minutes(75.), "1:15");
    }

    #[test]
    fn seconds_to_the_tenth_then_minutes() {
        assert_eq!(seconds(0.7264), "0.7 s");
        assert_eq!(seconds(9.94), "9.9 s");
        assert_eq!(seconds(75.5), "1:15");
    }

    #[test]
    fn half_way_is_twelve_decibels_down() {
        assert_eq!(gain(1.), 1.);
        assert!((20. * gain(0.5).log10() + 12.04).abs() < 0.01);
        assert_eq!(gain(0.), 0.);
    }

    #[test]
    fn muted_is_silent_at_any_volume() {
        let mut settings = PreviewSettings {
            auto_preview: true,
            volume: 0.5,
            muted: false,
        };
        assert_eq!(settings.gain(), 0.25);
        settings.muted = true;
        assert_eq!(settings.gain(), 0.);
    }
}
