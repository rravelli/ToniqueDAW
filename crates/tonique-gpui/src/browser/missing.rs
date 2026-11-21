//! Audio files the project uses that weren't found, in a tab of their
//! own: each looked for in the browser's folders by name, to replace with
//! one of the closest matches, or picked on disk. Their clips are kept,
//! silent, until then.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui::{
    AnyElement, Context, KeyDownEvent, MouseButton, PathPromptOptions, Window, div, prelude::*, px,
};
use tonique_engine::edit::SourceId;

use crate::{
    app::Tonique,
    core::state::ProjectState,
    file_match::{FileIndex, exact_matches, file_name, folder_name, query_for, rank},
    icons,
    theme::{Theme, hsla, icon, mix, size},
    widgets::{FieldKey, Flat, TextField},
};

const ROW_HEIGHT: f32 = 22.;
/// How much of the accent the selected file is tinted with.
const SELECTED_TINT: f32 = 0.25;

/// What the last replacement did.
enum Status {
    Located(usize),
    Failed(String),
}

pub struct MissingView {
    selected: Option<SourceId>,
    search: TextField,
    index: FileIndex,
    status: Option<Status>,
}

impl MissingView {
    pub fn new(context: &mut Context<Tonique>) -> Self {
        Self {
            selected: None,
            search: TextField::new(context),
            index: FileIndex::default(),
            status: None,
        }
    }

    /// Select the missing file at `path`.
    pub fn select_path(&mut self, state: &ProjectState, path: &Path) {
        let found = state
            .missing_files()
            .into_iter()
            .find(|(_, audio)| audio.path == path)
            .map(|(id, audio)| (id, audio.path.clone()));
        if let Some((id, path)) = found {
            self.select(id, &path);
        }
    }

    fn select(&mut self, id: SourceId, path: &Path) {
        self.selected = Some(id);
        self.search.text = query_for(path);
    }
}

/// A heading, in small capitals.
fn heading(text: &str, theme: &Theme) -> gpui::Div {
    div()
        .pt(px(6.))
        .pb(px(2.))
        .text_size(px(size::SMALL))
        .text_color(hsla(theme.text_muted))
        .child(text.to_uppercase())
}

fn note(text: impl Into<gpui::SharedString>, theme: &Theme) -> gpui::Div {
    div().text_color(hsla(theme.text_muted)).child(text.into())
}

/// A row of a list: `glyph` in `tint`, then `text`.
fn list_row(
    id: impl Into<gpui::ElementId>,
    glyph: &'static str,
    tint: crate::theme::Color,
    text: String,
    selected: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(ROW_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap_1()
        .px_1()
        .rounded(px(2.))
        .when(selected, |row| {
            row.bg(hsla(mix(theme.bg_panel, theme.accent, SELECTED_TINT)))
        })
        .hover(|style| style.bg(hsla(theme.hover_overlay)))
        .cursor_pointer()
        .whitespace_nowrap()
        .overflow_hidden()
        .child(icon(glyph).text_color(hsla(tint)))
        .child(div().flex_1().min_w_0().overflow_hidden().child(text))
}

impl Tonique {
    pub(super) fn missing_view(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let theme = self.theme.clone();
        let folders = self.browser.folders();
        let view = &mut self.browser.missing;
        view.index.update(&folders);
        let search_bar = self.search_bar(
            "missing-search",
            &self.browser.missing.search,
            "Search your folders",
            self.browser.missing.index.is_scanning(),
            &theme,
            window,
            context,
            Self::missing_search_key,
            |this, _| this.browser.missing.search.text.clear(),
        );
        let missing: Vec<(SourceId, String, PathBuf)> = self
            .state
            .missing_files()
            .into_iter()
            .map(|(id, audio)| (id, audio.name.clone(), audio.path.clone()))
            .collect();
        let body = div()
            .id("missing-files")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .px_2();
        if missing.is_empty() {
            self.browser.missing.selected = None;
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .child(search_bar)
                .child(
                    body.items_center()
                        .gap_1()
                        .pt(px(12.))
                        .child(
                            icon(icons::CHECK_CIRCLE)
                                .text_size(px(20.))
                                .text_color(hsla(theme.text_muted)),
                        )
                        .child(note("Every audio file is found.", &theme)),
                )
                .into_any_element();
        }
        // The first one, unless one still missing is selected.
        let view = &mut self.browser.missing;
        if !missing.iter().any(|(id, ..)| Some(*id) == view.selected) {
            let (id, _, path) = &missing[0];
            view.select(*id, path);
        }
        let body = body
            .child(heading(&format!("Missing ({})", missing.len()), &theme))
            .children(self.missing_rows(&missing, context))
            .child(self.missing_buttons(&missing, context))
            .children(self.missing_status())
            .children(self.missing_matches(&missing, &folders, context));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(search_bar)
            .child(body)
            .into_any_element()
    }

    fn missing_search_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let search = &mut self.browser.missing.search;
        match search.key(event) {
            FieldKey::Edited => {}
            FieldKey::Submit => window.focus(&self.focus, context),
            FieldKey::Cancel => {
                search.text.clear();
                window.focus(&self.focus, context);
            }
            FieldKey::Ignored => return,
        }
        context.stop_propagation();
        context.notify();
    }

    fn missing_rows(
        &self,
        missing: &[(SourceId, String, PathBuf)],
        context: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = &self.theme;
        missing
            .iter()
            .map(|(id, name, path)| {
                let (id, path) = (*id, path.clone());
                let selected = Some(id) == self.browser.missing.selected;
                list_row(
                    ("missing", id.0 as usize),
                    icons::WARNING,
                    theme.warning,
                    name.clone(),
                    selected,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, _, context| {
                        let view = &mut this.browser.missing;
                        view.select(id, &path);
                        view.status = None;
                        context.notify();
                    }),
                )
                .into_any_element()
            })
            .collect()
    }

    /// Finding them all by name, or the selected one on disk.
    fn missing_buttons(
        &self,
        missing: &[(SourceId, String, PathBuf)],
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = &self.theme;
        let exact = exact_matches(missing, self.browser.missing.index.files());
        let found_some = !exact.is_empty();
        div()
            .pt(px(6.))
            .flex()
            .gap_1()
            .child(
                Flat::new("find-all", 22.)
                    .fit(8.)
                    .enabled(found_some)
                    .element("Find all", theme)
                    .when(found_some, |button| {
                        button.on_mouse_down(
                            MouseButton::Left,
                            context.listener(move |this, _, _, context| {
                                let mut located = 0;
                                for (id, path) in &exact {
                                    // Already found along with another, from
                                    // its folder.
                                    let still = this
                                        .state
                                        .missing_files()
                                        .iter()
                                        .any(|(missing, _)| missing == id);
                                    if still {
                                        located += this.state.locate_file(*id, path).unwrap_or(0);
                                    }
                                }
                                this.browser.missing.status = Some(Status::Located(located));
                                context.notify();
                            }),
                        )
                    }),
            )
            .child(
                Flat::new("locate", 22.)
                    .fit(8.)
                    .element("Locate on disk…", theme)
                    .on_mouse_down(
                        MouseButton::Left,
                        context.listener(|this, _, window, context| {
                            this.locate_on_disk(window, context);
                        }),
                    ),
            )
    }

    /// Lets the user pick the selected missing file on disk.
    fn locate_on_disk(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let Some(id) = self.browser.missing.selected else {
            return;
        };
        let picked = context.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Locate".into()),
        });
        context
            .spawn_in(window, async move |this, context| {
                let Ok(Ok(Some(paths))) = picked.await else {
                    return;
                };
                let Some(path) = paths.into_iter().next() else {
                    return;
                };
                this.update(context, |this, context| {
                    this.locate_missing(id, &path);
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    fn locate_missing(&mut self, id: SourceId, path: &Path) {
        self.browser.missing.status = Some(match self.state.locate_file(id, path) {
            Ok(located) => Status::Located(located),
            Err(error) => Status::Failed(error),
        });
    }

    fn missing_status(&self) -> Option<impl IntoElement> {
        let theme = &self.theme;
        let (text, color) = match self.browser.missing.status.as_ref()? {
            Status::Located(0) => ("No file found.".to_string(), theme.warning),
            Status::Located(1) => ("Found 1 file.".to_string(), theme.text_muted),
            Status::Located(count) => (format!("Found {count} files."), theme.text_muted),
            Status::Failed(error) => (error.clone(), theme.danger),
        };
        Some(div().pt_1().text_color(hsla(color)).child(text))
    }

    /// The files like the selected one in the browser's folders, best
    /// first: one clicked replaces it.
    fn missing_matches(
        &self,
        missing: &[(SourceId, String, PathBuf)],
        folders: &[PathBuf],
        context: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = &self.theme;
        let view = &self.browser.missing;
        let Some((selected, name, path)) = missing
            .iter()
            .find(|(id, ..)| Some(*id) == view.selected)
            .cloned()
        else {
            return Vec::new();
        };
        let mut shown = vec![heading(&format!("Replace {name} with"), theme).into_any_element()];
        if folders.is_empty() {
            shown.push(
                note(
                    "Add your sample folders in the browser to search them.",
                    theme,
                )
                .into_any_element(),
            );
            return shown;
        }
        let matches = rank(&path, &view.search.text, view.index.files());
        if matches.is_empty() {
            let text = if view.index.is_scanning() {
                "Searching your folders…"
            } else {
                "Nothing alike in your folders."
            };
            shown.push(note(text, theme).into_any_element());
            return shown;
        }
        // Names alike tell their folders.
        let mut names: HashMap<&str, usize> = HashMap::new();
        for found in &matches {
            *names.entry(file_name(found)).or_default() += 1;
        }
        for (slot, found) in matches.iter().enumerate() {
            let name = file_name(found);
            let text = if names[name] > 1 {
                format!("{name} · {}", folder_name(found))
            } else {
                name.to_string()
            };
            let found = found.clone();
            shown.push(
                list_row(
                    ("match", slot),
                    icons::WAVEFORM,
                    theme.text_muted,
                    text,
                    false,
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    context.listener(move |this, _, _, context| {
                        this.locate_missing(selected, &found);
                        context.notify();
                    }),
                )
                .into_any_element(),
            );
        }
        shown
    }
}
