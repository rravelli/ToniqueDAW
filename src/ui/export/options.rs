//! What to export, the range and format, and where the files go.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use egui::{
    Align, Align2, Color32, CursorIcon, DragValue, FontFamily, FontId, Layout, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use egui_phosphor::regular::{CHECK_SQUARE, FOLDER_SIMPLE, SQUARE, WAVEFORM};
use rfd::FileDialog;
use tonique_engine::{sample::WavFormat, time::BeatPos};

use crate::{
    core::{
        export::{ExportFile, ExportOptions, ExportRange},
        state::ProjectState,
        track::{TrackKind, TrackRow},
    },
    ui::{
        font::{PHOSPHOR_FILL, PHOSPHOR_REGULAR},
        theme::{ThemeExt, font_size},
        widget::{flat_button::FlatButton, section::SectionHeader},
    },
    utils::{display_name, format_bars, parse_bars},
};

use super::{CHOICE_HEIGHT, Selection, format::*, widgets::choices};

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
/// The furthest a custom range can end, in bars.
const MAX_BARS: f64 = 9999.;
/// The effect tails offered, in seconds.
const TAILS: [u32; 4] = [0, 2, 5, 10];

/// The mix, then every track and group, indented under their group.
pub(super) fn sources_ui(ui: &mut Ui, rows: &[TrackRow], selection: &mut Selection) {
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
pub(super) fn source_row(
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
pub(super) enum RangeChoice {
    Project,
    Loop,
    Custom,
}

/// Range, tail and format, with what the files will be like.
pub(super) fn settings_ui(
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
pub(super) fn custom_range_ui(ui: &mut Ui, state: &ProjectState, custom: &mut (BeatPos, BeatPos)) {
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
                .range(min..=MAX_BARS * signature.beats_per_bar())
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
pub(super) fn field_label(ui: &mut Ui, text: &str) {
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
pub(super) fn destination_ui(
    ui: &mut Ui,
    dir: &mut String,
    name: &mut String,
    files: &[ExportFile],
    existing: &HashSet<PathBuf>,
) {
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
                if existing.contains(&file.path) {
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
                .filter(|f| existing.contains(&f.path))
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
