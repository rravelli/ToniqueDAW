//! Each file drawing itself while it renders, and the result.

use std::cmp::Ordering;

use gpui::{Context, MouseButton, PathBuilder, canvas, div, point, prelude::*, px};

use crate::{
    app::export::ExportDialog,
    geometry::Rect,
    icons,
    theme::{Color, Theme, hsla, size},
    widgets::{dialog_button, dialog_frame, glyph},
};
use tonique_core::files::export::{
    Export, ExportFile, Waveform,
    format::{duration, file_name, peak},
};

use super::{
    LANE_PADDING,
    parts::{card, header, small},
};

/// Progress and results.
const NARROW_WIDTH: f32 = 480.;
const GAP: f32 = 8.;
const WAVEFORM_HEIGHT: f32 = 72.;
/// A file's waveform in a list of several.
const THUMBNAIL_HEIGHT: f32 = 26.;
const FILE_LABEL_WIDTH: f32 = 150.;
const PROGRESS_HEIGHT: f32 = 4.;

impl ExportDialog {
    pub(super) fn running_view(&self, running: &Export, context: &mut Context<Self>) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let files = running.files();
        let count = files.len();
        let current = running.current().min(count.saturating_sub(1));
        let progress = running.progress();
        // How far the file being rendered is.
        let file_progress = (progress * count as f32 - current as f32).clamp(0., 1.);
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
        let list = (count > 1).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(files.iter().enumerate().map(|(index, file)| {
                    let (icon, color) = match index.cmp(&current) {
                        Ordering::Less => (icons::CHECK, theme.success),
                        Ordering::Equal => (icons::CIRCLE_NOTCH, theme.accent),
                        Ordering::Greater => (icons::CIRCLE, theme.text_disabled),
                    };
                    file_row(icon, color, &file.label, &file_name(&file.path), theme)
                }))
        });
        // Overall, then within the file being rendered.
        let seconds = running.seconds();
        let status = format!(
            "{:.0}%  ·  {} of {}",
            progress * 100.,
            duration(seconds * file_progress as f64),
            duration(seconds)
        );
        let cancel = dialog_button("export-cancel-running", "Cancel", false, theme).on_mouse_down(
            MouseButton::Left,
            context.listener(|this, _, _, context| {
                context.stop_propagation();
                if let super::Step::Running(running) = &this.step {
                    running.cancel();
                }
                context.notify();
            }),
        );
        dialog_frame(NARROW_WIDTH, theme)
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header("Exporting…", subtitle, theme))
            .children(
                running
                    .waveform(current)
                    .map(|waveform| mix_waveform(waveform, file_progress, WAVEFORM_HEIGHT, theme)),
            )
            .child(progress_bar(progress, theme))
            .children(list)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1().child(small(status, theme.text_muted)))
                    .child(cancel),
            )
    }

    pub(super) fn done_view(
        &self,
        files: &[ExportFile],
        waveforms: &[Waveform],
        seconds: f64,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let folder = files
            .first()
            .and_then(|file| file.path.parent())
            .map(|dir| dir.display().to_string())
            .unwrap_or_default();
        let title = match files.len() {
            1 => "Exported".to_string(),
            count => format!("Exported {count} files"),
        };
        let mut dialog = dialog_frame(NARROW_WIDTH, theme)
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(title, folder, theme));
        if let ([file], [waveform]) = (files, waveforms) {
            let about = format!(
                "{}  ·  {}  ·  {}",
                file_name(&file.path),
                duration(seconds),
                peak(waveform.peak)
            );
            dialog = dialog
                .child(mix_waveform(waveform.clone(), 1., WAVEFORM_HEIGHT, theme))
                .child(small(about, theme.text_muted));
        } else {
            for (file, waveform) in files.iter().zip(waveforms) {
                dialog = dialog.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(GAP))
                        .child(
                            div()
                                .flex_none()
                                .w(px(FILE_LABEL_WIDTH))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(file.label.clone())
                                .child(small(peak(waveform.peak), theme.text_muted)),
                        )
                        .child(div().flex_1().min_w_0().child(mix_waveform(
                            waveform.clone(),
                            1.,
                            THUMBNAIL_HEIGHT,
                            theme,
                        ))),
                );
            }
            dialog = dialog.child(small(
                format!("{} each", duration(seconds)),
                theme.text_muted,
            ));
        }
        dialog.child(self.ok_button(theme, context))
    }

    pub(super) fn failed_view(
        &self,
        title: &str,
        detail: &str,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        dialog_frame(NARROW_WIDTH, theme)
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(header(title.to_string(), String::new(), theme))
            .child(
                div()
                    .text_color(hsla(theme.text_muted))
                    .child(detail.to_string()),
            )
            .child(self.ok_button(theme, context))
    }

    /// The button closing the result, on the right.
    fn ok_button(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let ok = dialog_button("export-ok", "OK", true, theme).on_mouse_down(
            MouseButton::Left,
            context.listener(|this, _, window, context| {
                context.stop_propagation();
                this.close(window, context);
                context.notify();
            }),
        );
        div().flex().justify_end().mt(px(4.)).child(ok)
    }
}

/// A file in a list: an icon in `color`, its label and its file name.
fn file_row(icon: &'static str, color: Color, label: &str, file: &str, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(glyph(icon, size::BODY, false).text_color(hsla(color)))
        .child(label.to_string())
        .child(small(file.to_string(), theme.text_disabled))
}

/// A rendered file, `height` tall: drawn up to `progress`, the rest still
/// to come.
fn mix_waveform(waveform: Waveform, progress: f32, height: f32, theme: &Theme) -> gpui::Div {
    let (line, fill) = (hsla(theme.separator), hsla(theme.accent));
    let painting = canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let rect = Rect::of(bounds);
            let margin = (height / 18.).clamp(2., 4.);
            let inner = Rect::new(
                rect.left + LANE_PADDING,
                rect.top + margin,
                rect.width - 2. * LANE_PADDING,
                rect.height - 2. * margin,
            );
            let center = inner.top + inner.height / 2.;
            let axis = Rect::new(inner.left, center - 0.5, inner.width, 1.);
            window.paint_quad(gpui::fill(axis.bounds(), line));
            // One filled band through the columns, so neighbours join
            // without seams.
            let columns = waveform.columns.len();
            let step = inner.width / columns as f32;
            let done = ((progress * columns as f32).ceil() as usize).min(columns);
            if done < 2 {
                return;
            }
            let y = |sample: f32| center - sample.clamp(-1., 1.) * inner.height / 2.;
            let x = |index: usize| inner.left + (index as f32 + 0.5) * step;
            let drawn = &waveform.columns[..done];
            let mut band = PathBuilder::fill();
            band.move_to(point(px(x(0)), px(y(drawn[0].1))));
            for (index, &(_, high)) in drawn.iter().enumerate().skip(1) {
                band.line_to(point(px(x(index)), px(y(high))));
            }
            for (index, &(low, high)) in drawn.iter().enumerate().rev() {
                // Silence still shows as a hairline.
                band.line_to(point(px(x(index)), px(y(low).max(y(high) + 1.))));
            }
            band.close();
            if let Ok(band) = band.build() {
                window.paint_path(band, fill);
            }
        },
    )
    .size_full();
    card(height, theme).child(painting)
}

fn progress_bar(progress: f32, theme: &Theme) -> gpui::Div {
    div()
        .w_full()
        .h(px(PROGRESS_HEIGHT))
        .rounded(px(2.))
        .bg(hsla(theme.bg_control))
        .child(
            div()
                .h_full()
                .w(gpui::relative(progress.clamp(0., 1.)))
                .rounded(px(2.))
                .bg(hsla(theme.accent)),
        )
}
