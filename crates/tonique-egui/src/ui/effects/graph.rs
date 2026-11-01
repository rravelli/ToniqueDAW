//! Drawing shared by editors with a frequency axis: the axis, its grid,
//! and spectrum curves.

use crate::ui::theme::font_size;
use crate::ui::{effects::format_hz, theme::Theme, widget::flat_button::FlatButton};
use egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, Ui, UiBuilder, pos2, vec2,
};
use std::ops::RangeInclusive;
use tonique_engine::{meter::PluginTap, spectrum::Spectrum};

pub const MIN_HZ: f32 = 20.;
pub const MAX_HZ: f32 = 20_000.;

/// Where `hz` is across `rect`, on a log scale from [`MIN_HZ`] to
/// [`MAX_HZ`].
pub fn freq_x(rect: Rect, hz: f32) -> f32 {
    rect.left() + (hz / MIN_HZ).ln() / (MAX_HZ / MIN_HZ).ln() * rect.width()
}

/// The frequency at `x` across `rect`.
pub fn x_freq(rect: Rect, x: f32) -> f32 {
    MIN_HZ * (MAX_HZ / MIN_HZ).powf((x - rect.left()) / rect.width())
}

/// Maps frequencies and gains to a graph and back: frequencies across on
/// a log scale, gains in dB up it.
pub struct FreqScale {
    pub rect: Rect,
    pub top_db: f32,
    pub bottom_db: f32,
}

impl FreqScale {
    pub fn x(&self, hz: f32) -> f32 {
        freq_x(self.rect, hz)
    }
    pub fn hz(&self, x: f32) -> f32 {
        x_freq(self.rect, x)
    }
    pub fn y(&self, db: f32) -> f32 {
        self.rect.top() + (self.top_db - db) / (self.top_db - self.bottom_db) * self.rect.height()
    }
    pub fn db(&self, y: f32) -> f32 {
        self.top_db - (y - self.rect.top()) / self.rect.height() * (self.top_db - self.bottom_db)
    }

    /// The frequency grid, and lines every `step` dB, stronger at 0 dB.
    pub fn paint_grid(&self, painter: &Painter, step: f32, theme: &Theme) {
        paint_freq_grid(painter, self.rect, theme);
        let mut db = (self.bottom_db / step).ceil() * step;
        while db < self.top_db {
            let color = if db == 0. {
                theme.grid_bar
            } else {
                theme.grid_beat
            };
            painter.hline(self.rect.x_range(), self.y(db), Stroke::new(1., color));
            db += step;
        }
    }
}

/// Levels of a spectrum shown behind a response, in dBFS.
const BEHIND_RANGE: RangeInclusive<f32> = -90.0..=0.;

/// Which side of an effect a spectrum shows.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Input,
    Output,
}

/// The live spectrum of what goes into an effect, or comes out of it,
/// faint behind its response, with buttons to pick the side.
pub struct LiveSpectrum {
    spectrum: Spectrum,
    side: Side,
}

impl LiveSpectrum {
    pub fn new() -> Self {
        Self {
            spectrum: Spectrum::new(),
            side: Side::Input,
        }
    }

    pub fn paint(
        &mut self,
        ui: &Ui,
        painter: &Painter,
        rect: Rect,
        tap: &PluginTap,
        sample_rate: f32,
        theme: &Theme,
    ) {
        // Keep it recording for half a second past the last frame shown.
        let frames = (sample_rate / 2.) as u32;
        let meter = match self.side {
            Side::Input => {
                tap.watch_input_for(frames);
                &tap.input
            }
            Side::Output => {
                tap.watch_output_for(frames);
                &tap.output
            }
        };
        self.spectrum
            .update(meter, sample_rate, ui.input(|i| i.stable_dt));
        let points = spectrum_points(&self.spectrum, rect, &BEHIND_RANGE);
        paint_curve(
            painter,
            points,
            rect.bottom(),
            Stroke::new(1., theme.text_muted),
            theme.text_disabled.gamma_multiply(0.3),
        );
    }

    /// Buttons picking the side, at the top left of `rect`. `effect` names
    /// it in their tooltips.
    pub fn buttons(&mut self, ui: &mut Ui, rect: Rect, effect: &str) {
        // Over the graph, out of the layout: `put` would move the controls
        // below them.
        let ui = &mut ui.new_child(UiBuilder::new().max_rect(rect));
        let mut at = rect.left_top() + vec2(3., 3.);
        for (side, name, tooltip) in [
            (
                Side::Input,
                "Pre",
                format!("Show the audio going into the {effect}"),
            ),
            (
                Side::Output,
                "Post",
                format!("Show the audio coming out of the {effect}"),
            ),
        ] {
            let button = FlatButton::new(name)
                .size(vec2(26., 14.))
                .font(FontId::proportional(font_size::SMALL))
                .selected(self.side == side)
                .tooltip(tooltip);
            if ui
                .put(Rect::from_min_size(at, vec2(26., 14.)), button)
                .clicked()
            {
                self.side = side;
            }
            at.x += 28.;
        }
    }
}

/// Where `db` is down `rect`, `range` being the levels shown (quietest
/// first), clamped to it.
pub fn level_y(rect: Rect, range: &RangeInclusive<f32>, db: f32) -> f32 {
    let (bottom, top) = (*range.start(), *range.end());
    rect.bottom() - ((db - bottom) / (top - bottom)).clamp(0., 1.) * rect.height()
}

/// Lines at 1-2-5 steps, labelled at the decades.
pub fn paint_freq_grid(painter: &Painter, rect: Rect, theme: &Theme) {
    let mut decade = 10.;
    while decade < MAX_HZ {
        for step in [1., 2., 5.] {
            let hz = decade * step;
            if !(MIN_HZ..=MAX_HZ).contains(&hz) {
                continue;
            }
            let x = freq_x(rect, hz);
            let color = if step == 1. {
                theme.grid_bar
            } else {
                theme.grid_beat
            };
            painter.vline(x, rect.y_range(), Stroke::new(1., color));
            if step == 1. {
                painter.text(
                    pos2(x + 2., rect.bottom() - 2.),
                    Align2::LEFT_BOTTOM,
                    format_hz(hz).replace(' ', ""),
                    FontId::proportional(font_size::SMALL),
                    theme.text_muted,
                );
            }
        }
        decade *= 10.;
    }
}

/// `spectrum` as a point per pixel across `rect`, `range` being the
/// levels shown in dBFS.
pub fn spectrum_points(spectrum: &Spectrum, rect: Rect, range: &RangeInclusive<f32>) -> Vec<Pos2> {
    (0..=rect.width().ceil() as usize)
        .map(|i| {
            let x = rect.left() + i as f32;
            let db = spectrum.level(x_freq(rect, x));
            pos2(x, level_y(rect, range, db))
        })
        .collect()
}

/// A curve, filled below with a faint `fill`.
pub fn paint_curve(painter: &Painter, points: Vec<Pos2>, bottom: f32, line: Stroke, fill: Color32) {
    painter.add(fill_below(&points, bottom, fill));
    painter.add(Shape::line(points, line));
}

/// Fill between a curve going left to right and `bottom`. Unlike a
/// polygon fill, it can be any shape.
pub fn fill_below(points: &[Pos2], bottom: f32, color: Color32) -> Shape {
    let mut mesh = Mesh::default();
    for (i, p) in points.iter().enumerate() {
        mesh.colored_vertex(*p, color);
        mesh.colored_vertex(pos2(p.x, bottom.max(p.y)), color);
        if i > 0 {
            let n = 2 * i as u32;
            mesh.add_triangle(n - 2, n - 1, n);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    Shape::mesh(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn axes_map_back() {
        let rect = Rect::from_min_size(pos2(10., 20.), vec2(300., 100.));
        assert!((x_freq(rect, freq_x(rect, 1234.)) - 1234.).abs() < 0.1);
        assert_eq!(freq_x(rect, MIN_HZ), 10.);
        assert_eq!(level_y(rect, &(-90.0..=0.), 0.), 20.);
        assert_eq!(level_y(rect, &(-90.0..=0.), -200.), 120.);
        let scale = FreqScale {
            rect,
            top_db: 24.,
            bottom_db: -36.,
        };
        assert!((scale.hz(scale.x(1234.)) - 1234.).abs() < 0.1);
        assert!((scale.db(scale.y(-7.)) + 7.).abs() < 1e-3);
        assert_eq!(scale.y(24.), 20.);
    }
}
