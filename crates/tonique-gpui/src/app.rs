//! The window: transport bar on top, the browser on the left, the track
//! headers and the timeline on the right. One view owns Tonique's
//! `ProjectState`, as `ToniqueApp` does in the egui app.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{
    Context, FocusHandle, KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, Render, Task,
    Window, div, prelude::*, px,
};
use tonique_engine::{engine::Engine, time::BeatPos};

use crate::{
    audio::host::AudioHost,
    bench::Bench,
    browser::Browser,
    config::settings::Settings,
    core::state::{PlaybackState, ProjectState},
    headers::HeadersUi,
    icons,
    theme::{FONT_UI, Theme, h, icon, icon_fill, size, with_alpha},
    timeline::TimelineUi,
    utils::{format_bars, format_time},
    wave::WaveCache,
    widgets::Flat,
};

pub struct Tonique {
    pub state: ProjectState,
    pub theme: Theme,
    pub focus: FocusHandle,
    pub timeline: TimelineUi,
    pub headers: HeadersUi,
    pub browser: Browser,
    pub waves: Rc<RefCell<WaveCache>>,
    /// Whether the browser shows, as Tonique's left panel.
    pub browser_open: bool,
    /// `TONIQUE_BENCH`: steers the view and measures frames.
    bench: Option<Bench>,
    _ticker: Task<()>,
}

impl Tonique {
    pub fn new(
        engine: Engine,
        audio: Result<AudioHost, String>,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = Theme::dark();
        let mut state = ProjectState::new(engine);
        state.set_track_palette(&theme.palette);
        state.attach_audio(audio, settings);
        let mut bench = Bench::from_env("gpui-fast");
        match &mut bench {
            Some(bench) => bench.setup(&mut state),
            None => {
                state.add_track();
                state.add_track();
                state.grid.fit(BeatPos(0.), BeatPos(64.), 1000.);
            }
        }

        // Tonique's state polls the engine and the file loaders each frame;
        // when nothing animates, ten times a second is enough.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });

        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            browser: Browser::new(&mut state, cx),
            state,
            theme,
            focus,
            timeline: TimelineUi::default(),
            headers: HeadersUi::default(),
            waves: Rc::default(),
            browser_open: true,
            bench,
            _ticker: ticker,
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control || k.modifiers.platform;
        match k.key.as_str() {
            "space" => self.toggle_play(),
            "delete" | "backspace" => self.state.delete_selected_clips(),
            "z" if ctrl && k.modifiers.shift => self.state.redo(),
            "z" if ctrl => self.state.undo(),
            "y" if ctrl => self.state.redo(),
            "d" if ctrl => self.state.duplicate_selected_clips(),
            "a" if ctrl => self.state.select_all_clips(),
            "l" if !ctrl => self.state.set_looping(!self.state.looping()),
            "b" if ctrl => self.browser_open = !self.browser_open,
            "t" if ctrl => {
                self.state.add_track_below_selection();
            }
            "f" if ctrl && k.modifiers.shift => {
                let follow = self.state.follow_playhead();
                self.state.set_follow_playhead(!follow)
            }
            "home" => self.state.seek(BeatPos::ZERO),
            "escape" => self.state.clear_clip_selection(),
            _ => {
                if self.browser.list_focus.is_focused(window) {
                    self.browser_key(ev, cx);
                }
                return;
            }
        }
        cx.notify();
    }

    fn toggle_play(&mut self) {
        if self.state.playback_state() == PlaybackState::Playing {
            self.state.stop();
        } else {
            self.state.play();
        }
    }

    /// Drags follow the pointer anywhere in the window.
    fn on_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let changed =
            self.timeline_drag_move(ev) | self.header_drag_move(ev) | self.browser_drag_move(ev);
        if changed || self.timeline_hover(ev) {
            cx.notify();
        }
    }

    fn on_up(&mut self, ev: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.timeline_drag_end(ev);
        self.header_drag_end();
        self.browser.drag = Default::default();
        cx.notify();
    }

    /// As Tonique's transport bar: the browser and undo on the left, the
    /// transport centred, the grid, the song's tempo and the audio load on
    /// the right.
    fn transport(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let s = &self.state;
        let playing = s.playback_state() == PlaybackState::Playing;
        let gap = |w: f32| div().flex_none().w(px(w - bar::BUTTON_GAP));

        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(bar::BUTTON_GAP))
            .child(
                Flat::ghost("browser", bar::BUTTON)
                    .selected(self.browser_open)
                    .show(
                        glyph(icons::SIDEBAR_SIMPLE, 15., self.browser_open),
                        &t,
                        cx,
                        |this, _| this.browser_open = !this.browser_open,
                    ),
            )
            .child(gap(bar::GROUP_GAP))
            .child(Flat::ghost("undo", bar::BUTTON).enabled(s.can_undo()).show(
                glyph(icons::ARROW_U_UP_LEFT, 15., false),
                &t,
                cx,
                |this, _| this.state.undo(),
            ))
            .child(Flat::ghost("redo", bar::BUTTON).enabled(s.can_redo()).show(
                glyph(icons::ARROW_U_UP_RIGHT, 15., false),
                &t,
                cx,
                |this, _| this.state.redo(),
            ));

        // Blinks on the beat.
        let metronome = s.metronome();
        let click = metronome && playing && s.playhead().0 % 1.0 < 0.5;
        let metronome_color = match (metronome, click) {
            (false, _) => t.text,
            (true, false) => t.text_on_accent,
            (true, true) => with_alpha(t.text_on_accent, 110),
        };
        let looping = s.looping();
        let follow = s.follow_playhead();
        let (bars, time) = {
            let beats = s.playhead().0.max(0.);
            (
                format_bars(beats, s.time_signature()),
                format_time(beats * 60. / s.bpm() as f64),
            )
        };
        let center = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(bar::BUTTON_GAP))
            .child(
                Flat::new("metronome", bar::BUTTON)
                    .selected(metronome)
                    .color(metronome_color)
                    .show(
                        glyph(icons::METRONOME, 15., metronome),
                        &t,
                        cx,
                        |this, _| this.state.toggle_metronome(),
                    ),
            )
            .child(Flat::new("play", bar::BUTTON).selected(playing).show(
                glyph(if playing { icons::STOP } else { icons::PLAY }, 12., true),
                &t,
                cx,
                |this, _| this.toggle_play(),
            ))
            .child(Flat::new("loop", bar::BUTTON).selected(looping).show(
                glyph(icons::REPEAT, 14., looping),
                &t,
                cx,
                |this, _| {
                    let looping = this.state.looping();
                    this.state.set_looping(!looping)
                },
            ))
            .child(Flat::new("follow", bar::BUTTON).selected(follow).show(
                glyph(icons::CARET_LINE_RIGHT, 14., follow),
                &t,
                cx,
                |this, _| {
                    let follow = this.state.follow_playhead();
                    this.state.set_follow_playhead(!follow)
                },
            ))
            .child(gap(bar::GROUP_GAP))
            .child(
                well(bar::POSITION_WIDTH, &t)
                    .justify_between()
                    .px(px(8.))
                    .child(bars)
                    .child(div().text_color(h(t.text_muted)).child(time)),
            );

        // The grid's step, muted while adaptive, dimmed further while edits
        // don't snap.
        let grid = &s.grid;
        let (step, step_color) = if grid.settings.adaptive {
            (grid.line_step(), t.text_muted)
        } else {
            (grid.snap_step(), t.text)
        };
        let step_color = if grid.settings.snap {
            step_color
        } else {
            t.text_disabled
        };
        let signature = s.time_signature();
        let field = |w: f32, text: String| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(w))
                .h_full()
                .rounded(px(1.))
                .hover(|st| st.bg(h(t.hover_overlay)))
                .child(text)
        };
        let song = well(bar::SONG_WIDTH, &t)
            .child(field(bar::TEMPO_WIDTH, format!("{:.1} BPM", s.bpm())))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .w(px(bar::TEMPO_DIVIDER_WIDTH))
                    .child(div().w(px(1.)).h(px(bar::BUTTON - 8.)).bg(h(t.separator))),
            )
            .child(field(
                bar::SIGNATURE_FIELD_WIDTH,
                signature.numerator.to_string(),
            ))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .w(px(bar::SIGNATURE_SLASH_WIDTH))
                    .text_color(h(t.text_muted))
                    .child("/"),
            )
            .child(field(
                bar::SIGNATURE_FIELD_WIDTH,
                signature.denominator.to_string(),
            ));
        let load = s.metrics.latency;
        let load_color = if load >= bar::USAGE_DANGER {
            t.danger
        } else if load >= bar::USAGE_WARNING {
            t.warning
        } else {
            t.text_muted
        };
        let missing = !s.missing_files().is_empty();
        let right = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(bar::BUTTON_GAP))
            .child(
                div()
                    .flex_none()
                    .w(px(bar::GRID_STEP_WIDTH))
                    .h(px(bar::BUTTON))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(1.))
                    .bg(h(t.bg_control))
                    .text_size(px(size::SMALL))
                    .text_color(h(step_color))
                    .child(grid.step_name(step)),
            )
            .child(gap(bar::GROUP_GAP + bar::BUTTON_GAP))
            .child(song)
            .child(gap(bar::GROUP_GAP + bar::BUTTON_GAP))
            .when(missing, |el| {
                el.child(
                    Flat::new("missing-files", bar::BUTTON)
                        .color(t.warning)
                        .show(glyph(icons::WARNING, 14., false), &t, cx, |_, _| {}),
                )
            })
            .when(s.audio_lost(), |el| {
                el.child(Flat::new("no-audio", bar::BUTTON).color(t.danger).show(
                    glyph(icons::SPEAKER_SLASH, 14., false),
                    &t,
                    cx,
                    |_, _| {},
                ))
            })
            .child(
                div()
                    .flex_none()
                    .w(px(bar::USAGE_WIDTH))
                    .h(px(bar::BUTTON))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .pl(px(bar::USAGE_PADDING))
                    .text_size(px(size::SMALL))
                    .text_color(h(load_color))
                    .child(icon(icons::CPU))
                    .child(format!("{:.0}%", (load * 100.).round())),
            );

        div()
            .flex()
            .flex_none()
            .items_center()
            .p(px(4.))
            .bg(h(t.bg_panel))
            .border_b_1()
            .border_color(h(t.separator))
            .child(left)
            .child(div().flex_none().w(px(bar::GROUP_GAP)))
            .child(center)
            .child(div().flex_none().w(px(bar::GROUP_GAP)))
            .child(right)
    }

    fn status_bar(&self) -> impl IntoElement {
        let t = &self.theme;
        let audio = match (self.state.audio(), &self.state.audio_error) {
            (Some(a), _) => format!("{} · {} Hz", a.device_name, a.sample_rate),
            (None, Some(e)) => format!("No audio: {e}"),
            (None, None) => "No audio".into(),
        };
        div()
            .flex()
            .gap_4()
            .px_3()
            .h(px(22.))
            .flex_none()
            .items_center()
            .bg(h(t.bg_panel))
            .border_t_1()
            .border_color(h(t.separator))
            .text_size(px(size::SMALL))
            .text_color(h(t.text_muted))
            .child(div().flex().items_center().gap_1().child(icon(if self.state.audio().is_some() { icons::SPEAKER_HIGH } else { icons::SPEAKER_SLASH })).child(audio))
            .child(div().flex_1())
            .child("Space play/stop · drag a file from the browser or your file manager · Ctrl+wheel zoom · Shift+wheel scroll · Alt: no snap · Del · Ctrl+Z/D")
    }
}

impl Render for Tonique {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(bench) = &mut self.bench {
            bench.frame(&mut self.state);
            // Every frame, as the egui app draws.
            window.request_animation_frame();
        }
        crate::profile::frame();
        crate::profile::time("state.update", || self.state.update());
        self.waves.borrow_mut().start_frame(window);
        let animating = self.state.playback_state() == PlaybackState::Playing
            || self.state.preview_playback_state() == PlaybackState::Playing
            || cx.has_active_drag()
            || self.timeline.is_dragging()
            // Meters falling back after playback.
            || self.state.metrics.tracks.values().any(crate::meter::is_live);
        if animating {
            window.request_animation_frame();
        }
        if !cx.has_active_drag() {
            self.timeline.drop_target = None;
        }

        let t = self.theme.clone();
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_up))
            .size_full()
            .flex()
            .flex_col()
            .bg(h(t.bg_deep))
            .text_color(h(t.text))
            .font_family(FONT_UI)
            .text_size(px(size::BODY))
            .child(self.transport(cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.browser_open, |el| {
                        el.child(crate::profile::time("view.browser", || {
                            self.browser_view(window, cx)
                        }))
                        .child(div().w(px(1.)).h_full().bg(h(t.separator)))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .child(crate::profile::time("view.headers", || {
                                self.headers_view(cx)
                            }))
                            .child(div().w(px(1.)).h_full().bg(h(t.separator)))
                            .child(crate::profile::time("view.timeline", || {
                                self.timeline_view(window, cx)
                            })),
                    ),
            )
            .child(self.status_bar())
    }
}

/// The transport bar's sizes, as Tonique's.
mod bar {
    pub const BUTTON: f32 = 22.;
    /// Between the buttons of a group.
    pub const BUTTON_GAP: f32 = 2.;
    /// Between groups.
    pub const GROUP_GAP: f32 = 8.;
    /// The playhead's position: bars and time.
    pub const POSITION_WIDTH: f32 = 140.;
    pub const USAGE_WIDTH: f32 = 50.;
    pub const USAGE_PADDING: f32 = 4.;
    pub const USAGE_WARNING: f32 = 0.7;
    pub const USAGE_DANGER: f32 = 0.9;
    pub const TEMPO_WIDTH: f32 = 76.;
    pub const TEMPO_DIVIDER_WIDTH: f32 = 5.;
    pub const SIGNATURE_FIELD_WIDTH: f32 = 22.;
    pub const SIGNATURE_SLASH_WIDTH: f32 = 8.;
    pub const SONG_WIDTH: f32 =
        TEMPO_WIDTH + TEMPO_DIVIDER_WIDTH + 2. * SIGNATURE_FIELD_WIDTH + SIGNATURE_SLASH_WIDTH;
    pub const GRID_STEP_WIDTH: f32 = 48.;
}

/// An icon at `size`, filled or not.
fn glyph(g: &'static str, size: f32, fill: bool) -> gpui::Div {
    let el = if fill { icon_fill(g) } else { icon(g) };
    el.text_size(px(size)).line_height(px(size))
}

/// A sunken field `w` wide, as the position's and the tempo's.
fn well(w: f32, t: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(w))
        .h(px(bar::BUTTON))
        .rounded(px(2.))
        .bg(h(t.bg_deep))
}
