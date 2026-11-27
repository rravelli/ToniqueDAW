//! A stereo level meter, as Tonique's `LevelMeter`: a bar per channel on a
//! scale finer near the top, up to +6 dB, with a mark at full scale; green,
//! warming to amber by -12 dB and towards orange at full scale, red over it.
//! The peak shows lighter behind the RMS level, the highest recent peak as a
//! line, and clips as a lamp above each bar until the meter is clicked.

use gpui::{Window, linear_color_stop, linear_gradient};

use crate::{
    geometry::Rect,
    theme::{Color, Theme, hsla, mix},
};
use tonique_core::monitor::metrics::AudioMetrics;

const CHANNEL_GAP: f32 = 1.;
const LAMP_HEIGHT: f32 = 3.;
const LAMP_GAP: f32 = 1.;
const LAMP_MIN_HEIGHT: f32 = 30.;
const MARK_MIN_HEIGHT: f32 = 20.;
const TURN_DB: f32 = -18.;
const WARM_DB: f32 = -12.;
const HOT: f32 = 0.45;
const STOPS: [f32; 6] = [-18., -15., -12., -6., -3., 0.];
const TOP_DB: f32 = 6.;
const PEAK_SHADE: f32 = 0.5;

/// The meter's colours, by level.
struct Zones {
    low: Color,
    middle: Color,
    high: Color,
    tick: Color,
}

impl Zones {
    fn new(theme: &Theme, disabled: bool) -> Self {
        let tick = mix(theme.bg_deep, theme.text_disabled, 0.35);
        if disabled {
            let grey = theme.bg_control_hover;
            return Self {
                low: grey,
                middle: grey,
                high: grey,
                tick,
            };
        }
        Self {
            low: theme.meter_low,
            middle: theme.meter_mid,
            high: theme.meter_high,
            tick,
        }
    }

    /// Ranges of levels, from the bottom up, each solid or a gradient.
    fn spans() -> impl Iterator<Item = (f32, f32)> {
        let bounds = std::iter::once(f32::NEG_INFINITY)
            .chain(STOPS)
            .chain([f32::INFINITY]);
        bounds.clone().zip(bounds.skip(1))
    }

    fn color(&self, db: f32) -> Color {
        if db > 0. {
            self.high
        } else if db <= TURN_DB {
            self.low
        } else if db < WARM_DB {
            mix(self.low, self.middle, (db - TURN_DB) / (WARM_DB - TURN_DB))
        } else {
            mix(self.middle, self.high, HOT * (db - WARM_DB) / -WARM_DB)
        }
    }
}

/// How far up the meter `db` is, 0 to 1 at [`TOP_DB`] (IEC 60268-18-like
/// steps, widening towards the top).
pub fn scale(db: f32) -> f32 {
    const TOP: f32 = 1. + TOP_DB * 0.5 / 20.;
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

/// Whether the meter shows anything, so it needs redrawing as it falls.
pub fn is_live(metrics: &AudioMetrics) -> bool {
    metrics
        .peak_db()
        .iter()
        .chain(metrics.hold_db().iter())
        .any(|db| scale(*db) > 0.)
}

pub fn paint_meter(
    window: &mut Window,
    rect: Rect,
    metrics: &AudioMetrics,
    theme: &Theme,
    disabled: bool,
) {
    let lamps = rect.height >= LAMP_MIN_HEIGHT;
    let bars = if lamps {
        Rect::new(
            rect.left,
            rect.top + LAMP_HEIGHT + LAMP_GAP,
            rect.width,
            rect.height - LAMP_HEIGHT - LAMP_GAP,
        )
    } else {
        rect
    };
    let width = (rect.width - CHANNEL_GAP) / 2.;
    let zones = Zones::new(theme, disabled);
    let height_of = |db: f32| bars.bottom() - scale(db) * bars.height;
    let fill = |window: &mut Window, rect: Rect, color: Color| {
        window.paint_quad(gpui::fill(rect.bounds(), hsla(color)))
    };
    let (peaks, holds, rms, clipped) = (
        metrics.peak_db(),
        metrics.hold_db(),
        metrics.rms_db(),
        metrics.clipped(),
    );
    for channel in 0..2 {
        let left = rect.left + channel as f32 * (width + CHANNEL_GAP);
        let column = Rect::new(left, bars.top, width, bars.height);
        fill(window, column, theme.bg_deep);
        if bars.height >= MARK_MIN_HEIGHT {
            fill(
                window,
                Rect::new(left, height_of(0.).round(), width, 1.),
                zones.tick,
            );
        }
        // The peak, lighter, behind the RMS level.
        for (top, shade) in [(peaks[channel], PEAK_SHADE), (rms[channel], 1.)] {
            let shade = |color: Color| mix(theme.bg_deep, color, shade);
            for (from, to) in Zones::spans() {
                let to = to.min(top);
                if to <= from {
                    continue;
                }
                let (segment_top, segment_bottom) = (height_of(to), height_of(from));
                if segment_bottom - segment_top <= 0. {
                    continue;
                }
                let bottom = shade(zones.color(from));
                // Red over full scale is flat; the rest a gradient.
                let top_color = if from >= 0. {
                    bottom
                } else {
                    shade(zones.color(to))
                };
                let segment = Rect::new(left, segment_top, width, segment_bottom - segment_top);
                window.paint_quad(gpui::fill(
                    segment.bounds(),
                    linear_gradient(
                        0.,
                        linear_color_stop(hsla(bottom), 0.),
                        linear_color_stop(hsla(top_color), 1.),
                    ),
                ));
            }
        }
        if scale(holds[channel]) > 0. {
            let at = height_of(holds[channel]).max(column.top + 1.);
            fill(
                window,
                Rect::new(left, at - 1., width, 1.),
                zones.color(holds[channel]),
            );
        }
        if clipped[channel] {
            let lamp = if lamps {
                Rect::new(left, rect.top, width, LAMP_HEIGHT)
            } else {
                Rect::new(left, column.top, width, 2.)
            };
            fill(window, lamp, zones.high);
        } else if lamps {
            fill(
                window,
                Rect::new(left, rect.top, width, LAMP_HEIGHT),
                theme.bg_deep,
            );
        }
    }
}
