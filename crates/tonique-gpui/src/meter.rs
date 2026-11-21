//! A stereo level meter, as Tonique's `LevelMeter`: a bar per channel on a
//! scale finer near the top, up to +6 dB, with a mark at full scale; green,
//! warming to amber by -12 dB and towards orange at full scale, red over it.
//! The peak shows lighter behind the RMS level, the highest recent peak as a
//! line, and clips as a lamp above each bar until the meter is clicked.

use gpui::{Window, linear_color_stop, linear_gradient};

use crate::{
    core::metrics::AudioMetrics,
    theme::{Color, Theme, h, mix},
    wave::R,
};

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
    mid: Color,
    high: Color,
    tick: Color,
}

impl Zones {
    fn new(t: &Theme, disabled: bool) -> Self {
        let tick = mix(t.bg_deep, t.text_disabled, 0.35);
        if disabled {
            let grey = t.bg_control_hover;
            return Self {
                low: grey,
                mid: grey,
                high: grey,
                tick,
            };
        }
        Self {
            low: t.meter_low,
            mid: t.meter_mid,
            high: t.meter_high,
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
            mix(self.low, self.mid, (db - TURN_DB) / (WARM_DB - TURN_DB))
        } else {
            mix(self.mid, self.high, HOT * (db - WARM_DB) / -WARM_DB)
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
pub fn is_live(m: &AudioMetrics) -> bool {
    m.peak_db()
        .iter()
        .chain(m.hold_db().iter())
        .any(|db| scale(*db) > 0.)
}

pub fn paint_meter(window: &mut Window, r: R, m: &AudioMetrics, t: &Theme, disabled: bool) {
    let lamps = r.h >= LAMP_MIN_HEIGHT;
    let bars = if lamps {
        R::new(
            r.x,
            r.y + LAMP_HEIGHT + LAMP_GAP,
            r.w,
            r.h - LAMP_HEIGHT - LAMP_GAP,
        )
    } else {
        r
    };
    let width = (r.w - CHANNEL_GAP) / 2.;
    let zones = Zones::new(t, disabled);
    let y = |db: f32| bars.bottom() - scale(db) * bars.h;
    let fill =
        |window: &mut Window, rect: R, c: Color| window.paint_quad(gpui::fill(rect.bounds(), h(c)));
    let (peaks, holds, rms, clipped) = (m.peak_db(), m.hold_db(), m.rms_db(), m.clipped());
    for ch in 0..2 {
        let left = r.x + ch as f32 * (width + CHANNEL_GAP);
        let column = R::new(left, bars.y, width, bars.h);
        fill(window, column, t.bg_deep);
        if bars.h >= MARK_MIN_HEIGHT {
            fill(window, R::new(left, y(0.).round(), width, 1.), zones.tick);
        }
        // The peak, lighter, behind the RMS level.
        for (top, shade) in [(peaks[ch], PEAK_SHADE), (rms[ch], 1.)] {
            let shade = |c: Color| mix(t.bg_deep, c, shade);
            for (from, to) in Zones::spans() {
                let to = to.min(top);
                if to <= from {
                    continue;
                }
                let (y0, y1) = (y(to), y(from));
                if y1 - y0 <= 0. {
                    continue;
                }
                let bottom = shade(zones.color(from));
                // Red over full scale is flat; the rest a gradient.
                let top_color = if from >= 0. {
                    bottom
                } else {
                    shade(zones.color(to))
                };
                let segment = R::new(left, y0, width, y1 - y0);
                window.paint_quad(gpui::fill(
                    segment.bounds(),
                    linear_gradient(
                        0.,
                        linear_color_stop(h(bottom), 0.),
                        linear_color_stop(h(top_color), 1.),
                    ),
                ));
            }
        }
        if scale(holds[ch]) > 0. {
            let at = y(holds[ch]).max(column.y + 1.);
            fill(
                window,
                R::new(left, at - 1., width, 1.),
                zones.color(holds[ch]),
            );
        }
        if clipped[ch] {
            let lamp = if lamps {
                R::new(left, r.y, width, LAMP_HEIGHT)
            } else {
                R::new(left, column.y, width, 2.)
            };
            fill(window, lamp, zones.high);
        } else if lamps {
            fill(window, R::new(left, r.y, width, LAMP_HEIGHT), t.bg_deep);
        }
    }
}
