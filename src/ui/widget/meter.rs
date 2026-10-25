//! A stereo level meter: a bar per channel on a scale finer near the top,
//! up to +6 dB, with a mark at full scale: green, turning yellow just
//! under full scale, red over it. The peak shows lighter
//! behind the RMS level, the highest recent peak as a line, and clips as a
//! lamp above each bar until the meter is clicked.

use crate::{
    core::metrics::AudioMetrics,
    ui::theme::{Theme, ThemeExt, font_size},
};
use egui::{Color32, Mesh, Rect, Response, RichText, Sense, Vec2, Widget};

/// Between the two channels' bars.
const CHANNEL_GAP: f32 = 1.;
/// The clip lamps above the bars, and the gap below them; only on meters
/// at least [`LAMP_MIN_HEIGHT`] tall, shorter ones light the bar's top.
const LAMP_HEIGHT: f32 = 3.;
const LAMP_GAP: f32 = 1.;
const LAMP_MIN_HEIGHT: f32 = 30.;
/// The mark at full scale (0 dBFS), on meters tall enough for it to read.
const MARK_MIN_HEIGHT: f32 = 20.;
/// Where the green starts turning yellow, in dBFS; it's yellow at full
/// scale, and red above.
const TURN_DB: f32 = -18.;
/// How the green turns: slowly at first, so most of the meter is green
/// and only the last few dB are yellow.
const TURN_CURVE: f32 = 2.;
/// Levels where the colour is computed, in dBFS, the bar's colour blended
/// between them.
const STOPS: [f32; 9] = [-18., -12., -9., -6., -4.5, -3., -1.5, -0.5, 0.];
/// The top of the scale, in dBFS: room above full scale to see overs.
const TOP_DB: f32 = 6.;
/// How much of its colour the peak, behind the RMS level, has over the
/// well.
const PEAK_SHADE: f32 = 0.5;

pub struct LevelMeter<'a> {
    size: Vec2,
    disabled: bool,
    metrics: &'a AudioMetrics,
}

impl<'a> LevelMeter<'a> {
    pub fn new(size: Vec2, metrics: &'a AudioMetrics) -> Self {
        Self {
            size,
            disabled: false,
            metrics,
        }
    }

    /// Greyed, for a channel that's silenced or bypassed.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Widget for LevelMeter<'_> {
    /// Clicked: the caller resets the clips ([`AudioMetrics::reset_clip`]).
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let (response, painter) = ui.allocate_painter(self.size, Sense::click());
        let rect = response.rect;
        let theme = ui.app_theme();
        let m = self.metrics;
        let lamps = rect.height() >= LAMP_MIN_HEIGHT;
        let bars = if lamps {
            rect.with_min_y(rect.top() + LAMP_HEIGHT + LAMP_GAP)
        } else {
            rect
        };
        let width = (rect.width() - CHANNEL_GAP) / 2.;
        let zones = Zones::new(&theme, self.disabled);
        let y = |db: f32| bars.bottom() - scale(db) * bars.height();
        let (peaks, holds, rms, clipped) = (m.peak_db(), m.hold_db(), m.rms_db(), m.clipped());
        for ch in 0..2 {
            let left = rect.left() + ch as f32 * (width + CHANNEL_GAP);
            let column = Rect::from_x_y_ranges(left..=left + width, bars.y_range());
            painter.rect_filled(column, 0., theme.bg_deep);
            if bars.height() >= MARK_MIN_HEIGHT {
                painter.hline(column.x_range(), y(0.).round() + 0.5, (1., zones.tick));
            }
            // The peak, lighter, behind the RMS level.
            for (top, shade) in [(peaks[ch], PEAK_SHADE), (rms[ch], 1.)] {
                let shade = |c: Color32| theme.bg_deep.lerp_to_gamma(c, shade);
                let mut mesh = Mesh::default();
                for (from, to) in zones.spans() {
                    let to = to.min(top);
                    if to <= from {
                        continue;
                    }
                    let segment = Rect::from_x_y_ranges(column.x_range(), y(to)..=y(from));
                    let (bottom, top) = (shade(zones.color(from)), shade(zones.color(to)));
                    // Red over full scale is flat; the rest a gradient.
                    let top = if from >= 0. { bottom } else { top };
                    gradient(&mut mesh, segment, bottom, top);
                }
                painter.add(mesh);
            }
            if scale(holds[ch]) > 0. {
                let at = y(holds[ch]).max(column.top() + 1.);
                painter.hline(column.x_range(), at - 0.5, (1., zones.color(holds[ch])));
            }
            if clipped[ch] {
                let lamp = if lamps {
                    Rect::from_x_y_ranges(column.x_range(), rect.top()..=rect.top() + LAMP_HEIGHT)
                } else {
                    Rect::from_x_y_ranges(column.x_range(), column.top()..=column.top() + 2.)
                };
                painter.rect_filled(lamp, 0., zones.high);
            } else if lamps {
                let lamp =
                    Rect::from_x_y_ranges(column.x_range(), rect.top()..=rect.top() + LAMP_HEIGHT);
                painter.rect_filled(lamp, 0., theme.bg_deep);
            }
        }
        response.on_hover_ui(|ui| {
            let line = |label: &str, [l, r]: [f32; 2]| {
                format!("{label}  {}  {}", format_level(l), format_level(r))
            };
            let mut text = format!("{}\n{}", line("Peak", holds), line("RMS ", rms));
            if clipped.contains(&true) {
                text.push_str("\nClipped: click to reset");
            }
            ui.label(RichText::new(text).monospace().size(font_size::SMALL));
        })
    }
}

/// The meter's colours, by level.
struct Zones {
    low: Color32,
    mid: Color32,
    high: Color32,
    tick: Color32,
}

impl Zones {
    fn new(theme: &Theme, disabled: bool) -> Self {
        let tick = theme.bg_deep.lerp_to_gamma(theme.text_disabled, 0.35);
        if disabled {
            let grey = theme.bg_control_hover;
            return Self {
                low: grey,
                mid: grey,
                high: grey,
                tick,
            };
        }
        Self {
            low: theme.meter_low,
            mid: theme.meter_mid,
            high: theme.meter_high,
            tick,
        }
    }

    /// Ranges of levels, from the bottom up, each a solid colour or a
    /// gradient between its ends' colours.
    fn spans(&self) -> impl Iterator<Item = (f32, f32)> {
        let bounds = std::iter::once(f32::NEG_INFINITY)
            .chain(STOPS)
            .chain([f32::INFINITY]);
        bounds.clone().zip(bounds.skip(1))
    }

    /// Green, turning yellow towards full scale; red above it.
    fn color(&self, db: f32) -> Color32 {
        if db > 0. {
            self.high
        } else if db <= TURN_DB {
            self.low
        } else {
            let t = ((db - TURN_DB) / -TURN_DB).powf(TURN_CURVE);
            self.low.lerp_to_gamma(self.mid, t)
        }
    }
}

/// A vertical gradient over `rect`, `bottom` to `top`.
fn gradient(mesh: &mut Mesh, rect: Rect, bottom: Color32, top: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_top(), top);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

/// How far up the meter `db` (dBFS) is, 0 to 1 at [`TOP_DB`]: in steps
/// that widen towards the top (as IEC 60268-18 meters), so the levels
/// that matter get most of the height, and above full scale as near it.
/// -70 dB and below is the bottom.
fn scale(db: f32) -> f32 {
    // (dB, height) at each step's start, from the top, full scale at 1.
    const STEPS: [(f32, f32); 8] = [
        (TOP_DB, TOP),
        (0., 1.),
        (-20., 0.5),
        (-30., 0.3),
        (-40., 0.15),
        (-50., 0.075),
        (-60., 0.025),
        (-70., 0.),
    ];
    // As steep above full scale as just under it.
    const TOP: f32 = 1. + TOP_DB * 0.5 / 20.;
    if db >= TOP_DB {
        return 1.;
    }
    for pair in STEPS.windows(2) {
        let ((top_db, top), (bottom_db, bottom)) = (pair[0], pair[1]);
        if db >= bottom_db {
            return (bottom + (db - bottom_db) / (top_db - bottom_db) * (top - bottom)) / TOP;
        }
    }
    0.
}

/// A level in dBFS for the tooltip: `-3.2`, or `-inf` below the scale.
fn format_level(db: f32) -> String {
    if scale(db) <= 0. {
        format!("{:>5}", "-inf")
    } else {
        format!("{db:>5.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_finer_near_the_top() {
        assert_eq!(scale(TOP_DB), 1.);
        assert_eq!(scale(10.), 1.);
        // Full scale near the top, -20 dB at about half of it.
        assert!((0.85..0.9).contains(&scale(0.)), "{}", scale(0.));
        assert!((scale(-20.) - scale(0.) / 2.).abs() < 1e-6);
        assert_eq!(scale(-80.), 0.);
        // The top 20 dB get half the height: 6 dB there are more of it
        // than 20 dB near the bottom.
        assert!(scale(-6.) - scale(-12.) > scale(-40.) - scale(-60.));
        let mut last = 0.;
        for db in (-75..=6).map(|d| d as f32) {
            assert!(scale(db) >= last, "rises with the level, at {db}");
            last = scale(db);
        }
    }
}
