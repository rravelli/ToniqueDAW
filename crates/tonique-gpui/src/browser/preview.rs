//! Under the tree, the selected file's preview: its name, length and
//! format, its waveform to seek in, play, auto-preview, and the preview's
//! volume.

use super::BrowserPanel;
use crate::model::HoldsProject;
use gpui::{
    AnyElement, App, Context, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, canvas, div,
    prelude::*, px,
};

use crate::{
    browser::BrowserDrag,
    geometry::Rect,
    icons,
    theme::{Theme, hsla, icon, icon_fill, mono, size},
    wave::paint_waveform,
    widgets::{on_press, paint_slider, toggle_button},
};
use tonique_core::{library::analysis::AudioInfo, project::PlaybackState, utils::format_time};

const PREVIEW_HEIGHT: f32 = 118.;
const WAVE_HEIGHT: f32 = 52.;
const VOLUME_WIDTH: f32 = 90.;
/// How opaque the part not played yet is.
const REST_OPACITY: f32 = 0.55;

/// The preview's gain for the slider at `volume`: a square law, as in
/// Tonique.
pub fn preview_gain(volume: f32) -> f32 {
    volume.clamp(0., 1.).powi(2)
}

/// `44.1 kHz · 24-bit · Stereo`.
fn format(audio: &AudioInfo) -> String {
    let rate = audio.sample_rate as f64 / 1000.;
    let rate = if rate.fract() == 0. {
        format!("{rate:.0} kHz")
    } else {
        format!("{rate:.1} kHz")
    };
    let channels = match audio.channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        count => format!("{count} ch"),
    };
    match audio.bit_depth {
        Some(bits) => format!("{rate} · {bits}-bit · {channels}"),
        None => format!("{rate} · {channels}"),
    }
}

/// `m:ss`.
fn short_time(seconds: f64) -> String {
    let mut text = format_time(seconds);
    text.truncate(text.len().saturating_sub(4));
    text
}

impl BrowserPanel {
    fn seek_preview_at(&mut self, pointer_x: Pixels, context: &mut App) {
        let Some(audio) = self.audio.clone() else {
            return;
        };
        let Some(frames) = audio.total_frames() else {
            return;
        };
        let bounds = self.wave_bounds.get();
        let fraction = (f32::from(pointer_x - bounds.origin.x)
            / f32::from(bounds.size.width).max(1.))
        .clamp(0., 1.);
        if self.state(context).previewing() != Some(audio.path.as_path()) {
            self.state_mut(context).play_preview(audio.path.clone());
        }
        self.state_mut(context)
            .seek_preview((fraction as f64 * frames) as usize);
    }

    fn set_preview_volume_at(&mut self, pointer_x: Pixels, context: &mut App) {
        let bounds = self.volume_bounds.get();
        self.volume = (f32::from(pointer_x - bounds.origin.x)
            / f32::from(bounds.size.width).max(1.))
        .clamp(0., 1.);
        self.muted = false;
        self.apply_preview_gain(context);
    }

    fn apply_preview_gain(&mut self, context: &mut App) {
        let gain = if self.muted {
            0.
        } else {
            preview_gain(self.volume)
        };
        self.state_mut(context).set_preview_gain(gain);
    }

    pub fn browser_drag_move(&mut self, event: &MouseMoveEvent, context: &mut App) -> bool {
        match self.drag {
            BrowserDrag::None => return false,
            BrowserDrag::Seek => self.seek_preview_at(event.position.x, context),
            BrowserDrag::Volume => self.set_preview_volume_at(event.position.x, context),
        }
        true
    }

    /// The preview's drag let go: the volume is saved once, not on every
    /// step of it.
    pub fn browser_drag_end(&mut self) {
        let browser = &mut *self;
        if browser.drag == BrowserDrag::Volume && browser.config.preview_volume() != browser.volume
        {
            browser.config.set_preview_volume(browser.volume);
        }
        browser.drag = BrowserDrag::None;
    }

    /// Nothing when no audio file is selected.
    pub(super) fn preview_view(&mut self, context: &mut Context<Self>) -> Option<AnyElement> {
        let audio = self.audio.clone()?;
        let theme = crate::theme::current(context);
        let panel = div()
            .h(px(PREVIEW_HEIGHT))
            .flex_none()
            .flex()
            .border_t_1()
            .border_color(hsla(theme.separator));
        let this_file = self.state(context).previewing() == Some(audio.path.as_path());
        let position = if this_file {
            self.state(context).preview_position() as f64
        } else {
            0.
        };
        let seconds = |frame_count: f64| frame_count / audio.sample_rate.max(1) as f64;
        let length = audio.total_frames().map_or("…".to_string(), |frame_count| {
            short_time(seconds(frame_count))
        });
        let title = div()
            .flex()
            .items_baseline()
            .gap_2()
            .px_2()
            .pt_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(audio.name.clone()),
            )
            .child(
                div()
                    .text_size(px(size::SMALL))
                    .font(mono())
                    .text_color(hsla(theme.text_muted))
                    .child(format!("{} / {}", short_time(seconds(position)), length)),
            );
        panel
            .flex_col()
            .gap_1()
            .mb(px(4.0))
            .bg(hsla(theme.bg_panel))
            .child(title)
            .child(
                div()
                    .px_2()
                    .text_size(px(size::SMALL))
                    .text_color(hsla(theme.text_muted))
                    .child(format(&audio)),
            )
            .child(self.preview_waveform(&audio, position, &theme, context))
            .child(self.preview_controls(&audio, &theme, context))
            .into_any_element()
            .into()
    }

    /// The waveform, the part played bright: pressed or dragged, it seeks.
    fn preview_waveform(
        &self,
        audio: &AudioInfo,
        position: f64,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let waves = self.waves.clone();
        let slot = self.wave_bounds.clone();
        let canvas_theme = theme.clone();
        let data = audio.data.clone();
        let stereo = audio.channels >= 2;
        let frames = audio.total_frames();
        div()
            .h(px(WAVE_HEIGHT))
            .mx_2()
            .rounded(px(3.))
            .bg(hsla(theme.bg_deep))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    this.drag = BrowserDrag::Seek;
                    this.seek_preview_at(event.position.x, context);
                    context.notify();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, _| {
                        let Some(frames) = frames else { return };
                        let rect = Rect::of(bounds);
                        let rect =
                            Rect::new(rect.left, rect.top + 3., rect.width, rect.height - 6.);
                        let at = rect.left + (position / frames) as f32 * rect.width;
                        let mut waves = waves.borrow_mut();
                        let played = Rect::new(rect.left, rect.top, at - rect.left, rect.height);
                        let rest = Rect::new(at, rect.top, rect.right() - at, rect.height);
                        let parts = [
                            (played, hsla(canvas_theme.accent)),
                            (rest, hsla(canvas_theme.text_muted).opacity(REST_OPACITY)),
                        ];
                        for (part, color) in parts {
                            paint_waveform(
                                window,
                                &mut waves,
                                rect,
                                part,
                                &data,
                                0.0..frames,
                                stereo,
                                color,
                                1.,
                                0.,
                            );
                        }
                        if position > 0. {
                            window.paint_quad(gpui::fill(
                                Rect::new(at, rect.top - 3., 1., rect.height + 6.).bounds(),
                                hsla(canvas_theme.playhead),
                            ));
                        }
                    },
                )
                .size_full(),
            )
    }

    /// Play, auto-preview, and the preview's volume with its mute.
    fn preview_controls(
        &self,
        audio: &AudioInfo,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let playing = self.state(context).preview_playback_state() == PlaybackState::Playing
            && self.state(context).previewing() == Some(audio.path.as_path());
        let path = audio.path.clone();
        let play = on_press(
            div()
                .id("preview-play")
                .w(px(28.))
                .h(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .bg(hsla(if playing {
                    theme.accent
                } else {
                    theme.bg_control
                }))
                .text_color(hsla(if playing {
                    theme.text_on_accent
                } else {
                    theme.text
                }))
                .hover(|style| style.opacity(0.9))
                .cursor_pointer()
                .child(icon_fill(if playing { icons::STOP } else { icons::PLAY })),
            context,
            move |this, context| {
                if playing {
                    this.state_mut(context).pause_preview();
                } else {
                    this.state_mut(context).play_preview(path.clone());
                }
            },
        );
        let silent = self.muted || self.volume <= 0.;
        let mute = on_press(
            div()
                .id("preview-mute")
                .text_size(px(size::HEADING))
                .text_color(hsla(if self.muted {
                    theme.warning
                } else {
                    theme.text_muted
                }))
                .hover(|style| style.opacity(0.8))
                .cursor_pointer()
                .child(icon(if silent {
                    icons::SPEAKER_SLASH
                } else {
                    icons::SPEAKER_LOW
                })),
            context,
            |this, context| {
                this.muted = !this.muted;
                this.apply_preview_gain(context);
            },
        );
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .child(play)
            .child(toggle_button(
                "auto-preview",
                icon(icons::HEADPHONES),
                self.auto_preview,
                theme,
                context,
                |this, _| {
                    let browser = &mut *this;
                    browser.auto_preview = !browser.auto_preview;
                    browser.config.set_auto_preview(browser.auto_preview);
                },
            ))
            .child(div().flex_1())
            .child(mute)
            .child(self.preview_volume(theme, context))
    }

    fn preview_volume(&self, theme: &Theme, context: &mut Context<Self>) -> impl IntoElement {
        let slot = self.volume_bounds.clone();
        let volume = self.volume;
        let canvas_theme = theme.clone();
        div()
            .w(px(VOLUME_WIDTH))
            .h(px(14.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    this.drag = BrowserDrag::Volume;
                    this.set_preview_volume_at(event.position.x, context);
                    context.notify();
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    move |bounds, _, window, _| {
                        paint_slider(window, Rect::of(bounds), volume, &canvas_theme)
                    },
                )
                .size_full(),
            )
    }
}
