//! The browser, as Tonique's left panel. A rail of icons on its left
//! picks what it shows:
//! - Files: the folders added to it ([`folders`]), each a section of one
//!   tree, searchable together or one at a time in the background,
//!   browsed with the mouse or the arrows, and dragged onto the timeline;
//! - Labels: the files and folders given a label, by label ([`labels`]);
//! - Effects ([`effects`]), dragged onto tracks;
//! - Missing files ([`missing`]), while the project has some.
//!
//! Under the lists, the preview of the audio file selected ([`preview`]).
//! Folders and labels have menus ([`menus`]).

mod effects;
mod folders;
mod labels;
mod list;
mod menus;
mod missing;
mod preview;
mod rows;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};

use gpui::{
    App, Bounds, Context, Entity, FocusHandle, MouseButton, Pixels, Render, SharedString, Window,
    div, prelude::*, px,
};

use list::FileList;
pub use list::ListKind;
use missing::MissingView;

use crate::{
    icons,
    model::{HoldsProject, Project},
    theme::{Color, FONT_UI, Theme, hsla, icon, size},
    wave::WaveCache,
    widgets::{Recheck, TextField, follow_drag, icon_rail},
};
use tonique_core::{audio_files::analysis::AudioInfo, config::Config, state::PlaybackState};

/// Side of the rail's buttons, and so its width.
const RAIL_WIDTH: f32 = 28.;
/// Above the rail's first button.
const RAIL_TOP: f32 = 4.;

/// An audio file dragged from the browser.
#[derive(Clone)]
pub struct DraggedAudio {
    pub path: PathBuf,
}

/// What follows the pointer while a file is dragged.
struct DragGhost {
    name: SharedString,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        let theme = &*crate::theme::current(context);
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(hsla(theme.bg_raised))
            .border_1()
            .border_color(hsla(theme.border))
            .text_color(hsla(theme.text))
            .font_family(FONT_UI)
            .text_size(px(size::BODY))
            .child(self.name.clone())
    }
}

/// What the browser lists.
#[derive(Default, Clone, Copy, PartialEq)]
pub enum BrowserSource {
    #[default]
    Files,
    /// What's been given a label, by label.
    Labels,
    Effects,
    /// Audio files the project uses that weren't found: a tab only while
    /// there are.
    Missing,
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum BrowserDrag {
    #[default]
    None,
    Seek,
    Volume,
}

/// A label, as the lists show it.
#[derive(Clone)]
pub struct LabelView {
    pub name: String,
    pub color: Color,
}

pub struct BrowserPanel {
    project: Entity<Project>,
    /// The waveforms drawn, shared with the other views.
    waves: Rc<RefCell<WaveCache>>,
    /// Tonique's config: the folders, which are collapsed, the labels and
    /// the preview's settings, saved as they change.
    config: Config,
    files: FileList,
    /// The labels' tab.
    labelled: FileList,
    /// The labels' sections closed.
    labels_collapsed: Vec<PathBuf>,
    labels: Vec<LabelView>,
    /// Each labelled file or folder's label.
    item_labels: HashMap<PathBuf, usize>,
    /// The labels' sections, by their headers' paths: their labels, and
    /// how many items they have.
    sections: HashMap<PathBuf, (usize, usize)>,
    /// The name typed in a label's menu.
    label_name: TextField,
    effect_search: TextField,
    missing: MissingView,
    /// The selected audio file.
    audio: Option<AudioInfo>,
    pub list_focus: FocusHandle,
    auto_preview: bool,
    volume: f32,
    /// Silenced from its speaker, the slider kept where it is.
    muted: bool,
    /// Folders are dragged over it from the system's file manager.
    pub folders_over: bool,
    wave_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub drag: BrowserDrag,
    pub source: BrowserSource,
    /// Looks again while the selected file decodes or the folders are read.
    recheck: Recheck,
}

impl HoldsProject for BrowserPanel {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl BrowserPanel {
    /// What its waveforms take.
    pub fn wave_memory(&self) -> crate::wave::WaveMemory {
        self.waves.borrow().memory()
    }

    pub fn new(project: Entity<Project>, window: &mut Window, context: &mut Context<Self>) -> Self {
        let config = Config::load();
        let mut files = FileList::new(context);
        files.set_roots(config.list_dirs(), config.collapsed_dirs());
        project
            .state_mut(context)
            .set_preview_gain(preview::preview_gain(config.preview_volume()));
        // The folders are read again when the window gets the focus back.
        context
            .observe_window_activation(window, |this, window, context| {
                if window.is_window_active() {
                    this.refresh_browser(context);
                    context.notify();
                }
            })
            .detach();
        let mut browser = Self {
            project,
            waves: Rc::default(),
            files,
            labelled: FileList::new(context),
            labels_collapsed: Vec::new(),
            labels: Vec::new(),
            item_labels: HashMap::new(),
            sections: HashMap::new(),
            label_name: TextField::new(context),
            effect_search: TextField::new(context),
            missing: MissingView::new(context),
            audio: None,
            list_focus: context.focus_handle(),
            auto_preview: config.auto_preview(),
            volume: config.preview_volume(),
            muted: false,
            folders_over: false,
            wave_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            drag: BrowserDrag::None,
            source: BrowserSource::Files,
            recheck: Recheck::default(),
            config,
        };
        browser.refresh_labels();
        browser
    }

    pub(super) fn list(&self, kind: ListKind) -> &FileList {
        match kind {
            ListKind::Files => &self.files,
            ListKind::Labels => &self.labelled,
        }
    }

    pub(super) fn list_mut(&mut self, kind: ListKind) -> &mut FileList {
        match kind {
            ListKind::Files => &mut self.files,
            ListKind::Labels => &mut self.labelled,
        }
    }

    /// The list shown, if the tab shown is one.
    pub fn shown_list(&self) -> Option<ListKind> {
        match self.source {
            BrowserSource::Files => Some(ListKind::Files),
            BrowserSource::Labels => Some(ListKind::Labels),
            BrowserSource::Effects | BrowserSource::Missing => None,
        }
    }

    /// The folders added to the browser.
    pub fn folders(&self) -> Vec<PathBuf> {
        self.config.list_dirs()
    }
}

impl BrowserPanel {
    /// Shows the missing files' tab, `path`'s selected if given (a missing
    /// clip's).
    pub fn show_missing_files(&mut self, path: Option<PathBuf>, context: &mut App) {
        self.set_browser_source(BrowserSource::Missing);
        if let Some(path) = path {
            let state = self.state(context);
            self.missing.select_path(state, &path);
        }
    }

    fn set_browser_source(&mut self, source: BrowserSource) {
        if self.source != source {
            self.source = source;
            self.audio = None;
        }
    }
}

impl Render for BrowserPanel {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        self.waves.borrow_mut().start_frame(window);
        // The preview's playhead, while it plays.
        if self.state(context).preview_playback_state() == PlaybackState::Playing {
            self.watch_playback(context);
        }
        // The selected file's waveform and tempo, the files found for the
        // missing ones, as they come.
        let decoding = self
            .audio
            .as_ref()
            .is_some_and(|audio| audio.data.is_loading());
        let scanning = self.source == BrowserSource::Missing && self.missing.index.is_scanning();
        if decoding || scanning {
            self.recheck.soon(context);
        }
        if !context.has_active_drag() {
            self.folders_over = false;
        }
        let theme = crate::theme::current(context);
        let mut sources = vec![
            (BrowserSource::Files, icons::FOLDER_SIMPLE, "Files"),
            (BrowserSource::Labels, icons::TAG, "Labels"),
            (BrowserSource::Effects, icons::MAGIC_WAND, "Effects"),
        ];
        // Missing files below the rest, while there are (or shown).
        if !self.state(context).missing_files().is_empty() || self.source == BrowserSource::Missing
        {
            sources.push((BrowserSource::Missing, icons::WARNING, "Missing files"));
        }
        let rail = icon_rail(
            &sources,
            self.source,
            RAIL_WIDTH,
            &theme,
            context,
            |this, source| this.set_browser_source(source),
        )
        .pt(px(RAIL_TOP));
        let panel = div().flex_1().min_w_0().flex().flex_col();
        let content = match self.source {
            BrowserSource::Effects => panel
                .child(self.effect_search_bar(&theme, window, context))
                .child(self.effects_list(context))
                .into_any_element(),
            BrowserSource::Missing => panel
                .child(self.missing_view(window, context))
                .into_any_element(),
            BrowserSource::Files => {
                let list = self.list_view(ListKind::Files, window, context);
                self.take_folder_drops(panel.child(list), context)
                    .children(self.preview_view(context))
                    .into_any_element()
            }
            BrowserSource::Labels => panel
                .child(self.list_view(ListKind::Labels, window, context))
                .children(self.preview_view(context))
                .into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .bg(hsla(theme.bg_panel))
            .child(rail)
            .child(div().flex_none().w(px(1.)).bg(hsla(theme.separator)))
            .child(content)
            // A preview's seek or volume drag follows the pointer anywhere.
            .when(self.drag != BrowserDrag::None, |element| {
                element.child(follow_drag(
                    context.weak_entity(),
                    |this, event, context| {
                        this.browser_drag_move(event, context);
                    },
                    |this, _, _| this.browser_drag_end(),
                ))
            })
    }
}

impl BrowserPanel {
    /// A search field, `placeholder` in it while empty; `key` takes its
    /// keys, `clear` empties it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn search_bar(
        &self,
        id: &'static str,
        field: &TextField,
        placeholder: &str,
        busy: bool,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
        key: fn(&mut Self, &gpui::KeyDownEvent, &mut Window, &mut Context<Self>),
        clear: fn(&mut Self, &mut Context<Self>),
    ) -> gpui::Stateful<gpui::Div> {
        let focused = field.focus.is_focused(window);
        let focus = field.focus.clone();
        div()
            .id(id)
            .track_focus(&field.focus)
            .on_key_down(context.listener(key))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |_, _, window, context| {
                    window.focus(&focus, context);
                    context.notify();
                }),
            )
            .m_2()
            .px_2()
            .h(px(26.))
            .flex_none()
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(hsla(theme.bg_deep))
            .border_1()
            .border_color(hsla(if focused { theme.accent } else { theme.bg_deep }))
            .gap_2()
            .cursor_text()
            .child(
                icon(if busy {
                    icons::CIRCLE_NOTCH
                } else {
                    icons::MAGNIFYING_GLASS
                })
                .text_color(hsla(theme.text_muted)),
            )
            .child(field.view(placeholder, theme, window))
            .when(!field.text.is_empty(), |element| {
                element.child(
                    icon(icons::X)
                        .text_color(hsla(theme.text_muted))
                        .cursor_pointer()
                        .hover(|style| style.text_color(hsla(theme.text)))
                        .on_mouse_down(
                            MouseButton::Left,
                            context.listener(move |this, _, _, context| {
                                context.stop_propagation();
                                clear(this, context);
                                context.notify();
                            }),
                        ),
                )
            })
    }
}
