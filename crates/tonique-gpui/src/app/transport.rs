//! The transport bar, as Tonique's: the browser and undo on the left, the
//! transport centred, the grid's step, the song's tempo and time
//! signature, warnings and the audio load on the right.

use gpui::{
    Context, Entity, EventEmitter, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Window, div, point, prelude::*, px,
};
use tonique_engine::time::TimeSignature;

use crate::{
    app::menu_bar::MENU_BAR_HEIGHT,
    icons,
    model::{HoldsProject, Project},
    theme::{Theme, hsla, icon, size, with_alpha},
    widgets::{Flat, NumberField, NumberSpec, follow_drag, glyph},
};
use tonique_core::{
    config::keymap::Action,
    state::PlaybackState,
    utils::{format_bars, format_time},
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

/// The bar, a view of its own.
pub struct TransportBar {
    project: Entity<Project>,
    /// The song's fields: tempo, and the time signature's two numbers.
    bpm: NumberField,
    numerator: NumberField,
    denominator: NumberField,
    /// What the window shows, as its buttons light up.
    shows: Shows,
}

/// What the window around the bar shows, told by it.
#[derive(Clone, Copy, PartialEq, Default)]
pub struct Shows {
    pub browser: bool,
    pub graph: bool,
    pub grid_menu: bool,
}

/// What the bar asks of the window.
pub enum TransportEvent {
    ToggleBrowser,
    ToggleGraph,
    /// The grid's menu, its top right corner at this point.
    ToggleGridMenu(Point<Pixels>),
    ShowMissingFiles,
    OpenAudioSettings,
}

impl EventEmitter<TransportEvent> for TransportBar {}

impl HoldsProject for TransportBar {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl TransportBar {
    pub fn new(project: Entity<Project>) -> Self {
        Self {
            project,
            shows: Shows::default(),
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

    /// Tells the bar what the window shows.
    pub fn show(&mut self, shows: Shows, context: &mut Context<Self>) {
        if self.shows != shows {
            self.shows = shows;
            context.notify();
        }
    }

    fn is_dragging(&self) -> bool {
        self.bpm.is_dragging() || self.numerator.is_dragging() || self.denominator.is_dragging()
    }

    fn end_drags(&mut self, _: &MouseUpEvent, _: &mut Context<Self>) {
        self.bpm.end_drag();
        self.numerator.end_drag();
        self.denominator.end_drag();
    }
}

fn set_bpm(this: &mut TransportBar, bpm: f32, context: &mut Context<TransportBar>) {
    this.state_mut(context).set_bpm(bpm);
}

fn set_numerator(this: &mut TransportBar, numerator: f32, context: &mut Context<TransportBar>) {
    let current = this.state(context).time_signature();
    if let Some(signature) = TimeSignature::new(numerator.round() as u32, current.denominator) {
        this.state_mut(context).set_time_signature(signature);
    }
}

fn set_denominator(this: &mut TransportBar, denominator: f32, context: &mut Context<TransportBar>) {
    let current = this.state(context).time_signature();
    if let Some(signature) = TimeSignature::new(current.numerator, denominator as u32) {
        this.state_mut(context).set_time_signature(signature);
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

impl TransportBar {
    /// Follows a drag of the song's fields.
    fn drag_to(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        let pointer_x = f32::from(event.position.x);
        let fine = event.modifiers.shift;
        let signature = self.state(context).time_signature();
        let (numerator, denominator) = (signature.numerator as f32, signature.denominator as f32);
        let bpm = self.state(context).bpm();
        if let Some(bpm) = self.bpm.drag_to(pointer_x, fine, bpm) {
            set_bpm(self, bpm, context);
        } else if let Some(new_numerator) = self.numerator.drag_to(pointer_x, fine, numerator) {
            set_numerator(self, new_numerator, context);
        } else if let Some(new_denominator) = self.denominator.drag_to(pointer_x, fine, denominator)
        {
            set_denominator(self, new_denominator, context);
        }
    }
}

impl Render for TransportBar {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        // The playhead's time and the processor's load.
        self.watch_playback(context);
        self.watch_load(context);
        let theme = crate::theme::current(context);
        let dragging = self.is_dragging();
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
            // A field's drag follows the pointer anywhere in the window.
            .when(dragging, |element| {
                element.child(follow_drag(
                    context.weak_entity(),
                    Self::drag_to,
                    Self::end_drags,
                ))
            })
    }
}

impl TransportBar {
    fn transport_left(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let state = self.state(context);
        let (can_undo, can_redo) = (state.can_undo(), state.can_redo());
        let Shows {
            browser: open,
            graph,
            ..
        } = self.shows;
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
                |_, context| context.emit(TransportEvent::ToggleBrowser),
            ))
            // The audio graph is a development tool.
            .when(Action::ToggleGraphView.is_available(), |element| {
                element.child(Flat::ghost("graph", BUTTON).selected(graph).show(
                    glyph(icons::GRAPH, 15., graph),
                    theme,
                    context,
                    |_, context| context.emit(TransportEvent::ToggleGraph),
                ))
            })
            .child(gap(GROUP_GAP))
            .child(Flat::ghost("undo", BUTTON).enabled(can_undo).show(
                glyph(icons::ARROW_U_UP_LEFT, 15., false),
                theme,
                context,
                |this, context| this.state_mut(context).undo(),
            ))
            .child(Flat::ghost("redo", BUTTON).enabled(can_redo).show(
                glyph(icons::ARROW_U_UP_RIGHT, 15., false),
                theme,
                context,
                |this, context| this.state_mut(context).redo(),
            ))
    }

    fn transport_center(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let state = self.state(context);
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
                        |this, context| this.state_mut(context).toggle_metronome(),
                    ),
            )
            .child(Flat::new("play", BUTTON).selected(playing).show(
                glyph(if playing { icons::STOP } else { icons::PLAY }, 12., true),
                theme,
                context,
                |this, context| this.state_mut(context).toggle_play(),
            ))
            .child(Flat::new("loop", BUTTON).selected(looping).show(
                glyph(icons::REPEAT, 14., looping),
                theme,
                context,
                |this, context| {
                    let looping = this.state(context).looping();
                    this.state_mut(context).set_looping(!looping)
                },
            ))
            .child(Flat::new("follow", BUTTON).selected(follow).show(
                glyph(icons::CARET_LINE_RIGHT, 14., follow),
                theme,
                context,
                |this, context| {
                    let follow = this.state(context).follow_playhead();
                    this.state_mut(context).set_follow_playhead(!follow)
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
        let state = self.state(context);
        let load = state.metrics().latency;
        let load_color = if load >= USAGE_DANGER {
            theme.danger
        } else if load >= USAGE_WARNING {
            theme.warning
        } else {
            theme.text_muted
        };
        let missing = !state.missing_files().is_empty();
        let audio_lost = state.audio_lost();
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
                            context.listener(|_, _, _, context| {
                                context.emit(TransportEvent::ShowMissingFiles);
                            }),
                        ),
                )
            })
            .when(audio_lost, |element| {
                element.child(
                    Flat::new("no-audio", BUTTON)
                        .color(theme.danger)
                        .element(glyph(icons::SPEAKER_SLASH, 14., false), theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(|_, _, _, context| {
                                context.emit(TransportEvent::OpenAudioSettings);
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
        let grid = &self.state(context).grid;
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
        let open = self.shows.grid_menu;
        Flat::new("grid-step", BUTTON)
            .width(GRID_STEP_WIDTH)
            .color(color)
            .element(grid.step_name(step), theme)
            .text_size(px(size::SMALL))
            .when(open, |element| element.bg(hsla(theme.bg_control_hover)))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|_, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let below = px(MENU_BAR_HEIGHT + BAR_HEIGHT);
                    context.emit(TransportEvent::ToggleGridMenu(point(
                        event.position.x,
                        below,
                    )));
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
        let state = self.state(context);
        let (bpm, signature) = (state.bpm(), state.time_signature());
        let field = (SIGNATURE_FIELD_WIDTH, BUTTON);
        well(SONG_WIDTH, theme)
            .child(self.bpm.view(
                "bpm",
                (bpm, crate::app::arrows::holds_number("bpm", context)),
                (TEMPO_WIDTH, BUTTON),
                theme,
                window,
                context,
                |this| &mut this.bpm,
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
            .child(self.numerator.view(
                "numerator",
                (
                    signature.numerator as f32,
                    crate::app::arrows::holds_number("numerator", context),
                ),
                field,
                theme,
                window,
                context,
                |this| &mut this.numerator,
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
            .child(self.denominator.view(
                "denominator",
                (
                    signature.denominator as f32,
                    crate::app::arrows::holds_number("denominator", context),
                ),
                field,
                theme,
                window,
                context,
                |this| &mut this.denominator,
                set_denominator,
            ))
    }
}
