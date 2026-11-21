//! The window: the menu bar and the transport bar on top, the browser on
//! the left, the timeline and the track list on its right, the bottom
//! panel under them, a status bar below. One view
//! owns Tonique's `ProjectState`, as `ToniqueApp` does in the egui app.

mod dialog;
mod export;
mod keys;
mod menu_bar;
mod menus;
mod popup;
mod project;
mod settings;
mod status;
mod transport;

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{
    Context, DispatchPhase, FocusHandle, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Render, Subscription, Task, Window, canvas, div, prelude::*, px,
};
use tonique_engine::{engine::Engine, time::BeatPos};

use dialog::Dialog;
use export::ExportUi;
use menu_bar::MenuBarUi;
pub use popup::{Popup, PopupKind};
use project::ProjectFiles;
use settings::SettingsUi;
use transport::Transport;

use crate::{
    audio::host::AudioHost,
    bench::Bench,
    bottom::BottomPanel,
    browser::Browser,
    config::settings::Settings,
    core::state::{PlaybackState, ProjectState},
    headers::HeadersUi,
    theme::{FONT_UI, Theme, ThemeLibrary, hsla, size},
    timeline::TimelineUi,
    wave::WaveCache,
    widgets::{Edge, Splitter, splitter_handle},
};

/// The browser's width, as the egui app's left panel.
const BROWSER_WIDTH: f32 = 220.;
const BROWSER_MIN: f32 = 100.;
const BROWSER_MAX: f32 = 400.;

pub struct Tonique {
    pub state: ProjectState,
    pub theme: Theme,
    pub focus: FocusHandle,
    pub timeline: TimelineUi,
    pub headers: HeadersUi,
    pub browser: Browser,
    pub waves: Rc<RefCell<WaveCache>>,
    /// Whether the browser shows, as Tonique's left panel, and how wide.
    pub browser_open: bool,
    pub browser_width: Splitter,
    /// The selected track's effects, or a clip's details, at the bottom.
    pub bottom: BottomPanel,
    transport: Transport,
    menu_bar: MenuBarUi,
    /// The project's file, and the recent ones.
    project: ProjectFiles,
    /// The dialog open, if any, over everything.
    dialog: Option<Dialog>,
    settings: SettingsUi,
    /// Exporting audio: what's chosen, and the export running.
    export: ExportUi,
    /// The menu open, if any.
    pub popup: Option<Popup>,
    /// `TONIQUE_BENCH`: steers the view and measures frames.
    bench: Option<Bench>,
    _ticker: Task<()>,
    /// The browser reads its folders again when the window gets the focus
    /// back.
    _activation: Subscription,
}

impl Tonique {
    pub fn new(
        engine: Engine,
        audio: Result<AudioHost, String>,
        settings: Settings,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> Self {
        let themes = ThemeLibrary::load();
        let (theme, theme_warnings) = themes.resolve(&settings.theme);
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
        let ticker = context.spawn(async move |this, context| {
            loop {
                context
                    .background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this.update(context, |_, context| context.notify()).is_err() {
                    break;
                }
            }
        });

        let focus = context.focus_handle();
        window.focus(&focus, context);
        // Unsaved changes are asked about before the window closes.
        let this = context.weak_entity();
        window.on_window_should_close(context, move |window, context| {
            this.update(context, |this, context| this.should_close(window, context))
                .unwrap_or(true)
        });
        let project = ProjectFiles::new(&state);
        let activation = context.observe_window_activation(window, |this, window, context| {
            if window.is_window_active() {
                this.refresh_browser(context);
                context.notify();
            }
        });
        Self {
            browser: Browser::new(&mut state, context),
            headers: HeadersUi::new(),
            state,
            theme,
            focus,
            timeline: TimelineUi::default(),
            waves: Rc::default(),
            browser_open: true,
            browser_width: Splitter::new(Edge::Right, BROWSER_WIDTH, BROWSER_MIN, BROWSER_MAX),
            bottom: BottomPanel::new(),
            transport: Transport::new(),
            menu_bar: MenuBarUi::default(),
            dialog: None,
            settings: SettingsUi::new(themes, theme_warnings),
            export: ExportUi::new(context),
            project,
            popup: None,
            bench,
            _ticker: ticker,
            _activation: activation,
        }
    }

    fn toggle_play(&mut self) {
        if self.state.playback_state() == PlaybackState::Playing {
            self.state.stop();
        } else {
            self.state.play();
        }
    }

    /// Drags follow the pointer anywhere in the window, over a dialog too:
    /// heard before any element, whatever is under the pointer.
    fn follow_drags(&mut self, event: &MouseMoveEvent, window: &Window) -> bool {
        let width = f32::from(window.viewport_size().width);
        let height = f32::from(window.viewport_size().height);
        // The timeline keeps at least as much room as either panel.
        let browser = self
            .browser_width
            .drag_to(f32::from(event.position.x), width / 2.)
            .is_some();
        self.timeline_drag_move(event)
            | self.header_drag_move(event, width)
            | self.browser_drag_move(event)
            | self.transport_drag_move(event)
            | self.settings_drag_move(event)
            | self.export_drag_move(event)
            | self.bottom_drag_move(event, height)
            | browser
    }

    /// The pointer over the window, not under a dialog or a menu.
    fn on_hover(&mut self, event: &MouseMoveEvent, _: &mut Window, context: &mut Context<Self>) {
        if self.timeline_hover(event) | self.bottom_hover(event) {
            context.notify();
        }
    }

    /// Hears every pointer move, before the elements do: an element only
    /// hears it under the pointer, and a dialog's backdrop hides the
    /// window's root from it.
    fn drag_listener(&self, context: &mut Context<Self>) -> impl IntoElement {
        let this = context.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let this = this.clone();
                let pressed = this.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, app| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let _ = this.update(app, |this, context| {
                        if this.follow_drags(event, window) {
                            context.notify();
                        }
                    });
                });
                // Where the last press was decides where editing keys go.
                window.on_mouse_event(move |event: &MouseDownEvent, phase, _, app| {
                    if phase == DispatchPhase::Capture {
                        let _ = pressed.update(app, |this, _| this.bottom_press(event));
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn on_up(&mut self, event: &MouseUpEvent, window: &mut Window, context: &mut Context<Self>) {
        self.timeline_drag_end(event);
        self.header_drag_end();
        self.browser_drag_end();
        self.browser_width.end();
        self.transport.end_drags();
        self.settings_drag_end();
        self.export_drag_end();
        self.bottom_drag_end(window, context);
        context.notify();
    }

    fn is_dragging(&self) -> bool {
        self.timeline.is_dragging()
            || self.headers.is_dragging()
            || self.browser_width.is_dragging()
            || self.transport.is_dragging()
            || self.settings.is_dragging()
            || self.export.is_dragging()
            || self.bottom.is_dragging()
    }

    /// The browser, resizable from its right edge.
    fn browser_panel(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = self.theme.clone();
        div()
            .relative()
            .flex_none()
            .h_full()
            .w(px(self.browser_width.size))
            .child(crate::profile::time("view.browser", || {
                self.browser_view(window, context)
            }))
            .child(splitter_handle(
                "browser-edge",
                &self.browser_width,
                &theme,
                context,
                |this| &mut this.browser_width,
            ))
    }
}

/// A one pixel line between panels.
fn divider(theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(1.))
        .h_full()
        .bg(hsla(theme.separator))
}

impl Render for Tonique {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        if let Some(bench) = &mut self.bench {
            bench.frame(&mut self.state);
            // Every frame, as the egui app draws.
            window.request_animation_frame();
        }
        crate::profile::frame();
        crate::profile::time("state.update", || self.state.update());
        let scrolling = self.step_scrolling() | self.autoscroll();
        self.update_title(window);
        self.waves.borrow_mut().start_frame(window);
        let animating = self.state.playback_state() == PlaybackState::Playing
            || self.state.preview_playback_state() == PlaybackState::Playing
            || context.has_active_drag()
            || self.is_dragging()
            || scrolling
            || self.bottom.is_animating()
            || self.export.is_running()
            // Meters falling back after playback.
            || self.state.metrics.tracks.values().any(crate::meter::is_live);
        if animating {
            window.request_animation_frame();
        }
        if !context.has_active_drag() {
            self.timeline.drop_target = None;
            self.timeline.effect_target = None;
            self.browser.folders_over = false;
        }

        let theme = self.theme.clone();
        let body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .when(self.browser_open, |element| {
                element
                    .child(self.browser_panel(window, context))
                    .child(divider(&theme))
            })
            // As in the egui app: the track list on the timeline's right.
            .child(crate::profile::time("view.timeline", || {
                self.timeline_view(window, context)
            }))
            .child(divider(&theme))
            .child(crate::profile::time("view.headers", || {
                self.headers_view(context)
            }));

        div()
            .track_focus(&self.focus)
            .on_key_down(context.listener(Self::on_key))
            .on_mouse_move(context.listener(Self::on_hover))
            .on_mouse_up(MouseButton::Left, context.listener(Self::on_up))
            .on_mouse_up_out(MouseButton::Left, context.listener(Self::on_up))
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(theme.bg_deep))
            .text_color(hsla(theme.text))
            .font_family(FONT_UI)
            .text_size(px(size::BODY))
            .child(self.menu_bar(context))
            .child(self.transport(window, context))
            .child(body)
            .children(crate::profile::time("view.bottom", || {
                self.bottom_panel_view(window, context)
            }))
            .child(self.status_bar())
            .child(self.drag_listener(context))
            .children(self.popup_view(window, context))
            .children(self.dialog_view(window, context))
    }
}
