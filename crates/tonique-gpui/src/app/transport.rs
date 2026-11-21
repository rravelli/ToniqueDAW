//! The transport bar, as Tonique's: the browser and undo on the left, the
//! transport centred, the grid's step, the song's tempo and time
//! signature, warnings and the audio load on the right.

use gpui::{
    Anchor, Context, MouseButton, MouseDownEvent, MouseMoveEvent, Window, div, point, prelude::*,
    px,
};
use tonique_engine::time::TimeSignature;

use crate::{
    app::{PopupKind, Tonique, menu_bar::MENU_BAR_HEIGHT, settings::SettingsTab},
    core::state::PlaybackState,
    icons,
    theme::{Theme, hsla, icon, size, with_alpha},
    utils::{format_bars, format_time},
    widgets::{Flat, NumberField, NumberSpec, glyph},
};

const BUTTON: f32 = 22.;
/// Between the buttons of a group.
const BUTTON_GAP: f32 = 2.;
/// Between groups.
const GROUP_GAP: f32 = 8.;
/// The bar's height: its buttons and padding, and the line under it.
const BAR_HEIGHT: f32 = BUTTON + 2. * 4. + 1.;
/// The playhead's position: bars and time.
const POSITION_WIDTH: f32 = 140.;
const USAGE_WIDTH: f32 = 50.;
const USAGE_PADDING: f32 = 4.;
const USAGE_WARNING: f32 = 0.7;
const USAGE_DANGER: f32 = 0.9;
const TEMPO_WIDTH: f32 = 76.;
const TEMPO_DIVIDER_WIDTH: f32 = 5.;
const SIGNATURE_FIELD_WIDTH: f32 = 22.;
const SIGNATURE_SLASH_WIDTH: f32 = 8.;
const SONG_WIDTH: f32 =
    TEMPO_WIDTH + TEMPO_DIVIDER_WIDTH + 2. * SIGNATURE_FIELD_WIDTH + SIGNATURE_SLASH_WIDTH;
const GRID_STEP_WIDTH: f32 = 48.;

/// The song's fields: tempo, and the time signature's two numbers.
pub struct Transport {
    bpm: NumberField,
    numerator: NumberField,
    denominator: NumberField,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            bpm: NumberField::new(NumberSpec::range(10., 1000., 1).suffix(" BPM")),
            numerator: NumberField::new(NumberSpec::range(
                1.,
                TimeSignature::MAX_NUMERATOR as f32,
                0,
            )),
            denominator: NumberField::new(NumberSpec::values(
                TimeSignature::DENOMINATORS.map(|value| value as f32),
            )),
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.bpm.is_dragging() || self.numerator.is_dragging() || self.denominator.is_dragging()
    }

    pub fn end_drags(&mut self) {
        self.bpm.end_drag();
        self.numerator.end_drag();
        self.denominator.end_drag();
    }
}

fn set_bpm(this: &mut Tonique, bpm: f32) {
    this.state.set_bpm(bpm);
}

fn set_numerator(this: &mut Tonique, numerator: f32) {
    let current = this.state.time_signature();
    if let Some(signature) = TimeSignature::new(numerator.round() as u32, current.denominator) {
        this.state.set_time_signature(signature);
    }
}

fn set_denominator(this: &mut Tonique, denominator: f32) {
    let current = this.state.time_signature();
    if let Some(signature) = TimeSignature::new(current.numerator, denominator as u32) {
        this.state.set_time_signature(signature);
    }
}

/// A sunken field `width` wide, as the position's and the tempo's.
fn well(width: f32, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(width))
        .h(px(BUTTON))
        .rounded(px(2.))
        .bg(hsla(theme.bg_deep))
}

/// Space between groups (less the gap the row adds).
fn gap(width: f32) -> gpui::Div {
    div().flex_none().w(px(width - BUTTON_GAP))
}

/// Opens `kind`'s menu under the button pressed by `event`.
fn open_below(this: &mut Tonique, kind: PopupKind, event: &MouseDownEvent) {
    let below = px(MENU_BAR_HEIGHT + BAR_HEIGHT);
    this.toggle_popup(kind, point(event.position.x, below), Anchor::TopRight);
}

impl Tonique {
    /// Follows a drag of the song's fields. Returns whether it's one.
    pub(super) fn transport_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        let pointer_x = f32::from(event.position.x);
        let fine = event.modifiers.shift;
        let signature = self.state.time_signature();
        let (numerator, denominator) = (signature.numerator as f32, signature.denominator as f32);
        if let Some(bpm) = self
            .transport
            .bpm
            .drag_to(pointer_x, fine, self.state.bpm())
        {
            set_bpm(self, bpm);
        } else if let Some(new_numerator) =
            self.transport.numerator.drag_to(pointer_x, fine, numerator)
        {
            set_numerator(self, new_numerator);
        } else if let Some(new_denominator) =
            self.transport
                .denominator
                .drag_to(pointer_x, fine, denominator)
        {
            set_denominator(self, new_denominator);
        } else {
            return self.transport.is_dragging();
        }
        true
    }

    pub(super) fn transport(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme.clone();
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(BAR_HEIGHT))
            .px(px(4.))
            .bg(hsla(theme.bg_panel))
            .border_b_1()
            .border_color(hsla(theme.separator))
            .child(self.transport_left(&theme, context))
            .child(div().flex_none().w(px(GROUP_GAP)))
            .child(self.transport_center(&theme, context))
            .child(div().flex_none().w(px(GROUP_GAP)))
            .child(self.transport_right(&theme, window, context))
    }

    fn transport_left(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let state = &self.state;
        let open = self.browser_open;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(BUTTON_GAP))
            .child(Flat::ghost("browser", BUTTON).selected(open).show(
                glyph(icons::SIDEBAR_SIMPLE, 15., open),
                theme,
                context,
                |this, _| this.browser_open = !this.browser_open,
            ))
            .child(gap(GROUP_GAP))
            .child(Flat::ghost("undo", BUTTON).enabled(state.can_undo()).show(
                glyph(icons::ARROW_U_UP_LEFT, 15., false),
                theme,
                context,
                |this, _| this.state.undo(),
            ))
            .child(Flat::ghost("redo", BUTTON).enabled(state.can_redo()).show(
                glyph(icons::ARROW_U_UP_RIGHT, 15., false),
                theme,
                context,
                |this, _| this.state.redo(),
            ))
    }

    fn transport_center(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let state = &self.state;
        let playing = state.playback_state() == PlaybackState::Playing;
        // Blinks on the beat.
        let metronome = state.metronome();
        let click = metronome && playing && state.playhead().0 % 1.0 < 0.5;
        let metronome_color = match (metronome, click) {
            (false, _) => theme.text,
            (true, false) => theme.text_on_accent,
            (true, true) => with_alpha(theme.text_on_accent, 110),
        };
        let looping = state.looping();
        let follow = state.follow_playhead();
        let beats = state.playhead().0.max(0.);
        let bars = format_bars(beats, state.time_signature());
        let time = format_time(beats * 60. / state.bpm() as f64);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(BUTTON_GAP))
            .child(
                Flat::new("metronome", BUTTON)
                    .selected(metronome)
                    .color(metronome_color)
                    .show(
                        glyph(icons::METRONOME, 15., metronome),
                        theme,
                        context,
                        |this, _| this.state.toggle_metronome(),
                    ),
            )
            .child(Flat::new("play", BUTTON).selected(playing).show(
                glyph(if playing { icons::STOP } else { icons::PLAY }, 12., true),
                theme,
                context,
                |this, _| this.toggle_play(),
            ))
            .child(Flat::new("loop", BUTTON).selected(looping).show(
                glyph(icons::REPEAT, 14., looping),
                theme,
                context,
                |this, _| {
                    let looping = this.state.looping();
                    this.state.set_looping(!looping)
                },
            ))
            .child(Flat::new("follow", BUTTON).selected(follow).show(
                glyph(icons::CARET_LINE_RIGHT, 14., follow),
                theme,
                context,
                |this, _| {
                    let follow = this.state.follow_playhead();
                    this.state.set_follow_playhead(!follow)
                },
            ))
            .child(gap(GROUP_GAP))
            .child(
                well(POSITION_WIDTH, theme)
                    .justify_between()
                    .px(px(8.))
                    .child(bars)
                    .child(div().text_color(hsla(theme.text_muted)).child(time)),
            )
    }

    fn transport_right(
        &self,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let state = &self.state;
        let load = state.metrics.latency;
        let load_color = if load >= USAGE_DANGER {
            theme.danger
        } else if load >= USAGE_WARNING {
            theme.warning
        } else {
            theme.text_muted
        };
        let missing = !state.missing_files().is_empty();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(BUTTON_GAP))
            .child(self.grid_step(theme, context))
            .child(gap(GROUP_GAP + BUTTON_GAP))
            .child(self.song(theme, window, context))
            .child(gap(GROUP_GAP + BUTTON_GAP))
            .when(missing, |element| {
                element.child(
                    Flat::new("missing-files", BUTTON)
                        .color(theme.warning)
                        .element(glyph(icons::WARNING, 14., false), theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(|this, _, _, context| {
                                this.show_missing_files(None);
                                context.notify();
                            }),
                        ),
                )
            })
            .when(state.audio_lost(), |element| {
                element.child(
                    Flat::new("no-audio", BUTTON)
                        .color(theme.danger)
                        .element(glyph(icons::SPEAKER_SLASH, 14., false), theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(|this, _, _, context| {
                                this.open_settings(SettingsTab::Audio);
                                context.notify();
                            }),
                        ),
                )
            })
            .child(
                div()
                    .flex_none()
                    .w(px(USAGE_WIDTH))
                    .h(px(BUTTON))
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .pl(px(USAGE_PADDING))
                    .text_size(px(size::SMALL))
                    .text_color(hsla(load_color))
                    .child(icon(icons::CPU))
                    .child(format!("{:.0}%", (load * 100.).round())),
            )
    }

    /// The grid's step, muted while adaptive, dimmed further while edits
    /// don't snap; pressed, the grid's menu.
    fn grid_step(&self, theme: &Theme, context: &mut Context<Self>) -> impl IntoElement {
        let grid = &self.state.grid;
        let (step, color) = if grid.settings.adaptive {
            (grid.line_step(), theme.text_muted)
        } else {
            (grid.snap_step(), theme.text)
        };
        let color = if grid.settings.snap {
            color
        } else {
            theme.text_disabled
        };
        let open = self
            .popup
            .as_ref()
            .is_some_and(|popup| popup.kind == PopupKind::Grid);
        Flat::new("grid-step", BUTTON)
            .width(GRID_STEP_WIDTH)
            .color(color)
            .element(grid.step_name(step), theme)
            .text_size(px(size::SMALL))
            .when(open, |element| element.bg(hsla(theme.bg_control_hover)))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    open_below(this, PopupKind::Grid, event);
                    context.notify();
                }),
            )
    }

    /// The tempo and time signature: each dragged, or double-clicked to
    /// type.
    fn song(
        &self,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let state = &self.state;
        let transport = &self.transport;
        let signature = state.time_signature();
        let field = (SIGNATURE_FIELD_WIDTH, BUTTON);
        well(SONG_WIDTH, theme)
            .child(transport.bpm.view(
                "bpm",
                state.bpm(),
                (TEMPO_WIDTH, BUTTON),
                theme,
                window,
                context,
                |this| &mut this.transport.bpm,
                set_bpm,
            ))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .w(px(TEMPO_DIVIDER_WIDTH))
                    .child(div().w(px(1.)).h(px(BUTTON - 8.)).bg(hsla(theme.separator))),
            )
            .child(transport.numerator.view(
                "numerator",
                signature.numerator as f32,
                field,
                theme,
                window,
                context,
                |this| &mut this.transport.numerator,
                set_numerator,
            ))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .w(px(SIGNATURE_SLASH_WIDTH))
                    .text_color(hsla(theme.text_muted))
                    .child("/"),
            )
            .child(transport.denominator.view(
                "denominator",
                signature.denominator as f32,
                field,
                theme,
                window,
                context,
                |this| &mut this.transport.denominator,
                set_denominator,
            ))
    }
}
