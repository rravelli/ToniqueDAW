//! The file browser: the folders from Tonique's config as one tree
//! (Tonique's own `FileTree`), searchable in the background, browsed with
//! the mouse or the arrows, previewed as files are selected, and dragged
//! onto the timeline. Under it, the preview: the file's waveform to seek in,
//! play, auto-preview and the preview's volume.
//!
//! Left out: labels, folder menus, adding folders, pinned headers.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use gpui::{
    Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Render, ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, canvas,
    div, prelude::*, px, uniform_list,
};

use crate::{
    analysis::AudioInfo,
    app::Tonique,
    cache::AUDIO_ANALYSIS_CACHE,
    config::Config,
    core::state::{PlaybackState, ProjectState},
    file_tree::{FileTree, is_audio_file},
    icons,
    theme::{FONT_UI, Theme, h, icon, icon_fill, mix, mono, size},
    utils::format_time,
    wave::{R, paint_waveform},
    widgets::{on_press, paint_slider, toggle_button},
};

const ROW_H: f32 = 22.;
const INDENT: f32 = 12.;
const PREVIEW_H: f32 = 118.;

/// An audio file dragged from the browser.
#[derive(Clone)]
pub struct DraggedAudio {
    pub path: PathBuf,
}

/// What follows the pointer while a file is dragged.
struct DragGhost {
    name: SharedString,
    theme: Theme,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(h(t.bg_raised))
            .border_1()
            .border_color(h(t.border))
            .text_color(h(t.text))
            .font_family(FONT_UI)
            .text_size(px(size::BODY))
            .child(self.name.clone())
    }
}

#[derive(Default, Clone, Copy, PartialEq)]
pub enum BrowserDrag {
    #[default]
    None,
    Seek,
    Volume,
}

pub struct Browser {
    tree: FileTree,
    selected: Option<usize>,
    /// The selected audio file.
    audio: Option<AudioInfo>,
    query: String,
    search_focus: FocusHandle,
    pub list_focus: FocusHandle,
    scroll: UniformListScrollHandle,
    auto_preview: bool,
    volume: f32,
    /// Silenced from its speaker, the slider kept where it is.
    muted: bool,
    _search: Option<Task<()>>,
    wave_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub drag: BrowserDrag,
}

/// The preview's gain for the slider at `volume`: a square law, as in
/// Tonique.
fn preview_gain(volume: f32) -> f32 {
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
        n => format!("{n} ch"),
    };
    match audio.bit_depth {
        Some(bits) => format!("{rate} · {bits}-bit · {channels}"),
        None => format!("{rate} · {channels}"),
    }
}

impl Browser {
    pub fn new(state: &mut ProjectState, cx: &mut Context<Tonique>) -> Self {
        // Read only: nothing is written back to Tonique's config.
        let config = Config::load();
        let mut tree = FileTree::new();
        tree.set_roots(config.list_dirs(), config.collapsed_dirs());
        state.set_preview_gain(preview_gain(config.preview_volume()));
        Self {
            tree,
            selected: None,
            audio: None,
            query: String::new(),
            search_focus: cx.focus_handle(),
            list_focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            auto_preview: config.auto_preview(),
            volume: config.preview_volume(),
            muted: false,
            _search: None,
            wave_bounds: Rc::default(),
            volume_bounds: Rc::default(),
            drag: BrowserDrag::None,
        }
    }
}

impl Tonique {
    /// Select row `i`: open or close a folder, or select (and preview) a
    /// file.
    fn browser_select(&mut self, i: usize, toggle_folder: bool) {
        let Some(item) = self.browser.tree.items.get(i).cloned() else {
            return;
        };
        self.browser.selected = Some(i);
        if item.is_dir {
            if toggle_folder {
                self.browser.tree.toggle_folder(i);
            }
            return;
        }
        self.browser.audio = None;
        if !is_audio_file(&item.path) {
            return;
        }
        self.browser.audio = AUDIO_ANALYSIS_CACHE.get_or_analyze(item.path.clone());
        if let Some(audio) = &self.browser.audio {
            audio.load_peaks();
        }
        if self.browser.auto_preview {
            self.state.play_preview(item.path);
        }
    }

    pub fn browser_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let len = self.browser.tree.items.len();
        if len == 0 {
            return;
        }
        let sel = self.browser.selected;
        let item = |this: &Self, i: usize| this.browser.tree.items.get(i).cloned();
        match ev.keystroke.key.as_str() {
            "down" => self.browser_select(sel.map_or(0, |i| (i + 1).min(len - 1)), false),
            "up" => self.browser_select(sel.map_or(0, |i| i.saturating_sub(1)), false),
            "right" | "left" | "enter" => {
                let Some(i) = sel else { return };
                let Some(node) = item(self, i) else { return };
                let open = self
                    .browser
                    .tree
                    .items
                    .get(i + 1)
                    .is_some_and(|n| n.depth > node.depth);
                match ev.keystroke.key.as_str() {
                    "right" if node.is_dir && !open => self.browser.tree.toggle_folder(i),
                    "left" if node.is_dir && open => self.browser.tree.toggle_folder(i),
                    // To the folder it's in.
                    "left" => {
                        if let Some(parent) = (0..i)
                            .rev()
                            .find(|&p| self.browser.tree.items[p].depth < node.depth)
                        {
                            self.browser.selected = Some(parent);
                        }
                    }
                    "enter" if node.is_dir => self.browser.tree.toggle_folder(i),
                    "enter" => self.state.play_preview(node.path),
                    _ => return,
                }
            }
            _ => return,
        }
        if let Some(i) = self.browser.selected {
            self.browser
                .scroll
                .scroll_to_item(i, ScrollStrategy::Center);
        }
        cx.notify();
    }

    fn search_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        match k.key.as_str() {
            "backspace" => {
                self.browser.query.pop();
            }
            "escape" => {
                self.browser.query.clear();
                window.focus(&self.browser.list_focus, cx);
            }
            "enter" | "down" => window.focus(&self.browser.list_focus, cx),
            _ if k.modifiers.control || k.modifiers.platform => return,
            _ => match &k.key_char {
                Some(c) if !c.chars().any(char::is_control) => self.browser.query.push_str(c),
                _ => return,
            },
        }
        cx.stop_propagation();
        self.run_search(cx);
        cx.notify();
    }

    /// Search the folders in the background; the latest search wins.
    fn run_search(&mut self, cx: &mut Context<Self>) {
        let query = self.browser.query.trim().to_string();
        self.browser.selected = None;
        if query.is_empty() {
            self.browser._search = None;
            self.browser.tree.clear_search();
            return;
        }
        let mut search = self.browser.tree.start_search();
        self.browser._search = Some(cx.spawn(async move |this, cx| {
            let q = query.clone();
            let search = cx
                .background_executor()
                .spawn(async move {
                    search.run(&q, None);
                    search
                })
                .await;
            this.update(cx, |this, cx| {
                if this.browser.query.trim() == query {
                    this.browser.tree.finish_search(&query, search);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn seek_preview_at(&mut self, x: Pixels) {
        let Some(audio) = self.browser.audio.clone() else {
            return;
        };
        let Some(frames) = audio.total_frames() else {
            return;
        };
        let b = self.browser.wave_bounds.get();
        let t = (f32::from(x - b.origin.x) / f32::from(b.size.width).max(1.)).clamp(0., 1.);
        if self.state.previewing() != Some(audio.path.as_path()) {
            self.state.play_preview(audio.path.clone());
        }
        self.state.seek_preview((t as f64 * frames) as usize);
    }

    fn set_preview_volume_at(&mut self, x: Pixels) {
        let b = self.browser.volume_bounds.get();
        self.browser.volume =
            (f32::from(x - b.origin.x) / f32::from(b.size.width).max(1.)).clamp(0., 1.);
        self.browser.muted = false;
        self.apply_preview_gain();
    }

    fn apply_preview_gain(&mut self) {
        let gain = if self.browser.muted {
            0.
        } else {
            preview_gain(self.browser.volume)
        };
        self.state.set_preview_gain(gain);
    }

    pub fn browser_drag_move(&mut self, ev: &MouseMoveEvent) -> bool {
        match self.browser.drag {
            BrowserDrag::None => return false,
            BrowserDrag::Seek => self.seek_preview_at(ev.position.x),
            BrowserDrag::Volume => self.set_preview_volume_at(ev.position.x),
        }
        true
    }

    pub fn browser_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = self.theme.clone();
        let searching = self.browser.search_focus.is_focused(window);
        let count = self.browser.tree.items.len();

        let search = div()
            .id("search")
            .track_focus(&self.browser.search_focus)
            .on_key_down(cx.listener(Self::search_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.browser.search_focus, cx);
                    cx.notify();
                }),
            )
            .mx_2()
            .my_2()
            .px_2()
            .h(px(26.))
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(h(t.bg_deep))
            .border_1()
            .border_color(h(if searching { t.accent } else { t.bg_deep }))
            .gap_2()
            .cursor_text()
            .child(icon(icons::MAGNIFYING_GLASS).text_color(h(t.text_muted)))
            .child(if self.browser.query.is_empty() && !searching {
                div()
                    .flex_1()
                    .text_color(h(t.text_disabled))
                    .child("Search files")
            } else {
                div().flex_1().child(format!(
                    "{}{}",
                    self.browser.query,
                    if searching { "▏" } else { "" }
                ))
            })
            .when(!self.browser.query.is_empty(), |d| {
                d.child(
                    icon(icons::X)
                        .text_color(h(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.text_color(h(t.text)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.browser.query.clear();
                                this.run_search(cx);
                                cx.notify();
                            }),
                        ),
                )
            });

        let list = uniform_list(
            "files",
            count,
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let t = this.theme.clone();
                range
                    .map(|i| {
                        let item = this.browser.tree.items[i].clone();
                        let name = item.path.file_name().map_or_else(
                            || item.path.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        let selected = this.browser.selected == Some(i);
                        let open = item.is_dir
                            && this
                                .browser
                                .tree
                                .items
                                .get(i + 1)
                                .is_some_and(|n| n.depth > item.depth);
                        let audio = !item.is_dir && is_audio_file(&item.path);
                        let root = item.depth == 0;
                        let previewing = this.state.previewing() == Some(item.path.as_path())
                            && this.state.preview_playback_state() == PlaybackState::Playing;
                        let kind = if item.is_dir {
                            if open {
                                icons::FOLDER_OPEN
                            } else {
                                icons::FOLDER
                            }
                        } else if audio {
                            icons::FILE_AUDIO
                        } else {
                            icons::FILE
                        };
                        let bg = if selected {
                            mix(t.bg_panel, t.accent, 0.25)
                        } else if root {
                            t.bg_raised
                        } else {
                            t.bg_panel
                        };
                        let color = if root || item.is_dir || audio {
                            t.text
                        } else {
                            t.text_disabled
                        };
                        let row = div()
                            .id(i)
                            .h(px(ROW_H))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_1()
                            .pl(px(8. + item.depth as f32 * INDENT))
                            .pr_2()
                            .bg(h(bg))
                            .hover(move |s| s.bg(h(mix(bg, t.text, 0.05))))
                            .text_color(h(color))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                    window.focus(&this.browser.list_focus, cx);
                                    this.browser_select(i, true);
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .w(px(12.))
                                    .flex_none()
                                    .text_color(h(t.text_muted))
                                    .when(item.is_dir, |d| {
                                        d.child(icon(if open {
                                            icons::CARET_DOWN
                                        } else {
                                            icons::CARET_RIGHT
                                        }))
                                    }),
                            )
                            .when(!root, |d| {
                                d.child(icon(kind).text_color(h(if audio {
                                    t.accent
                                } else {
                                    t.text_muted
                                })))
                            });
                        let row = if root {
                            row.child(
                                div()
                                    .text_size(px(size::SMALL))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(h(t.text_muted))
                                    .child(name.to_uppercase()),
                            )
                        } else {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .child(name.clone()),
                            )
                            .when(previewing, |d| {
                                d.child(icon(icons::SPEAKER_HIGH).text_color(h(t.accent)))
                            })
                        };
                        if audio {
                            let theme = t.clone();
                            row.on_drag(
                                DraggedAudio {
                                    path: item.path.clone(),
                                },
                                move |_, _, _, cx| {
                                    cx.new(|_| DragGhost {
                                        name: name.clone().into(),
                                        theme: theme.clone(),
                                    })
                                },
                            )
                        } else {
                            row
                        }
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.browser.scroll)
        .flex_1();

        div()
            .w(px(280.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(h(t.bg_panel))
            .child(search)
            .child(
                div()
                    .id("file-list")
                    .track_focus(&self.browser.list_focus)
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(if count == 0 {
                        div()
                            .p_3()
                            .text_color(h(t.text_muted))
                            .child(if self.browser.query.is_empty() {
                                "No folders in Tonique's config"
                            } else {
                                "Nothing found"
                            })
                            .into_any_element()
                    } else {
                        list.into_any_element()
                    }),
            )
            .child(self.preview_view(cx))
    }

    fn preview_view(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme.clone();
        let Some(audio) = self.browser.audio.clone() else {
            return div()
                .h(px(PREVIEW_H))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .border_t_1()
                .border_color(h(t.separator))
                .text_color(h(t.text_disabled))
                .child("Select an audio file to preview it")
                .into_any_element();
        };
        let playing_this = self.state.preview_playback_state() == PlaybackState::Playing
            && self.state.previewing() == Some(audio.path.as_path());
        let frames = audio.total_frames();
        let position = if self.state.previewing() == Some(audio.path.as_path()) {
            self.state.preview_position() as f64
        } else {
            0.
        };
        let seconds = |f: f64| f / audio.sample_rate.max(1) as f64;
        let length = frames.map_or("…".to_string(), |f| short_time(seconds(f)));

        let waves = self.waves.clone();
        let slot = self.browser.wave_bounds.clone();
        let theme = t.clone();
        let data = audio.data.clone();
        let stereo = audio.channels >= 2;
        let waveform = div()
            .h(px(52.))
            .mx_2()
            .rounded(px(3.))
            .bg(h(t.bg_deep))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    this.browser.drag = BrowserDrag::Seek;
                    this.seek_preview_at(ev.position.x);
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    move |b, _, _| slot.set(b),
                    move |b, _, window, _| {
                        let r = R::new(
                            f32::from(b.origin.x),
                            f32::from(b.origin.y) + 3.,
                            f32::from(b.size.width),
                            f32::from(b.size.height) - 6.,
                        );
                        let Some(frames) = frames else { return };
                        let at = r.x + (position / frames) as f32 * r.w;
                        let mut waves = waves.borrow_mut();
                        // Played: bright; the rest: dim.
                        let played = R::new(r.x, r.y, at - r.x, r.h);
                        let rest = R::new(at, r.y, r.right() - at, r.h);
                        paint_waveform(
                            window,
                            &mut waves,
                            r,
                            played,
                            &data,
                            0.0..frames,
                            stereo,
                            h(theme.accent),
                            1.,
                            0.,
                        );
                        paint_waveform(
                            window,
                            &mut waves,
                            r,
                            rest,
                            &data,
                            0.0..frames,
                            stereo,
                            h(theme.text_muted).opacity(0.55),
                            1.,
                            0.,
                        );
                        if position > 0. {
                            window.paint_quad(gpui::fill(
                                R::new(at, r.y - 3., 1., r.h + 6.).bounds(),
                                h(theme.playhead),
                            ));
                        }
                    },
                )
                .size_full(),
            );

        let vslot = self.browser.volume_bounds.clone();
        let volume = self.browser.volume;
        let theme = t.clone();
        let volume_slider = div()
            .w(px(90.))
            .h(px(14.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    this.browser.drag = BrowserDrag::Volume;
                    this.set_preview_volume_at(ev.position.x);
                    cx.notify();
                }),
            )
            .child(
                canvas(
                    move |b, _, _| vslot.set(b),
                    move |b, _, window, _| {
                        let r = R::new(
                            f32::from(b.origin.x),
                            f32::from(b.origin.y),
                            f32::from(b.size.width),
                            f32::from(b.size.height),
                        );
                        paint_slider(window, r, volume, &theme);
                    },
                )
                .size_full(),
            );

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
                .bg(h(if playing_this { t.accent } else { t.bg_control }))
                .text_color(h(if playing_this {
                    t.text_on_accent
                } else {
                    t.text
                }))
                .hover(|s| s.opacity(0.9))
                .cursor_pointer()
                .child(icon_fill(if playing_this {
                    icons::STOP
                } else {
                    icons::PLAY
                })),
            cx,
            move |this, _| {
                if playing_this {
                    this.state.pause_preview();
                } else {
                    this.state.play_preview(path.clone());
                }
            },
        );

        div()
            .h(px(PREVIEW_H))
            .flex_none()
            .flex()
            .flex_col()
            .gap_1()
            .border_t_1()
            .border_color(h(t.separator))
            .bg(h(t.bg_panel))
            .child(
                div()
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
                            .text_color(h(t.text_muted))
                            .child(format!("{} / {}", short_time(seconds(position)), length)),
                    ),
            )
            .child(
                div()
                    .px_2()
                    .text_size(px(size::SMALL))
                    .text_color(h(t.text_muted))
                    .child(format(&audio)),
            )
            .child(waveform)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(play)
                    .child(toggle_button(
                        "auto-preview",
                        icon(icons::HEADPHONES),
                        self.browser.auto_preview,
                        &t,
                        cx,
                        |this, _| this.browser.auto_preview = !this.browser.auto_preview,
                    ))
                    .child(div().flex_1())
                    .child(on_press(
                        div()
                            .id("preview-mute")
                            .text_size(px(size::HEADING))
                            .text_color(h(if self.browser.muted {
                                t.warning
                            } else {
                                t.text_muted
                            }))
                            .hover(|s| s.opacity(0.8))
                            .cursor_pointer()
                            .child(icon(if self.browser.muted || self.browser.volume <= 0. {
                                icons::SPEAKER_SLASH
                            } else {
                                icons::SPEAKER_LOW
                            })),
                        cx,
                        |this, _| {
                            this.browser.muted = !this.browser.muted;
                            this.apply_preview_gain();
                        },
                    ))
                    .child(volume_slider),
            )
            .into_any_element()
    }
}

/// `m:ss`.
fn short_time(seconds: f64) -> String {
    let mut s = format_time(seconds);
    s.truncate(s.len().saturating_sub(4));
    s
}
