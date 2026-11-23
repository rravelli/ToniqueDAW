//! What to export, the range and format, and where the files go.

use crate::model::HoldsProject;
use std::{collections::HashSet, path::PathBuf};

use gpui::{
    Context, KeyDownEvent, MouseButton, PathPromptOptions, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use tonique_engine::{sample::WavFormat, time::BeatPos};

use crate::{
    app::{export::ExportDialog, settings::section_header},
    core::{
        export::{
            ExportFile, ExportRange,
            format::{audio_format, duration, file_name, file_size, sample_bytes},
        },
        track::{TrackKind, TrackRow},
    },
    icons,
    theme::{Color, Theme, hsla, size},
    utils::{display_name, format_bars, parse_bars},
    widgets::{
        FieldKey, Flat, TextField, dialog_button, dialog_frame, glyph, on_press, typing_view,
    },
};

use super::{
    Bound, CHOICE_HEIGHT,
    parts::{choices, field_label, header, note, small},
};

/// The options' width: what to export on the left, how on the right.
const WIDTH: f32 = 720.;
const SOURCES_WIDTH: f32 = 250.;
const COLUMN_GAP: f32 = 20.;
const ROW_GAP: f32 = 8.;
/// A `bar.beat.sixteenth` field.
const POSITION_WIDTH: f32 = 88.;
const BROWSE_WIDTH: f32 = 84.;
const ROW_HEIGHT: f32 = 24.;
/// Each group level's indent in the track list.
const INDENT: f32 = 14.;
const TRACK_LIST_HEIGHT: f32 = 236.;
const CHECK_SIZE: f32 = 15.;
/// Files listed under the destination before "and n more".
const LISTED_FILES: usize = 4;
/// The furthest a custom range can end, in bars.
const MAX_BARS: f64 = 9999.;
/// The effect tails offered, in seconds.
const TAILS: [u32; 4] = [0, 2, 5, 10];

/// Which range is chosen, whatever the custom one's bounds.
#[derive(Clone, Copy, PartialEq)]
enum RangeChoice {
    Project,
    Loop,
    Custom,
}

impl ExportDialog {
    pub(super) fn options_view(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = crate::theme::current(context);
        let state = self.state(context);
        // Tracks deleted since are no longer chosen.
        let rows = state.all_rows();
        self.selection.retain(&rows);
        // Seeded once only: an end typed before the start stays, with the
        // problem shown, until the start or end is corrected.
        self.custom.get_or_insert_with(|| {
            state.export_span(ExportRange::Project).unwrap_or((
                BeatPos::ZERO,
                BeatPos(state.time_signature().beats_per_bar() * 4.),
            ))
        });
        let sources = self.selection.sources(state);
        let dir = PathBuf::from(self.folder.text.trim());
        let files = state.export_files(&dir, &self.name.text, &sources);
        let paths: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
        if self
            .existing
            .as_ref()
            .is_none_or(|(checked, _)| *checked != paths)
        {
            let existing = paths.iter().filter(|path| path.exists()).cloned().collect();
            self.existing = Some((paths, existing));
        }
        let problem = state
            .export_span(self.options.range)
            .err()
            .or_else(|| {
                sources
                    .is_empty()
                    .then(|| "Choose the mix or a track to export.".into())
            })
            .or_else(|| {
                if self.folder.text.trim().is_empty() {
                    Some("Choose a folder to export into.".into())
                } else {
                    (!dir.is_dir()).then(|| "The folder doesn't exist.".into())
                }
            });

        let subtitle = match state.export_seconds(self.options.range) {
            Ok(seconds) => format!(
                "{}  ·  {}  ·  {}",
                self.name.text,
                duration(seconds + self.options.tail_seconds as f64),
                audio_format(state)
            ),
            Err(_) => self.name.text.clone(),
        };
        let existing = self
            .existing
            .as_ref()
            .map(|(_, existing)| existing.clone())
            .unwrap_or_default();

        let columns = div()
            .flex()
            .items_start()
            .gap(px(COLUMN_GAP))
            .child(
                div()
                    .flex_none()
                    .w(px(SOURCES_WIDTH))
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .children(self.sources(&rows, &theme, context)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .children(self.settings(files.len(), &theme, context))
                    .children(self.destination(&files, &existing, &theme, window, context)),
            );

        let label = match files.len() {
            0 | 1 => "Export".to_string(),
            count => format!("Export {count} files"),
        };
        let mut start = dialog_button("export-start", label, true, &theme);
        start = if problem.is_none() {
            start.on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, window, context| {
                    context.stop_propagation();
                    this.start(window, context);
                    context.notify();
                }),
            )
        } else {
            start.opacity(0.5)
        };
        let cancel = dialog_button("export-cancel", "Cancel", false, &theme).on_mouse_down(
            MouseButton::Left,
            context.listener(|this, _, window, context| {
                context.stop_propagation();
                this.close(window, context);
                context.notify();
            }),
        );
        let footer = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .mt(px(4.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .children(problem.map(|problem| note(icons::WARNING, theme.danger, problem))),
            )
            .child(cancel)
            .child(start);

        dialog_frame(WIDTH, &theme)
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .child(header("Export audio", subtitle, &theme))
            .child(self.preview(&rows, &theme, context))
            .child(columns)
            .child(footer)
    }

    /// The mix, then every track and group, indented under their group.
    fn sources(
        &self,
        rows: &[TrackRow],
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let selection = &self.selection;
        let mix = on_press(
            source_row(
                "export-mix",
                selection.mix,
                0.,
                theme.accent,
                "Mix".into(),
                "Everything heard",
                theme,
            ),
            context,
            |this, _| this.selection.mix ^= true,
        );
        let small_button =
            |id: &'static str| Flat::ghost(id, 18.).fit(6.).enabled(!rows.is_empty());
        let all: HashSet<_> = rows.iter().map(|row| row.id).collect();
        let tracks_line = div()
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .child(small("Tracks on their own", theme.text_muted)),
            )
            .child(
                small_button("export-none")
                    .show(
                        div().text_size(px(size::LABEL)).child("None"),
                        theme,
                        context,
                        |this, _| this.selection.tracks.clear(),
                    )
                    .rounded(px(3.)),
            )
            .child(
                small_button("export-all")
                    .show(
                        div().text_size(px(size::LABEL)).child("All"),
                        theme,
                        context,
                        move |this, _| this.selection.tracks = all.clone(),
                    )
                    .rounded(px(3.)),
            );

        let list = div()
            .id("export-tracks")
            .h(px(TRACK_LIST_HEIGHT))
            .p(px(3.))
            .rounded(px(4.))
            .bg(hsla(theme.bg_deep))
            .border_1()
            .border_color(hsla(theme.border))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(1.));
        let list = if rows.is_empty() {
            list.items_center()
                .justify_center()
                .text_color(hsla(theme.text_disabled))
                .child("No tracks yet")
        } else {
            list.children(rows.iter().enumerate().map(|(index, row)| {
                let detail = match (row.kind, row.muted) {
                    (TrackKind::Group, true) => "Group  ·  muted",
                    (TrackKind::Group, false) => "Group",
                    (_, true) => "Muted",
                    _ => "",
                };
                let id = row.id;
                let element = source_row(
                    ("export-track", index),
                    selection.tracks.contains(&id),
                    row.depth as f32 * INDENT,
                    row.color,
                    display_name(&row.name, row.first_track_index).into(),
                    detail,
                    theme,
                );
                on_press(element, context, move |this, _| {
                    let tracks = &mut this.selection.tracks;
                    if !tracks.remove(&id) {
                        tracks.insert(id);
                    }
                })
            }))
        };
        vec![
            section_header("Export", theme).into_any_element(),
            mix.into_any_element(),
            tracks_line.into_any_element(),
            list.into_any_element(),
        ]
    }

    /// Range, tail and format, with what the files will be like.
    fn settings(
        &self,
        files: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let state = self.state(context);
        let options = self.options;
        let length = |range| {
            state
                .export_seconds(range)
                .map_or_else(|_| "empty".to_string(), duration)
        };
        let (project_length, loop_length) =
            (length(ExportRange::Project), length(ExportRange::Loop));
        let mut details = format!("WAV  ·  {}", audio_format(state));
        if let Ok(seconds) = state.export_seconds(options.range) {
            let config = state.engine_config();
            let seconds = seconds + options.tail_seconds as f64;
            let bytes = seconds
                * config.sample_rate
                * (config.output_channels * sample_bytes(options.format)) as f64;
            details += &match files {
                0 | 1 => format!("  ·  ≈ {}", file_size(bytes)),
                count => format!(
                    "  ·  ≈ {} each, {} in all",
                    file_size(bytes),
                    file_size(bytes * count as f64)
                ),
            };
        }
        let choice = match options.range {
            ExportRange::Project => RangeChoice::Project,
            ExportRange::Loop => RangeChoice::Loop,
            ExportRange::Custom(..) => RangeChoice::Custom,
        };
        let ranges = choices(
            "export-range",
            choice,
            vec![
                (
                    RangeChoice::Project,
                    format!("Whole project  ·  {project_length}"),
                ),
                (RangeChoice::Loop, format!("Loop region  ·  {loop_length}")),
                (RangeChoice::Custom, "Custom".to_string()),
            ],
            theme,
            context,
            |this, choice| {
                this.options.range = match choice {
                    RangeChoice::Project => ExportRange::Project,
                    RangeChoice::Loop => ExportRange::Loop,
                    RangeChoice::Custom => {
                        let (start, end) = this.custom.unwrap_or_default();
                        ExportRange::Custom(start, end)
                    }
                };
            },
        );
        let tails = choices(
            "export-tail",
            options.tail_seconds,
            TAILS
                .iter()
                .map(|&seconds| {
                    let label = match seconds {
                        0 => "None".to_string(),
                        seconds => format!("{seconds} s"),
                    };
                    (seconds, label)
                })
                .collect(),
            theme,
            context,
            |this, seconds| this.options.tail_seconds = seconds,
        );
        let formats = choices(
            "export-format",
            options.format,
            vec![
                (WavFormat::Int16, "16-bit".to_string()),
                (WavFormat::Int24, "24-bit".to_string()),
                (WavFormat::Float32, "32-bit float".to_string()),
            ],
            theme,
            context,
            |this, format| this.options.format = format,
        );

        let mut elements = vec![
            section_header("Range", theme).into_any_element(),
            ranges.into_any_element(),
        ];
        if choice == RangeChoice::Custom {
            elements.push(self.custom_range(theme, context).into_any_element());
        }
        elements.extend([
            section_header("Effect tail", theme).into_any_element(),
            tails.into_any_element(),
            section_header("Format", theme).into_any_element(),
            formats.into_any_element(),
            small(details, theme.text_muted).into_any_element(),
        ]);
        elements
    }

    /// The custom range's start and end, as `bar.beat.sixteenth`, and how
    /// long it is.
    fn custom_range(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let (start, end) = self.custom.unwrap_or_default();
        let seconds = self
            .state(context)
            .export_seconds(ExportRange::Custom(start, end))
            .ok();
        div()
            .flex()
            .items_center()
            .child(field_label("Start", theme))
            .child(self.bound_field(Bound::Start, start, theme, context))
            .child(div().w(px(8.)))
            .child(field_label("End", theme))
            .child(self.bound_field(Bound::End, end, theme, context))
            .children(seconds.map(|seconds| small(duration(seconds), theme.text_muted).ml(px(6.))))
    }

    /// A bound of the custom range: pressed, it's typed in.
    fn bound_field(
        &self,
        bound: Bound,
        beats: BeatPos,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = match bound {
            Bound::Start => "export-start-bound",
            Bound::End => "export-end-bound",
        };
        if let Some((_, field)) = self.typing.as_ref().filter(|(typed, _)| *typed == bound) {
            return typing_view(
                id,
                field,
                theme,
                context,
                |this| this.typing.as_mut().map(|(_, field)| field),
                Self::finish_bound_typing,
            )
            .flex_none()
            .w(px(POSITION_WIDTH))
            .h(px(CHOICE_HEIGHT))
            .into_any_element();
        }
        let text = format_bars(beats.0, self.state(context).time_signature());
        let typed = text.clone();
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(POSITION_WIDTH))
            .h(px(CHOICE_HEIGHT))
            .rounded(px(2.))
            .bg(hsla(theme.bg_control))
            .hover(|style| style.bg(hsla(theme.bg_control_hover)))
            .cursor_text()
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, window, context| {
                    context.stop_propagation();
                    let mut field = TextField::new(context);
                    field.start(typed.clone(), window, context);
                    this.typing = Some((bound, field));
                    context.notify();
                }),
            )
            .into_any_element()
    }

    /// Takes the typed bound, on whole sixteenths, if it reads as a
    /// position.
    fn finish_bound_typing(
        &mut self,
        keep: bool,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let Some((bound, field)) = self.typing.take() else {
            return;
        };
        crate::app::focus_main(window, context);
        let signature = self.state(context).time_signature();
        let Some(beats) = parse_bars(&field.text, signature).filter(|_| keep) else {
            return;
        };
        let beats =
            BeatPos(((beats * 4.).round() / 4.).clamp(0., MAX_BARS * signature.beats_per_bar()));
        let (start, end) = self.custom.unwrap_or_default();
        self.set_custom(match bound {
            Bound::Start => (beats, end),
            Bound::End => (start, beats),
        });
    }

    /// The folder and the name the files are named after, and the files
    /// that gives.
    fn destination(
        &self,
        files: &[ExportFile],
        existing: &HashSet<PathBuf>,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let browse = Flat::new("export-browse", CHOICE_HEIGHT)
            .width(BROWSE_WIDTH)
            .show(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(glyph(icons::FOLDER_SIMPLE, size::BODY, false))
                    .child("Browse…"),
                theme,
                context,
                |this, context| this.pick_folder(context),
            )
            .rounded(px(3.));
        let folder = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(field_label("Folder", theme))
            .child(input(
                "export-folder",
                &self.folder,
                "Choose a folder…",
                theme,
                window,
                context,
                |this| &mut this.folder,
            ))
            .child(browse);
        let name = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(field_label("Name", theme))
            .child(input(
                "export-name",
                &self.name,
                "Untitled",
                theme,
                window,
                context,
                |this| &mut this.name,
            ));

        // What gets written, and what that replaces.
        let listed = files.iter().take(LISTED_FILES).map(|file| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    glyph(icons::WAVEFORM, size::BODY, false).text_color(hsla(theme.text_disabled)),
                )
                .child(small(file_name(&file.path), theme.text_muted))
                .when(existing.contains(&file.path), |element| {
                    element.child(small("replaces the existing file", theme.warning))
                })
        });
        let more = (files.len() > LISTED_FILES).then(|| {
            let replaced = files[LISTED_FILES..]
                .iter()
                .filter(|file| existing.contains(&file.path))
                .count();
            let mut more = format!("and {} more", files.len() - LISTED_FILES);
            if replaced > 0 {
                more += &format!(" ({replaced} replacing existing files)");
            }
            small(more, theme.text_disabled)
        });
        let list = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(listed)
            .children(more);
        vec![
            section_header("Destination", theme).into_any_element(),
            folder.into_any_element(),
            name.into_any_element(),
            list.into_any_element(),
        ]
    }

    /// Lets the user pick the folder to export into.
    fn pick_folder(&mut self, context: &mut Context<Self>) {
        let picked = context.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export here".into()),
        });
        context
            .spawn(async move |this, context| {
                let Ok(Ok(Some(paths))) = picked.await else {
                    return;
                };
                let Some(folder) = paths.into_iter().next() else {
                    return;
                };
                this.update(context, |this, context| {
                    this.folder.text = folder.display().to_string();
                    context.notify();
                })
                .ok();
            })
            .detach();
    }
}

/// A row to tick: a check box, a swatch of `color`, the name and, muted
/// after it, `detail`.
fn source_row(
    id: impl Into<gpui::ElementId>,
    checked: bool,
    indent: f32,
    color: Color,
    name: SharedString,
    detail: &'static str,
    theme: &Theme,
) -> Stateful<gpui::Div> {
    let (check, ink) = if checked {
        (glyph(icons::CHECK_SQUARE, CHECK_SIZE, true), theme.accent)
    } else {
        (glyph(icons::SQUARE, CHECK_SIZE, false), theme.text_muted)
    };
    let text = if checked {
        theme.text
    } else {
        theme.text_muted
    };
    let hover = theme.hover_overlay;
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .h(px(ROW_HEIGHT))
        .pl(px(6. + indent))
        .pr(px(6.))
        .rounded(px(3.))
        .overflow_hidden()
        .whitespace_nowrap()
        .hover(move |style| style.bg(hsla(hover)))
        .cursor_pointer()
        .child(check.flex_none().w(px(22.)).text_color(hsla(ink)))
        .child(
            div()
                .flex_none()
                .w(px(3.))
                .h(px(12.))
                .mr(px(6.))
                .rounded(px(1.))
                .bg(hsla(color)),
        )
        .child(div().text_color(hsla(text)).child(name))
        .when(!detail.is_empty(), |element| {
            element.child(small(detail, theme.text_disabled).ml(px(8.)))
        })
}

/// A line of text to type in, pressed to take the keyboard; Enter or
/// Escape give it back. `get` finds its field.
fn input(
    id: &'static str,
    field: &TextField,
    placeholder: &str,
    theme: &Theme,
    window: &Window,
    context: &mut Context<ExportDialog>,
    get: fn(&mut ExportDialog) -> &mut TextField,
) -> Stateful<gpui::Div> {
    let border = if field.focus.is_focused(window) {
        theme.accent
    } else {
        theme.border
    };
    div()
        .id(id)
        .track_focus(&field.focus)
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .h(px(CHOICE_HEIGHT))
        .px(px(4.))
        .rounded(px(2.))
        .bg(hsla(theme.bg_deep))
        .border_1()
        .border_color(hsla(border))
        .cursor_text()
        .child(field.view(placeholder, theme, window))
        .on_mouse_down(
            MouseButton::Left,
            context.listener(move |this, _, window, context| {
                context.stop_propagation();
                window.focus(&get(this).focus, context);
                context.notify();
            }),
        )
        .on_key_down(
            context.listener(move |this, event: &KeyDownEvent, window, context| {
                match get(this).key(event) {
                    FieldKey::Edited => {}
                    FieldKey::Submit | FieldKey::Cancel => crate::app::focus_main(window, context),
                    // Shortcuts go on to the app; other keys stay in the field.
                    FieldKey::Ignored => {
                        let modifiers = &event.keystroke.modifiers;
                        if modifiers.control || modifiers.platform {
                            return;
                        }
                    }
                }
                context.stop_propagation();
                context.notify();
            }),
        )
}
