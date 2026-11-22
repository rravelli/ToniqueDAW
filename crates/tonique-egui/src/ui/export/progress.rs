//! Each file drawing itself while it renders, and the result.

use egui::{
    Align, Color32, FontFamily, Layout, Mesh, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2,
};
use egui_phosphor::regular::{CHECK, CIRCLE, CIRCLE_NOTCH};

use crate::{
    core::export::{Export, ExportFile, ExportOutcome, Waveform},
    ui::{
        font::PHOSPHOR_REGULAR,
        theme::{ThemeExt, font_size},
        widget::dialog::{self, button},
    },
};

use super::{
    LANE_PADDING, Step,
    format::*,
    widgets::{card, header},
};

/// Progress and results.
const NARROW_WIDTH: f32 = 480.;
const WAVEFORM_HEIGHT: f32 = 72.;
/// A file's waveform in a list of several.
const THUMBNAIL_HEIGHT: f32 = 26.;

pub(super) fn running_ui(ui: &Ui, export: &mut Export) -> Option<Step> {
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
    let waveform = export.waveform(current);
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
        if let Some(waveform) = &waveform {
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

pub(super) fn done_ui(
    ui: &Ui,
    files: &[ExportFile],
    waveforms: &[Waveform],
    seconds: f64,
) -> Option<Step> {
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

pub(super) fn failed_ui(ui: &Ui, title: &str, detail: &str) -> Option<Step> {
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
pub(super) fn file_row(ui: &mut Ui, icon: &str, color: Color32, label: &str, file: &str) {
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

/// A rendered file, `height` tall: drawn up to `progress`, the rest still
/// to come.
pub(super) fn mix_waveform(ui: &mut Ui, waveform: &Waveform, progress: f32, height: f32) {
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

pub(super) fn progress_bar(ui: &mut Ui, progress: f32) {
    let theme = ui.app_theme();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2., theme.bg_control);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * progress, rect.height()));
    painter.rect_filled(filled, 2., theme.accent);
}
