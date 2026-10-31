//! Export audio: what to export (the mix, tracks on their own) beside the
//! range, format and destination, with a preview of what's exported; then
//! each file drawing itself while it renders, and the result.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use egui::{
    Align, Align2, Color32, CursorIcon, DragValue, FontFamily, FontId, Layout, Mesh, Painter, Pos2,
    Rect, Response, RichText, ScrollArea, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, pos2,
    vec2,
};
use egui_phosphor::regular::{
    CHECK, CHECK_SQUARE, CIRCLE, CIRCLE_NOTCH, FOLDER_SIMPLE, SQUARE, WARNING, WAVEFORM,
};
use rfd::FileDialog;
use tonique_engine::{edit::TrackId, sample::WavFormat, time::BeatPos};

use crate::{
    core::{
        export::{Export, ExportFile, ExportOptions, ExportOutcome, ExportRange, Waveform},
        project::project_name,
        state::{ExportSource, ProjectState},
        track::{TrackKind, TrackRow},
    },
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        project::ProjectManager,
        taskbar::TaskbarProgress,
        theme::{Theme, ThemeExt, font_size, with_alpha},
        waveform::paint_waveform,
        widget::{
            dialog::{self, button},
            flat_button::FlatButton,
            section::SectionHeader,
        },
    },
    utils::{display_name, format_bars, parse_bars},
};

/// The options' width: what to export on the left, how on the right.
const WIDTH: f32 = 720.;
const SOURCES_WIDTH: f32 = 250.;
const COLUMN_GAP: f32 = 20.;
/// Progress and results.
const NARROW_WIDTH: f32 = 480.;
const PREVIEW_HEIGHT: f32 = 96.;
const WAVEFORM_HEIGHT: f32 = 72.;
/// A file's waveform in a list of several.
const THUMBNAIL_HEIGHT: f32 = 26.;
const CHOICE_HEIGHT: f32 = 26.;
/// Room for a field's name before it.
const FIELD_LABEL_WIDTH: f32 = 48.;
/// A `bar.beat.sixteenth` field.
const POSITION_WIDTH: f32 = 88.;
const ROW_HEIGHT: f32 = 24.;
/// Each group level's indent in the track list.
const INDENT: f32 = 14.;
const TRACK_LIST_HEIGHT: f32 = 236.;
/// Files listed under the destination before "and n more".
const LISTED_FILES: usize = 4;
/// Space around the preview's lanes and between them.
const LANE_PADDING: f32 = 6.;
const LANE_GAP: f32 = 2.;
const MAX_LANE_HEIGHT: f32 = 16.;
/// Under the lanes, for the times.
const LABEL_STRIP: f32 = 10.;
/// The effect tails offered, in seconds.
const TAILS: [u32; 4] = [0, 2, 5, 10];

enum Step {
    Closed,
    /// Choosing what to export and where.
    Options,
    Running(Export),
    Done {
        files: Vec<ExportFile>,
        waveforms: Vec<Waveform>,
        seconds: f64,
    },
    Failed {
        title: String,
        detail: String,
    },
}

/// What's chosen to export, kept between exports.
struct Selection {
    mix: bool,
    tracks: HashSet<TrackId>,
}

impl Selection {
    /// The files to write, in the track list's order, the mix first.
    fn sources(&self, state: &ProjectState) -> Vec<ExportSource> {
        let tracks = state
            .all_rows()
            .into_iter()
            .filter(|track| self.tracks.contains(&track.id))
            .map(|track| ExportSource::Track(track.id));
        self.mix
            .then_some(ExportSource::Mix)
            .into_iter()
            .chain(tracks)
            .collect()
    }

    /// Whether `track` is heard in what's exported: in the mix, or chosen
    /// with or inside a chosen group.
    fn includes(&self, state: &ProjectState, track: &TrackRow) -> bool {
        if self.mix {
            return !track.is_silenced();
        }
        let mut id = Some(track.id);
        while let Some(current) = id {
            if self.tracks.contains(&current) {
                return true;
            }
            id = state.parent(current);
        }
        false
    }
}

pub struct ExportWindow {
    step: Step,
    /// Kept between exports.
    options: ExportOptions,
    selection: Selection,
    /// Where the files go: the project's folder unless another was chosen.
    dir: String,
    /// What the files are named after: the project, at first.
    name: String,
    /// The custom range, kept while another is chosen; the whole project
    /// at first.
    custom: (BeatPos, BeatPos),
    taskbar: TaskbarProgress,
}

impl ExportWindow {
    pub fn new() -> Self {
        Self {
            step: Step::Closed,
            options: ExportOptions::default(),
            selection: Selection {
                mix: true,
                tracks: HashSet::new(),
            },
            dir: String::new(),
            name: String::new(),
            custom: (BeatPos::ZERO, BeatPos::ZERO),
            taskbar: TaskbarProgress::new(),
        }
    }

    /// Ask what to export. Ignored while an export runs.
    pub fn open(&mut self, project: &ProjectManager) {
        if matches!(self.step, Step::Running(_)) {
            return;
        }
        self.name = project.name();
        if let Some(dir) = project.dir() {
            self.dir = dir.display().to_string();
        }
        self.step = Step::Options;
    }

    pub fn show(&mut self, ui: &Ui, state: &ProjectState) {
        let next = match &mut self.step {
            Step::Closed => None,
            Step::Options => self.options_ui(ui, state),
            Step::Running(export) => running_ui(ui, export),
            Step::Done {
                files,
                waveforms,
                seconds,
            } => done_ui(ui, files, waveforms, *seconds),
            Step::Failed { title, detail } => failed_ui(ui, title, detail),
        };
        if let Some(next) = next {
            self.step = next;
        }
        self.taskbar.set(match &self.step {
            Step::Running(export) => Some(export.progress()),
            _ => None,
        });
    }

    fn options_ui(&mut self, ui: &Ui, state: &ProjectState) -> Option<Step> {
        let theme = ui.app_theme();
        // Tracks deleted since are no longer chosen.
        let rows = state.all_rows();
        self.selection
            .tracks
            .retain(|id| rows.iter().any(|row| row.id == *id));
        if self.custom.1.0 <= self.custom.0.0 {
            self.custom = state.export_span(ExportRange::Project).unwrap_or((
                BeatPos::ZERO,
                BeatPos(state.time_signature().beats_per_bar() * 4.),
            ));
        }
        let sources = self.selection.sources(state);
        let dir = PathBuf::from(self.dir.trim());
        let files = state.export_files(&dir, &self.name, &sources);
        let problem = state
            .export_span(self.options.range)
            .err()
            .or_else(|| {
                sources
                    .is_empty()
                    .then(|| "Choose the mix or a track to export.".into())
            })
            .or_else(|| {
                if self.dir.trim().is_empty() {
                    Some("Choose a folder to export into.".into())
                } else {
                    (!dir.is_dir()).then(|| "The folder doesn't exist.".into())
                }
            });

        let mut next = None;
        let modal = dialog::modal(ui, "export-options").show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing = vec2(6., 8.);
            let subtitle = match state.export_seconds(self.options.range) {
                Ok(seconds) => format!(
                    "{}  ·  {}  ·  {}",
                    self.name,
                    duration(seconds + self.options.tail_seconds as f64),
                    audio_format(state)
                ),
                Err(_) => self.name.clone(),
            };
            header(ui, "Export audio", &subtitle);
            if let Some(dragged) =
                arrangement_preview(ui, state, &rows, &self.selection, self.options.range)
            {
                self.custom = dragged;
                self.options.range = ExportRange::Custom(dragged.0, dragged.1);
            }
            ui.add_space(2.);

            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = COLUMN_GAP;
                ui.vertical(|ui| {
                    ui.set_width(SOURCES_WIDTH);
                    ui.spacing_mut().item_spacing.y = 8.;
                    sources_ui(ui, &rows, &mut self.selection);
                });
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.;
                    settings_ui(ui, state, &mut self.options, &mut self.custom, files.len());
                    destination_ui(ui, &mut self.dir, &mut self.name, &files);
                });
            });

            ui.add_space(4.);
            ui.horizontal(|ui| {
                if let Some(problem) = &problem {
                    note(ui, WARNING, theme.danger, problem);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let label = match files.len() {
                        0 | 1 => "Export".to_string(),
                        n => format!("Export {n} files"),
                    };
                    let export = ui.add_enabled(problem.is_none(), button(&label).selected(true));
                    if export.clicked() {
                        next = Some(start(state, &dir, &self.name, &sources, self.options));
                    }
                    if ui.add(button("Cancel")).clicked() {
                        next = Some(Step::Closed);
                    }
                });
            });
        });
        if next.is_none() && modal.should_close() {
            next = Some(Step::Closed);
        }
        next
    }
}

/// The mix, then every track and group, indented under their group.
fn sources_ui(ui: &mut Ui, rows: &[TrackRow], selection: &mut Selection) {
    let theme = ui.app_theme();
    ui.add(SectionHeader::new("Export"));
    let mix = source_row(
        ui,
        selection.mix,
        0.,
        theme.accent,
        "Mix",
        "Everything heard",
    );
    if mix.clicked() {
        selection.mix = !selection.mix;
    }

    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Tracks on their own")
                .color(theme.text_muted)
                .size(font_size::LABEL),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.;
            let small = |text| {
                FlatButton::ghost(text)
                    .font(FontId::proportional(font_size::LABEL))
                    .padding(6.)
                    .border_radius(3.)
            };
            if ui.add_enabled(!rows.is_empty(), small("None")).clicked() {
                selection.tracks.clear();
            }
            if ui.add_enabled(!rows.is_empty(), small("All")).clicked() {
                selection.tracks = rows.iter().map(|row| row.id).collect();
            }
        });
    });

    let (frame, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), TRACK_LIST_HEIGHT),
        Sense::hover(),
    );
    ui.painter().rect(
        frame,
        4.,
        theme.bg_deep,
        Stroke::new(1., theme.border),
        StrokeKind::Inside,
    );
    let mut list = ui.new_child(egui::UiBuilder::new().max_rect(frame.shrink(3.)));
    if rows.is_empty() {
        list.centered_and_justified(|ui| {
            ui.label(RichText::new("No tracks yet").color(theme.text_disabled));
        });
        return;
    }
    ScrollArea::vertical()
        .id_salt("export-tracks")
        .auto_shrink(false)
        .show(&mut list, |ui| {
            ui.spacing_mut().item_spacing.y = 1.;
            for row in rows {
                let detail = match (row.kind, row.muted) {
                    (TrackKind::Group, true) => "Group  ·  muted",
                    (TrackKind::Group, false) => "Group",
                    (_, true) => "Muted",
                    _ => "",
                };
                let checked = selection.tracks.contains(&row.id);
                let response = source_row(
                    ui,
                    checked,
                    row.depth as f32 * INDENT,
                    row.color,
                    &display_name(&row.name, row.first_track_index),
                    detail,
                );
                if response.clicked() {
                    if checked {
                        selection.tracks.remove(&row.id);
                    } else {
                        selection.tracks.insert(row.id);
                    }
                }
            }
        });
}

/// A row to tick: a check box, a swatch of `color`, the name and, muted
/// after it, `detail`.
fn source_row(
    ui: &mut Ui,
    checked: bool,
    indent: f32,
    color: Color32,
    name: &str,
    detail: &str,
) -> Response {
    let theme = ui.app_theme();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let painter = ui.painter_at(rect);
    if response.hovered() {
        painter.rect_filled(rect, 3., theme.hover_overlay);
    }
    let mut x = rect.left() + 6. + indent;
    let (icon, family, ink) = if checked {
        (CHECK_SQUARE, PHOSPHOR_FILL, theme.accent)
    } else {
        (SQUARE, PHOSPHOR_REGULAR, theme.text_muted)
    };
    painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        icon,
        FontId::new(15., FontFamily::Name(family.into())),
        ink,
    );
    x += 22.;
    let swatch = Rect::from_center_size(pos2(x + 1.5, rect.center().y), vec2(3., 12.));
    painter.rect_filled(swatch, 1., color);
    x += 9.;
    let text = if checked {
        theme.text
    } else {
        theme.text_muted
    };
    let name = painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(font_size::BODY),
        text,
    );
    if !detail.is_empty() {
        painter.text(
            pos2(name.right() + 8., rect.center().y),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(font_size::LABEL),
            theme.text_disabled,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// Which range is chosen, whatever the custom one's bounds.
#[derive(Clone, Copy, PartialEq)]
enum RangeChoice {
    Project,
    Loop,
    Custom,
}

/// Range, tail and format, with what the files will be like.
fn settings_ui(
    ui: &mut Ui,
    state: &ProjectState,
    options: &mut ExportOptions,
    custom: &mut (BeatPos, BeatPos),
    files: usize,
) {
    let theme = ui.app_theme();
    ui.add(SectionHeader::new("Range"));
    let length = |range| {
        state
            .export_seconds(range)
            .map_or_else(|_| "empty".to_string(), duration)
    };
    let mut choice = match options.range {
        ExportRange::Project => RangeChoice::Project,
        ExportRange::Loop => RangeChoice::Loop,
        ExportRange::Custom(..) => RangeChoice::Custom,
    };
    choices(
        ui,
        &mut choice,
        [
            (
                RangeChoice::Project,
                format!("Whole project  ·  {}", length(ExportRange::Project)),
            ),
            (
                RangeChoice::Loop,
                format!("Loop region  ·  {}", length(ExportRange::Loop)),
            ),
            (RangeChoice::Custom, "Custom".to_string()),
        ],
    );
    if choice == RangeChoice::Custom {
        custom_range_ui(ui, state, custom);
    }
    options.range = match choice {
        RangeChoice::Project => ExportRange::Project,
        RangeChoice::Loop => ExportRange::Loop,
        RangeChoice::Custom => ExportRange::Custom(custom.0, custom.1),
    };

    ui.add(SectionHeader::new("Effect tail"));
    choices(
        ui,
        &mut options.tail_seconds,
        TAILS.map(|seconds| {
            let label = match seconds {
                0 => "None".to_string(),
                s => format!("{s} s"),
            };
            (seconds, label)
        }),
    );

    ui.add(SectionHeader::new("Format"));
    choices(
        ui,
        &mut options.format,
        [
            (WavFormat::Int16, "16-bit".to_string()),
            (WavFormat::Int24, "24-bit".to_string()),
            (WavFormat::Float32, "32-bit float".to_string()),
        ],
    );
    let mut details = format!("WAV  ·  {}", audio_format(state));
    if let Ok(seconds) = state.export_seconds(options.range) {
        let config = state.engine_config();
        let seconds = seconds + options.tail_seconds as f64;
        let bytes = seconds
            * config.sample_rate
            * (config.output_channels * sample_bytes(options.format)) as f64;
        details += &match files {
            0 | 1 => format!("  ·  ≈ {}", file_size(bytes)),
            n => format!(
                "  ·  ≈ {} each, {} in all",
                file_size(bytes),
                file_size(bytes * n as f64)
            ),
        };
    }
    ui.label(
        RichText::new(details)
            .color(theme.text_muted)
            .size(font_size::LABEL),
    );
}

/// The custom range's start and end, as `bar.beat.sixteenth`, and how long
/// it is.
fn custom_range_ui(ui: &mut Ui, state: &ProjectState, custom: &mut (BeatPos, BeatPos)) {
    let theme = ui.app_theme();
    let signature = state.time_signature();
    // Whole sixteenths.
    let snap = |beats: f64| (beats * 4.).round() / 4.;
    let field = |ui: &mut Ui, label: &str, beats: &mut f64, min: f64| {
        field_label(ui, label);
        ui.add_sized(
            vec2(POSITION_WIDTH, CHOICE_HEIGHT),
            DragValue::new(beats)
                .speed(1. / 16.)
                .range(min..=f64::MAX)
                .custom_formatter(|v, _| format_bars(v, signature))
                .custom_parser(|text| parse_bars(text, signature)),
        )
        .on_hover_text("bar.beat.sixteenth");
    };
    ui.horizontal(|ui| {
        let (mut start, mut end) = (custom.0.0, custom.1.0);
        field(ui, "Start", &mut start, 0.);
        ui.add_space(8.);
        field(ui, "End", &mut end, 0.);
        let (start, end) = (snap(start), snap(end));
        *custom = (BeatPos(start), BeatPos(end));
        let seconds = state.export_seconds(ExportRange::Custom(custom.0, custom.1));
        if let Ok(seconds) = seconds {
            ui.label(
                RichText::new(duration(seconds))
                    .color(theme.text_muted)
                    .size(font_size::LABEL),
            );
        }
    });
}

/// A field's name before it, muted.
fn field_label(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(FIELD_LABEL_WIDTH, CHOICE_HEIGHT), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(font_size::BODY),
        ui.app_theme().text_muted,
    );
}

/// The folder and the name the files are named after, and the files that
/// gives.
fn destination_ui(ui: &mut Ui, dir: &mut String, name: &mut String, files: &[ExportFile]) {
    let theme = ui.app_theme();
    ui.add(SectionHeader::new("Destination"));
    ui.horizontal(|ui| {
        field_label(ui, "Folder");
        let browse_width = 84.;
        let field = ui.available_width() - browse_width - ui.spacing().item_spacing.x;
        ui.add(
            TextEdit::singleline(dir)
                .desired_width(field)
                .hint_text("Choose a folder…"),
        );
        let browse = FlatButton::new(format!("{FOLDER_SIMPLE}  Browse…"))
            .size(vec2(browse_width, CHOICE_HEIGHT))
            .font(FontId::proportional(font_size::BODY))
            .border_radius(3.);
        if ui.add(browse).clicked() {
            let mut dialog = FileDialog::new();
            if Path::new(dir.trim()).is_dir() {
                dialog = dialog.set_directory(dir.trim());
            }
            if let Some(folder) = dialog.pick_folder() {
                *dir = folder.display().to_string();
            }
        }
    });
    ui.horizontal(|ui| {
        field_label(ui, "Name");
        ui.add(
            TextEdit::singleline(name)
                .desired_width(ui.available_width())
                .hint_text("Untitled"),
        );
    });

    // What gets written, and what that replaces.
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        for file in files.iter().take(LISTED_FILES) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                ui.label(
                    RichText::new(WAVEFORM)
                        .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                        .color(theme.text_disabled),
                );
                ui.label(
                    RichText::new(file_name(&file.path))
                        .color(theme.text_muted)
                        .size(font_size::LABEL),
                );
                if file.path.exists() {
                    ui.label(
                        RichText::new("replaces the existing file")
                            .color(theme.warning)
                            .size(font_size::LABEL),
                    );
                }
            });
        }
        if files.len() > LISTED_FILES {
            let replaced = files[LISTED_FILES..]
                .iter()
                .filter(|f| f.path.exists())
                .count();
            let mut more = format!("and {} more", files.len() - LISTED_FILES);
            if replaced > 0 {
                more += &format!(" ({replaced} replacing existing files)");
            }
            ui.label(
                RichText::new(more)
                    .color(theme.text_disabled)
                    .size(font_size::LABEL),
            );
        }
    });
}

fn start(
    state: &ProjectState,
    dir: &Path,
    name: &str,
    sources: &[ExportSource],
    options: ExportOptions,
) -> Step {
    match state.export(dir, name, sources, options) {
        Ok(export) => Step::Running(export),
        Err(detail) => Step::Failed {
            title: "Couldn't export".into(),
            detail,
        },
    }
}

fn running_ui(ui: &Ui, export: &mut Export) -> Option<Step> {
    if let Some(outcome) = export.outcome() {
        return Some(match outcome.clone() {
            ExportOutcome::Done => Step::Done {
                files: export.files().to_vec(),
                waveforms: export.waveforms(),
                seconds: export.seconds(),
            },
            ExportOutcome::Cancelled => Step::Options,
            ExportOutcome::Failed(detail) => Step::Failed {
                title: "Couldn't export".into(),
                detail,
            },
        });
    }
    let theme = ui.app_theme();
    let files = export.files();
    let count = files.len();
    let current = export.current().min(count.saturating_sub(1));
    let progress = export.progress();
    // How far the file being rendered is.
    let file_progress = (progress * count as f32 - current as f32).clamp(0., 1.);
    let waveforms = export.waveforms();
    dialog::modal(ui, "export-progress").show(ui.ctx(), |ui| {
        ui.set_width(NARROW_WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        let subtitle = match files.get(current) {
            Some(file) if count > 1 => {
                format!(
                    "File {} of {count}  ·  {}",
                    current + 1,
                    file_name(&file.path)
                )
            }
            Some(file) => file_name(&file.path),
            None => String::new(),
        };
        header(ui, "Exporting…", &subtitle);
        if let Some(waveform) = waveforms.get(current) {
            mix_waveform(ui, waveform, file_progress, WAVEFORM_HEIGHT);
        }
        progress_bar(ui, progress);
        if count > 1 {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.;
                for (i, file) in files.iter().enumerate() {
                    let (icon, color) = match i.cmp(&current) {
                        std::cmp::Ordering::Less => (CHECK, theme.success),
                        std::cmp::Ordering::Equal => (CIRCLE_NOTCH, theme.accent),
                        std::cmp::Ordering::Greater => (CIRCLE, theme.text_disabled),
                    };
                    file_row(ui, icon, color, &file.label, &file_name(&file.path));
                }
            });
        }
        ui.horizontal(|ui| {
            // Overall, then within the file being rendered.
            let seconds = export.seconds();
            ui.label(
                RichText::new(format!(
                    "{:.0}%  ·  {} of {}",
                    progress * 100.,
                    duration(seconds * file_progress as f64),
                    duration(seconds)
                ))
                .color(theme.text_muted)
                .size(font_size::LABEL),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(button("Cancel")).clicked() {
                    export.cancel();
                }
            });
        });
    });
    // Keeps redrawing while the files render.
    ui.ctx().request_repaint();
    None
}

fn done_ui(ui: &Ui, files: &[ExportFile], waveforms: &[Waveform], seconds: f64) -> Option<Step> {
    let theme = ui.app_theme();
    let mut close = false;
    let modal = dialog::modal(ui, "export-done").show(ui.ctx(), |ui| {
        ui.set_width(NARROW_WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        let folder = files
            .first()
            .and_then(|f| f.path.parent())
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        let title = match files.len() {
            1 => "Exported".to_string(),
            n => format!("Exported {n} files"),
        };
        header(ui, &title, &folder);
        if let ([file], [waveform]) = (files, waveforms) {
            mix_waveform(ui, waveform, 1., WAVEFORM_HEIGHT);
            ui.label(
                RichText::new(format!(
                    "{}  ·  {}  ·  {}",
                    file_name(&file.path),
                    duration(seconds),
                    peak(waveform.peak)
                ))
                .color(theme.text_muted)
                .size(font_size::LABEL),
            );
        } else {
            for (file, waveform) in files.iter().zip(waveforms) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.;
                    ui.vertical(|ui| {
                        ui.set_width(150.);
                        ui.spacing_mut().item_spacing.y = 0.;
                        ui.label(RichText::new(&file.label).size(font_size::BODY));
                        ui.label(
                            RichText::new(peak(waveform.peak))
                                .color(theme.text_muted)
                                .size(font_size::LABEL),
                        )
                        .on_hover_text(file.path.display().to_string());
                    });
                    mix_waveform(ui, waveform, 1., THUMBNAIL_HEIGHT);
                });
            }
            ui.label(
                RichText::new(format!("{} each", duration(seconds)))
                    .color(theme.text_muted)
                    .size(font_size::LABEL),
            );
        }
        ui.add_space(4.);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui.add(button("OK").selected(true)).clicked();
        });
    });
    (close || modal.should_close()).then_some(Step::Closed)
}

fn failed_ui(ui: &Ui, title: &str, detail: &str) -> Option<Step> {
    let theme = ui.app_theme();
    let mut close = false;
    let modal = dialog::modal(ui, "export-failed").show(ui.ctx(), |ui| {
        ui.set_width(NARROW_WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        header(ui, title, "");
        ui.label(RichText::new(detail).color(theme.text_muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui.add(button("OK").selected(true)).clicked();
        });
    });
    (close || modal.should_close()).then_some(Step::Closed)
}

/// A file in a list: an icon in `color`, its label and its file name.
fn file_row(ui: &mut Ui, icon: &str, color: Color32, label: &str, file: &str) {
    let theme = ui.app_theme();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        ui.label(
            RichText::new(icon)
                .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                .color(color),
        );
        ui.label(RichText::new(label).size(font_size::BODY));
        ui.label(
            RichText::new(file)
                .color(theme.text_disabled)
                .size(font_size::LABEL),
        );
    });
}

/// `48 kHz  ·  stereo`: what the engine renders.
fn audio_format(state: &ProjectState) -> String {
    let config = state.engine_config();
    let channels = match config.output_channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} channels"),
    };
    format!("{} kHz  ·  {channels}", trimmed(config.sample_rate / 1000.))
}

fn peak(level: f32) -> String {
    match level {
        0. => "silent".to_string(),
        level => format!("peak {}", decibels(level)),
    }
}

/// The title and, muted under it, a subtitle.
fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.;
        dialog::title(ui, title);
        if !subtitle.is_empty() {
            ui.label(
                RichText::new(subtitle)
                    .color(ui.app_theme().text_muted)
                    .size(font_size::LABEL),
            );
        }
    });
}

/// One line with an icon, in `color`.
fn note(ui: &mut Ui, icon: &str, color: Color32, text: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.;
        ui.label(
            RichText::new(icon)
                .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                .color(color),
        );
        ui.label(RichText::new(text).color(color).size(font_size::LABEL));
    });
}

/// Buttons side by side sharing the width; the one matching `value` is
/// highlighted.
fn choices<T: PartialEq + Copy, const N: usize>(
    ui: &mut Ui,
    value: &mut T,
    options: [(T, String); N],
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.;
        let width = (ui.available_width() - 4. * (N - 1) as f32) / N as f32;
        for (option, label) in options {
            let clicked = ui
                .add(
                    FlatButton::new(label)
                        .size(vec2(width, CHOICE_HEIGHT))
                        .font(FontId::proportional(12.))
                        .border_radius(3.)
                        .selected(*value == option),
                )
                .clicked();
            if clicked {
                *value = option;
            }
        }
    });
}

/// A card to draw into, `height` tall and as wide as the dialog.
fn card(ui: &mut Ui, height: f32, theme: &Theme, sense: Sense) -> (Rect, Painter, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        4.,
        theme.bg_deep,
        Stroke::new(1., theme.border),
        StrokeKind::Inside,
    );
    (rect, painter, response)
}

/// The project in miniature: each track's clips in their colour with their
/// waveform, the tracks and the part of the timeline left out dimmed, and
/// the exported part's bounds marked. Dragging across it picks a custom
/// range, returned while dragging.
fn arrangement_preview(
    ui: &mut Ui,
    state: &ProjectState,
    rows: &[TrackRow],
    selection: &Selection,
    range: ExportRange,
) -> Option<(BeatPos, BeatPos)> {
    let theme = ui.app_theme();
    let (rect, painter, response) = card(ui, PREVIEW_HEIGHT, &theme, Sense::drag());
    let project = state.export_span(ExportRange::Project).ok();
    let looped = state.export_span(ExportRange::Loop).ok();
    let chosen = state.export_span(range).ok();
    let shown = [project, looped, chosen]
        .into_iter()
        .flatten()
        .map(|s| s.1.0)
        .fold(0., f64::max);
    if project.is_none() {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Nothing to export yet: add clips to the timeline",
            FontId::proportional(12.),
            theme.text_muted,
        );
        return None;
    }
    // Lanes on top, the exported part's start and end times below.
    let inner = rect
        .shrink(LANE_PADDING)
        .with_max_y(rect.bottom() - LANE_PADDING - LABEL_STRIP);
    let x = |beat: BeatPos| inner.left() + (beat.0 / shown) as f32 * inner.width();
    let bpm = state.bpm();

    // From where the drag started to the pointer, in whole beats.
    let beat_at = |pos: Pos2| {
        let beats = (pos.x - inner.left()) as f64 / inner.width() as f64 * shown;
        BeatPos(beats.round().clamp(0., shown.ceil()))
    };
    let anchor = response.id.with("anchor");
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        ui.data_mut(|d| d.insert_temp(anchor, beat_at(pos)));
    }
    let dragged = response
        .interact_pointer_pos()
        .filter(|_| response.dragged())
        .zip(ui.data(|d| d.get_temp::<BeatPos>(anchor)))
        .map(|(pos, from)| {
            let to = beat_at(pos);
            (BeatPos(from.0.min(to.0)), BeatPos(from.0.max(to.0)))
        })
        .filter(|(start, end)| end.0 > start.0);
    response
        .on_hover_cursor(CursorIcon::ResizeHorizontal)
        .on_hover_text("Drag across to export part of the project");

    let lanes: Vec<_> = rows
        .iter()
        .filter(|track| track.kind == TrackKind::Audio && !track.clips.is_empty())
        .collect();
    let lane_height = ((inner.height() - LANE_GAP * (lanes.len() as f32 - 1.))
        / lanes.len() as f32)
        .clamp(1., MAX_LANE_HEIGHT);
    for (i, track) in lanes.iter().enumerate() {
        let top = inner.top() + i as f32 * (lane_height + LANE_GAP);
        let color = if !selection.includes(state, track) {
            track.color.gamma_multiply(0.3)
        } else {
            track.color
        };
        let ink = with_alpha(theme.text_on(color), 170);
        for clip in &track.clips {
            let clip_rect =
                Rect::from_x_y_ranges(x(clip.position)..=x(clip.end(bpm)), top..=top + lane_height);
            painter.rect_filled(clip_rect, 1.5, color);
            let seconds = clip.source_seconds() as f64;
            if lane_height >= 6.
                && seconds > 0.
                && let Some(frames) = clip.audio.total_frames()
            {
                // Each pass of the clip's loop shows its part of the whole
                // file laid out.
                let pixels_per_beat = inner.width() / shown as f32;
                let file_width = (seconds * bpm as f64 / 60.) as f32 * pixels_per_beat;
                let clip_x = |beats: f64| clip_rect.left() + beats as f32 * pixels_per_beat;
                for segment in clip.segments(bpm) {
                    let (left, right) = (clip_x(segment.start), clip_x(segment.end));
                    let file_left = left - (segment.file_start / seconds) as f32 * file_width;
                    let file_rect =
                        Rect::from_min_size(pos2(file_left, top), vec2(file_width, lane_height));
                    let segment_rect = Rect::from_x_y_ranges(left..=right, clip_rect.y_range());
                    paint_waveform(
                        &painter,
                        file_rect,
                        segment_rect.intersect(clip_rect),
                        &clip.audio.data,
                        0.0..frames,
                        false,
                        ink,
                        clip.gain,
                        clip.pan,
                    );
                }
            }
        }
    }

    // What's exported stays bright; the rest goes under a veil.
    let marker = theme.accent;
    match state.export_span(range) {
        Ok((start, end)) => {
            let (left, right) = (x(start).max(rect.left()), x(end).min(rect.right()));
            let veil = |from: f32, to: f32| {
                if to > from {
                    painter.rect_filled(
                        Rect::from_x_y_ranges(from..=to, rect.y_range()),
                        0.,
                        with_alpha(theme.bg_deep, 190),
                    );
                }
            };
            veil(rect.left(), left);
            veil(right, rect.right());
            for edge in [left, right] {
                painter.line_segment(
                    [pos2(edge, rect.top()), pos2(edge, rect.bottom())],
                    Stroke::new(1.5, marker),
                );
            }
            let label = |pos: f32, align: Align2, text: String| {
                painter.text(
                    pos2(pos, rect.bottom() - 2.),
                    align,
                    text,
                    FontId::monospace(9.),
                    marker,
                );
            };
            let tempo_seconds = |beat: BeatPos| beat.0 * 60. / bpm as f64;
            label(
                left + 3.,
                Align2::LEFT_BOTTOM,
                duration(tempo_seconds(start)),
            );
            label(
                right - 3.,
                Align2::RIGHT_BOTTOM,
                duration(tempo_seconds(end)),
            );
        }
        Err(_) => {
            painter.rect_filled(rect, 4., with_alpha(theme.bg_deep, 190));
        }
    }
    dragged
}

/// A rendered file, `height` tall: drawn up to `progress`, the rest still
/// to come.
fn mix_waveform(ui: &mut Ui, waveform: &Waveform, progress: f32, height: f32) {
    let theme = ui.app_theme();
    let (rect, painter, _) = card(ui, height, &theme, Sense::hover());
    let inner = rect.shrink2(vec2(LANE_PADDING, (height / 18.).clamp(2., 4.)));
    let center = inner.center().y;
    painter.line_segment(
        [pos2(inner.left(), center), pos2(inner.right(), center)],
        Stroke::new(1., theme.separator),
    );
    // One filled band through the columns, so neighbours join without seams.
    let columns = waveform.columns.len();
    let step = inner.width() / columns as f32;
    let done = ((progress * columns as f32).ceil() as usize).min(columns);
    let y = |s: f32| center - s.clamp(-1., 1.) * inner.height() / 2.;
    let mut mesh = Mesh::default();
    for (i, &(low, high)) in waveform.columns[..done].iter().enumerate() {
        let x = inner.left() + (i as f32 + 0.5) * step;
        let top = y(high);
        // Silence still shows as a hairline.
        let bottom = y(low).max(top + 1.);
        let n = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(x, top), theme.accent);
        mesh.colored_vertex(pos2(x, bottom), theme.accent);
        if i > 0 {
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn progress_bar(ui: &mut Ui, progress: f32) {
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2., theme.bg_control);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * progress, rect.height()));
    painter.rect_filled(filled, 2., theme.accent);
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || project_name(path),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `m:ss`, or `m:ss.s` under ten seconds.
fn duration(seconds: f64) -> String {
    let minutes = (seconds / 60.).floor();
    let rest = seconds - minutes * 60.;
    if seconds < 10. {
        format!("{minutes:.0}:{rest:04.1}")
    } else {
        format!("{minutes:.0}:{:02.0}", rest.floor())
    }
}

/// A level in dB relative to full scale.
fn decibels(level: f32) -> String {
    let db = 20. * level.log10();
    if db > 0. {
        format!("+{db:.1} dBFS")
    } else {
        format!("{db:.1} dBFS").replace('-', "−")
    }
}

/// `48`, `44.1`: without a trailing `.0`.
fn trimmed(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_string)
}

fn sample_bytes(format: WavFormat) -> usize {
    match format {
        WavFormat::Int16 => 2,
        WavFormat::Int24 => 3,
        WavFormat::Float32 => 4,
    }
}

fn file_size(bytes: f64) -> String {
    match bytes {
        b if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        b if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        b => format!("{:.0} kB", (b / 1e3).max(1.)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration(2.5), "0:02.5");
        assert_eq!(duration(75.9), "1:15");
        assert_eq!(duration(600.), "10:00");
    }

    #[test]
    fn levels_sizes_and_rates_read_naturally() {
        assert_eq!(decibels(1.), "0.0 dBFS");
        assert_eq!(decibels(0.5), "−6.0 dBFS");
        assert_eq!(decibels(2.), "+6.0 dBFS");
        assert_eq!(file_size(12_345_678.), "12.3 MB");
        assert_eq!(file_size(10.), "1 kB");
        assert_eq!((trimmed(48.), trimmed(44.1)), ("48".into(), "44.1".into()));
    }
}
