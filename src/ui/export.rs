//! Export audio: the options with a preview of what's exported, the save
//! dialog, the mix drawing itself while it renders, and the result.

use std::path::{Path, PathBuf};

use egui::{
    Align, Align2, Color32, FontFamily, FontId, Id, Layout, Modal, Painter, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use egui_phosphor::regular::{CHECK_CIRCLE, EXPORT, WARNING, WAVEFORM, X_CIRCLE};
use rfd::FileDialog;
use tonique_engine::{sample::WavFormat, time::BeatPos};

use crate::{
    core::{
        export::{Export, ExportOptions, ExportOutcome, ExportRange, Waveform},
        project::project_name,
        state::ProjectState,
        track::TrackKind,
    },
    ui::{
        font::PHOSPHOR_REGULAR,
        project::ProjectManager,
        theme::{Theme, ThemeExt, with_alpha},
        waveform::paint_waveform,
        widget::{dialog::button, flat_button::FlatButton, section::SectionHeader},
    },
};

const WIDTH: f32 = 440.;
const PREVIEW_HEIGHT: f32 = 96.;
const WAVEFORM_HEIGHT: f32 = 72.;
const CHOICE_HEIGHT: f32 = 26.;
/// Space around the preview's lanes and between them.
const LANE_PADDING: f32 = 6.;
const LANE_GAP: f32 = 2.;
const MAX_LANE_HEIGHT: f32 = 16.;
/// Under the lanes, for the times.
const LABEL_STRIP: f32 = 10.;

enum Step {
    Closed,
    /// Choosing what to export; `file_name` and `dir` suggest where.
    Options {
        file_name: String,
        dir: Option<PathBuf>,
    },
    Running(Export),
    Done {
        path: PathBuf,
        seconds: f64,
        waveform: Waveform,
        format: WavFormat,
    },
    Failed {
        title: String,
        detail: String,
    },
}

pub struct ExportWindow {
    step: Step,
    /// Kept between exports.
    options: ExportOptions,
}

impl ExportWindow {
    pub fn new() -> Self {
        Self {
            step: Step::Closed,
            options: ExportOptions::default(),
        }
    }

    /// Ask what to export. Ignored while an export runs.
    pub fn open(&mut self, project: &ProjectManager) {
        if matches!(self.step, Step::Running(_)) {
            return;
        }
        self.step = Step::Options {
            file_name: format!("{}.wav", project.name()),
            dir: project.dir().map(PathBuf::from),
        };
    }

    pub fn show(&mut self, ui: &Ui, state: &ProjectState) {
        let next = match &mut self.step {
            Step::Closed => None,
            Step::Options { file_name, dir } => {
                options_ui(ui, state, &mut self.options, file_name, dir.as_deref())
            }
            Step::Running(export) => running_ui(ui, export, self.options.format),
            Step::Done {
                path,
                seconds,
                waveform,
                format,
            } => done_ui(ui, path, *seconds, waveform, *format),
            Step::Failed { title, detail } => failed_ui(ui, title, detail),
        };
        if let Some(next) = next {
            self.step = next;
        }
    }
}

fn options_ui(
    ui: &Ui,
    state: &ProjectState,
    options: &mut ExportOptions,
    file_name: &str,
    dir: Option<&Path>,
) -> Option<Step> {
    let theme = ui.app_theme();
    let mut next = None;
    let modal = modal("export-options").show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        header(ui, EXPORT, theme.accent, "Export audio", file_name);
        arrangement_preview(ui, state, options.range);

        ui.add(SectionHeader::new("Range"));
        let lengths = [ExportRange::Project, ExportRange::Loop]
            .map(|range| state.export_seconds(range).map(duration));
        choices(
            ui,
            &mut options.range,
            [
                (ExportRange::Project, "Whole project", &lengths[0]),
                (ExportRange::Loop, "Loop region", &lengths[1]),
            ]
            .map(|(range, label, length)| {
                let length = length.as_deref().unwrap_or("empty");
                (range, format!("{label}  ·  {length}"))
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
        let rate = state.engine_config().sample_rate;
        let channels = state.engine_config().output_channels;
        let mut details = format!(
            "WAV  ·  {} kHz  ·  {}",
            trimmed(rate / 1000.),
            if channels == 2 {
                "stereo".into()
            } else {
                format!("{channels} channels")
            }
        );
        if let Ok(seconds) = state.export_seconds(options.range) {
            let bytes = seconds * rate * (channels * sample_bytes(options.format)) as f64;
            details += &format!("  ·  ≈ {}", file_size(bytes));
        }
        ui.label(RichText::new(details).color(theme.text_muted).small());

        let problem = state.export_span(options.range).err();
        if let Some(problem) = &problem {
            note(ui, WARNING, theme.danger, problem);
        }

        ui.add_space(4.);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let export = ui.add_enabled(problem.is_none(), button("Export…").selected(true));
            if export.clicked() {
                next = Some(start(state, *options, file_name, dir));
            }
            if ui.add(button("Cancel")).clicked() {
                next = Some(Step::Closed);
            }
        });
    });
    if next.is_none() && modal.should_close() {
        next = Some(Step::Closed);
    }
    next
}

/// Ask where to save, then start. Back to the options if no file is chosen.
fn start(
    state: &ProjectState,
    options: ExportOptions,
    file_name: &str,
    dir: Option<&Path>,
) -> Step {
    let mut dialog = FileDialog::new()
        .add_filter("WAV audio", &["wav"])
        .set_file_name(file_name);
    if let Some(dir) = dir {
        dialog = dialog.set_directory(dir);
    }
    let Some(mut path) = dialog.save_file() else {
        return Step::Options {
            file_name: file_name.to_string(),
            dir: dir.map(PathBuf::from),
        };
    };
    if path.extension().is_none_or(|ext| ext != "wav") {
        path.set_extension("wav");
    }
    match state.export(path, options) {
        Ok(export) => Step::Running(export),
        Err(detail) => Step::Failed {
            title: "Couldn't export".into(),
            detail,
        },
    }
}

fn running_ui(ui: &Ui, export: &mut Export, format: WavFormat) -> Option<Step> {
    let name = file_name(export.path());
    if let Some(outcome) = export.outcome() {
        return Some(match outcome.clone() {
            ExportOutcome::Done => Step::Done {
                path: export.path().to_path_buf(),
                seconds: export.seconds(),
                waveform: export.waveform(),
                format,
            },
            ExportOutcome::Cancelled => Step::Closed,
            ExportOutcome::Failed(detail) => Step::Failed {
                title: format!("Couldn't export “{name}”"),
                detail,
            },
        });
    }
    let theme = ui.app_theme();
    let progress = export.progress();
    modal("export-progress").show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        header(ui, WAVEFORM, theme.accent, "Exporting…", &name);
        mix_waveform(ui, &export.waveform(), progress);
        progress_bar(ui, progress);
        ui.horizontal(|ui| {
            let seconds = export.seconds();
            ui.label(
                RichText::new(format!(
                    "{:.0}%  ·  {} of {}",
                    progress * 100.,
                    duration(seconds * progress as f64),
                    duration(seconds)
                ))
                .color(theme.text_muted)
                .small(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(button("Cancel")).clicked() {
                    export.cancel();
                }
            });
        });
    });
    None
}

fn done_ui(
    ui: &Ui,
    path: &Path,
    seconds: f64,
    waveform: &Waveform,
    format: WavFormat,
) -> Option<Step> {
    let theme = ui.app_theme();
    let mut close = false;
    let modal = modal("export-done").show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        header(
            ui,
            CHECK_CIRCLE,
            theme.success,
            "Exported",
            &file_name(path),
        );
        mix_waveform(ui, waveform, 1.);
        let peak = match waveform.peak {
            0. => "silent".to_string(),
            peak => format!("peak {}", decibels(peak)),
        };
        ui.label(
            RichText::new(format!("{}  ·  {peak}", duration(seconds)))
                .color(theme.text_muted)
                .small(),
        );
        // Floats keep what's above full scale; integers cut it off.
        if waveform.peak > 1. && format != WavFormat::Float32 {
            note(
                ui,
                WARNING,
                theme.warning,
                &format!(
                    "The mix clipped (peak {}). Lower the master volume and export again.",
                    decibels(waveform.peak)
                ),
            );
        }
        ui.label(
            RichText::new(path.display().to_string())
                .color(theme.text_muted)
                .small(),
        );
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
    let modal = modal("export-failed").show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing = vec2(6., 8.);
        header(ui, X_CIRCLE, theme.danger, title, "");
        ui.label(RichText::new(detail).color(theme.text_muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = ui.add(button("OK").selected(true)).clicked();
        });
    });
    (close || modal.should_close()).then_some(Step::Closed)
}

fn modal(id: &str) -> Modal {
    Modal::new(Id::new(id))
}

/// An icon in a tinted badge, a title and a muted subtitle.
fn header(ui: &mut Ui, icon: &str, color: Color32, title: &str, subtitle: &str) {
    ui.horizontal(|ui| {
        let (badge, _) = ui.allocate_exact_size(vec2(34., 34.), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(badge, 6., with_alpha(color, 40));
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            icon,
            FontId::new(19., FontFamily::Name(PHOSPHOR_REGULAR.into())),
            color,
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            // A lone title sits level with the badge.
            ui.add_space(if subtitle.is_empty() { 8. } else { 1. });
            ui.label(RichText::new(title).strong().size(14.));
            if !subtitle.is_empty() {
                ui.label(
                    RichText::new(subtitle)
                        .color(ui.app_theme().text_muted)
                        .small(),
                );
            }
        });
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
        ui.label(RichText::new(text).color(color).small());
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
fn card(ui: &mut Ui, height: f32, theme: &Theme) -> (Rect, Painter) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        4.,
        theme.bg_deep,
        Stroke::new(1., theme.border),
        StrokeKind::Inside,
    );
    (rect, painter)
}

/// The project in miniature: each track's clips in their colour with their
/// waveform, what's left out dimmed and the exported part's bounds marked.
fn arrangement_preview(ui: &mut Ui, state: &ProjectState, range: ExportRange) {
    let theme = ui.app_theme();
    let (rect, painter) = card(ui, PREVIEW_HEIGHT, &theme);
    let project = state.export_span(ExportRange::Project).ok();
    let looped = state.export_span(ExportRange::Loop).ok();
    let shown = [project, looped]
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
        return;
    }
    // Lanes on top, the exported part's start and end times below.
    let inner = rect
        .shrink(LANE_PADDING)
        .with_max_y(rect.bottom() - LANE_PADDING - LABEL_STRIP);
    let x = |beat: BeatPos| inner.left() + (beat.0 / shown) as f32 * inner.width();
    let bpm = state.bpm();

    let lanes: Vec<_> = state
        .tracks()
        .filter(|track| track.kind == TrackKind::Audio && !track.clips.is_empty())
        .collect();
    let lane_height = ((inner.height() - LANE_GAP * (lanes.len() as f32 - 1.))
        / lanes.len() as f32)
        .clamp(1., MAX_LANE_HEIGHT);
    for (i, track) in lanes.iter().enumerate() {
        let top = inner.top() + i as f32 * (lane_height + LANE_GAP);
        let color = if track.is_silenced() {
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
}

/// The rendered mix: bright up to `progress`, the rest still to come.
fn mix_waveform(ui: &mut Ui, waveform: &Waveform, progress: f32) {
    let theme = ui.app_theme();
    let (rect, painter) = card(ui, WAVEFORM_HEIGHT, &theme);
    let inner = rect.shrink2(vec2(LANE_PADDING, 4.));
    let center = inner.center().y;
    painter.line_segment(
        [pos2(inner.left(), center), pos2(inner.right(), center)],
        Stroke::new(1., theme.separator),
    );
    let columns = waveform.columns.len();
    let step = inner.width() / columns as f32;
    let done = (progress * columns as f32).ceil() as usize;
    let shapes = waveform.columns[..done.min(columns)]
        .iter()
        .enumerate()
        .map(|(i, &(low, high))| {
            let x = inner.left() + (i as f32 + 0.5) * step;
            let y = |s: f32| center - s.clamp(-1., 1.) * inner.height() / 2.;
            // Clipped columns stand out.
            let color = if high > 1. || low < -1. {
                theme.meter_high
            } else {
                theme.accent
            };
            Shape::line_segment(
                [pos2(x, y(high)), pos2(x, y(low) + 0.5)],
                Stroke::new(step.max(1.), color),
            )
        });
    painter.extend(shapes);
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
