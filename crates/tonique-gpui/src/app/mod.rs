//! The window: the menu bar and the transport bar on top, the browser on
//! the left, the timeline and the track list on its right (or the audio
//! graph in their place), the bottom panel under them. The workspace
//! lays out those views, each an entity of its own sharing the `Project`
//! model, and shows the menus and dialogs over them; it passes on what one
//! panel asks of another, and hears the app-wide shortcuts.

pub mod arrows;
pub mod command;
mod dialog;
mod export;
pub mod keys;
mod memory;
pub mod menu_bar;
mod menus;
mod popup;
mod project;
mod settings;
mod transport;

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

use gpui::{
    Anchor, App, Context, DispatchPhase, Entity, FocusHandle, Global, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Render, Subscription, Task, Window, canvas, div, prelude::*, px,
};
use tonique_engine::{engine::Engine, time::BeatPos};

pub use arrows::ArrowHolder;
use dialog::Dialog;
use export::{ExportDialog, ExportEvent};
use menu_bar::MenuBarUi;
pub use popup::PopupKind;
use project::ProjectFiles;
use settings::{SettingsDialog, SettingsEvent, SettingsTab};
use transport::{Shows, TransportBar, TransportEvent};

use crate::{
    audio::host::AudioHost,
    bench::Bench,
    bottom::{BottomPanel, BottomView},
    browser::BrowserPanel,
    config::settings::Settings,
    core::state::{PlaybackState, ProjectState},
    graph::GraphView,
    model::{HoldsProject, Project, toggle_play},
    popup::PopupLayer,
    theme::{FONT_UI, Theme, ThemeLibrary, hsla, size},
    timeline::{Arrangement, ArrangementEvent},
    wave::WaveCache,
    widgets::{Edge, Splitter, splitter_handle},
};

/// The browser's width, as the egui app's left panel.
const BROWSER_WIDTH: f32 = 220.;
const BROWSER_MIN: f32 = 100.;
const BROWSER_MAX: f32 = 400.;

pub struct Workspace {
    pub project: Entity<Project>,
    pub focus: FocusHandle,
    /// The timeline and the track list.
    pub arrangement: Entity<Arrangement>,
    /// The audio graph, shown instead of the timeline when open.
    pub graph: Entity<GraphView>,
    pub browser: Entity<BrowserPanel>,
    pub waves: Rc<RefCell<WaveCache>>,
    /// Whether the browser shows, as Tonique's left panel, and how wide.
    pub browser_open: bool,
    pub browser_width: Splitter,
    /// The selected track's effects, or a clip's details, at the bottom.
    pub bottom: Entity<BottomPanel>,
    transport: Entity<TransportBar>,
    menu_bar: MenuBarUi,
    /// The project's file, and the recent ones.
    files: ProjectFiles,
    /// The dialog open, if any, over everything.
    dialog: Option<Dialog>,
    settings: Entity<SettingsDialog>,
    /// Exporting audio: what's chosen, and the export running.
    export: Entity<ExportDialog>,
    /// The menus' layer, over everything.
    popups: Entity<PopupLayer>,
    /// `TONIQUE_BENCH`: steers the view and measures frames.
    bench: Option<Bench>,
    _ticker: Task<()>,
    /// What the panels ask of the window.
    _subscriptions: Vec<Subscription>,
}

impl HoldsProject for Workspace {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl Workspace {
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
        crate::theme::install(theme, context);
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
        context.set_global(MainFocus(focus.clone()));
        // Unsaved changes are asked about before the window closes.
        let this = context.weak_entity();
        window.on_window_should_close(context, move |window, context| {
            this.update(context, |this, context| this.should_close(window, context))
                .unwrap_or(true)
        });
        let files = ProjectFiles::new(&state);
        let project = context.new(|_| Project::new(state));
        let waves: Rc<RefCell<WaveCache>> = Rc::default();
        let browser = context
            .new(|context| BrowserPanel::new(project.clone(), waves.clone(), window, context));
        let transport = context.new(|_| TransportBar::new(project.clone()));
        let transport_events = context.subscribe(&transport, Self::on_transport);
        let arrangement = context.new(|_| Arrangement::new(project.clone(), waves.clone()));
        let settings = context
            .new(|context| SettingsDialog::new(project.clone(), themes, theme_warnings, context));
        let settings_events = context.subscribe_in(&settings, window, Self::on_settings);
        let arrangement_events = context.subscribe(&arrangement, Self::on_arrangement);
        let export =
            context.new(|context| ExportDialog::new(project.clone(), waves.clone(), context));
        let export_events = context.subscribe_in(&export, window, Self::on_export);
        Self {
            browser,
            graph: context.new(|_| GraphView::new(project.clone())),
            bottom: context.new(|_| BottomPanel::new(project.clone(), waves.clone())),
            arrangement,
            project,
            focus,
            waves,
            browser_open: true,
            browser_width: Splitter::new(Edge::Right, BROWSER_WIDTH, BROWSER_MIN, BROWSER_MAX),
            transport,
            menu_bar: MenuBarUi::default(),
            dialog: None,
            settings,
            export,
            files,
            popups: crate::popup::init(context),
            bench,
            _ticker: ticker,
            _subscriptions: vec![
                transport_events,
                arrangement_events,
                settings_events,
                export_events,
            ],
        }
    }

    fn on_transport(
        &mut self,
        _: Entity<TransportBar>,
        event: &TransportEvent,
        context: &mut Context<Self>,
    ) {
        match event {
            TransportEvent::ToggleBrowser => self.browser_open = !self.browser_open,
            TransportEvent::ToggleGraph => self.toggle_graph(context),
            TransportEvent::ToggleGridMenu(at) => {
                let build = self
                    .arrangement
                    .read(context)
                    .grid_menu_builder(&self.arrangement);
                crate::popup::toggle(PopupKind::Grid, *at, Anchor::TopRight, build, context);
            }
            TransportEvent::ShowMissingFiles => self.show_missing_files(None, context),
            TransportEvent::OpenAudioSettings => {
                self.open_settings(SettingsTab::Audio, context);
            }
        }
        context.notify();
    }

    /// Shows the browser's missing files' tab, `path`'s selected if given (a
    /// missing clip's).
    pub fn show_missing_files(&mut self, path: Option<PathBuf>, context: &mut App) {
        self.browser_open = true;
        self.browser.update(context, |browser, context| {
            browser.show_missing_files(path, context);
            context.notify();
        });
    }

    fn on_arrangement(
        &mut self,
        _: Entity<Arrangement>,
        event: &ArrangementEvent,
        context: &mut Context<Self>,
    ) {
        match event {
            ArrangementEvent::ShowEffects => self.bottom.update(context, |bottom, context| {
                bottom.shown.open = true;
                bottom.shown.view = BottomView::Effects;
                context.notify();
            }),
            ArrangementEvent::ShowClipDetails(id) => {
                self.bottom.update(context, |bottom, context| {
                    bottom.show_clip_details(*id, context);
                    context.notify();
                });
            }
            ArrangementEvent::ToggleEffectsOfSelected(was_selected) => {
                self.bottom.update(context, |bottom, context| {
                    bottom.toggle_effects_of_selected(*was_selected);
                    context.notify();
                });
            }
            ArrangementEvent::ShowMissingFiles(path) => {
                self.show_missing_files(path.clone(), context);
            }
        }
        context.notify();
    }

    fn on_settings(
        &mut self,
        _: &Entity<SettingsDialog>,
        SettingsEvent::Close: &SettingsEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.close_dialog(window, context);
        context.notify();
    }

    fn on_export(
        &mut self,
        _: &Entity<ExportDialog>,
        ExportEvent::Close: &ExportEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.close_dialog(window, context);
        context.notify();
    }

    /// Asks what to export. Ignored while an export runs.
    pub fn open_export(&mut self, window: &mut Window, context: &mut Context<Self>) {
        if self.export_running(context) {
            return;
        }
        let name = self.files.name();
        let dir = self.files.dir().map(PathBuf::from);
        self.export.update(context, |export, context| {
            export.open(name, dir.as_deref());
            context.notify();
        });
        crate::popup::close(context);
        self.dialog = Some(Dialog::Export);
        window.focus(&self.focus, context);
    }

    fn export_running(&self, context: &App) -> bool {
        self.export.read(context).is_running()
    }

    /// Opens the settings on `tab`, from the settings in use.
    pub fn open_settings(&mut self, tab: SettingsTab, context: &mut App) {
        self.settings
            .update(context, |settings, context| settings.open(tab, context));
        self.dialog = Some(Dialog::Settings);
    }

    /// Opens or closes the settings, as their shortcut does.
    pub fn toggle_settings(&mut self, context: &mut App) {
        if self.dialog == Some(Dialog::Settings) {
            self.dialog = None;
        } else {
            self.open_settings(SettingsTab::General, context);
        }
    }

    /// Between the timeline and the audio graph.
    fn toggle_graph(&mut self, context: &mut Context<Self>) {
        self.graph.update(context, GraphView::toggle);
    }

    fn toggle_play(&mut self, context: &mut App) {
        toggle_play(&mut self.state_mut(context));
    }

    /// The browser's edge follows the pointer anywhere in the window:
    /// heard before any element, whatever is under the pointer.
    fn follow_drags(&mut self, event: &MouseMoveEvent, window: &Window) -> bool {
        let width = f32::from(window.viewport_size().width);
        // The timeline keeps at least as much room as either panel.
        self.browser_width
            .drag_to(f32::from(event.position.x), width / 2.)
            .is_some()
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
                // Where the last press was decides where editing keys go;
                // a press lets go of the arrow keys and stops recording a
                // shortcut, unless on the control holding them, which takes
                // them back after this.
                window.on_mouse_event(move |_: &MouseDownEvent, phase, _, app| {
                    if phase == DispatchPhase::Capture {
                        let _ = pressed.update(app, |_this, context| {
                            crate::app::arrows::release(context);
                        });
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn on_up(&mut self, _: &MouseUpEvent, _window: &mut Window, context: &mut Context<Self>) {
        self.browser_width.end();
        context.notify();
    }

    fn is_dragging(&self) -> bool {
        self.browser_width.is_dragging()
    }

    /// The browser, resizable from its right edge.
    fn browser_panel(&mut self, context: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::current(context);
        div()
            .relative()
            .flex_none()
            .h_full()
            .w(px(self.browser_width.size))
            .child(self.browser.clone())
            .child(splitter_handle(
                "browser-edge",
                &self.browser_width,
                &theme,
                context,
                |this| &mut this.browser_width,
            ))
    }
}

/// The app's keyboard focus: where the shortcuts are heard. Fields give
/// it back when they're done.
struct MainFocus(FocusHandle);

impl Global for MainFocus {}

/// Gives the keyboard back to the app, from a field.
pub fn focus_main(window: &mut Window, context: &mut App) {
    let focus = context.global::<MainFocus>().0.clone();
    window.focus(&focus, context);
}

/// A one pixel line between panels.
fn divider(theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .w(px(1.))
        .h_full()
        .bg(hsla(theme.separator))
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        if let Some(bench) = &mut self.bench {
            bench.frame(&mut self.project.as_mut(context).state);
            // Every frame, as the egui app draws.
            window.request_animation_frame();
        }
        crate::profile::frame();
        self.menu_bar.frame();
        crate::profile::time("state.update", || self.state_mut(context).update());
        self.update_title(window, context);
        self.waves.borrow_mut().start_frame(window);
        let animating = self.state(context).playback_state() == PlaybackState::Playing
            || self.state(context).preview_playback_state() == PlaybackState::Playing
            || context.has_active_drag()
            || self.is_dragging()
            // Meters falling back after playback.
            || self.state(context).metrics.tracks.values().any(crate::meter::is_live);
        if animating {
            window.request_animation_frame();
        }
        let shows = Shows {
            browser: self.browser_open,
            graph: self.graph.read(context).is_open(),
            grid_menu: crate::popup::shown(context) == Some(PopupKind::Grid),
        };
        self.transport
            .update(context, |transport, context| transport.show(shows, context));

        let theme = crate::theme::current(context);
        let bottom_open = self.bottom.read(context).shown.open;
        let body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .when(self.browser_open, |element| {
                element
                    .child(self.browser_panel(context))
                    .child(divider(&theme))
            });
        let body = if self.graph.read(context).is_open() {
            body.child(self.graph.clone())
        } else {
            body.child(self.arrangement.clone())
        };

        div()
            .track_focus(&self.focus)
            .on_key_down(context.listener(Self::on_key))
            .on_action(context.listener(Self::on_command))
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
            .child(self.transport.clone())
            .child(body)
            .when(bottom_open, |element| element.child(self.bottom.clone()))
            .child(self.drag_listener(context))
            .child(self.popups.clone())
            .children(self.dialog_view(window, context))
    }
}
