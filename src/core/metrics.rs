use std::{collections::HashMap, time::Instant};
use tonique_engine::{
    edit::TrackId,
    meter::{ChannelMeter, SCOPE_LEN},
};

/// How fast a meter's peak falls back, in dB per second.
const PEAK_FALL: f32 = 24.;
/// How long a meter's peak hold stays before it falls too, in seconds.
const PEAK_HOLD: f32 = 1.5;
/// The time over which the RMS level is averaged, in seconds.
const RMS_TIME: f32 = 0.3;
/// After this long without audio processed (seconds), levels fall as if
/// silent, rather than stay where playback stopped.
const IDLE: f32 = 0.1;
/// Below this, a level shows as silence.
pub const FLOOR_DB: f32 = -96.;

/// A stereo channel's levels as a meter shows them: the peak, jumping up
/// at once and falling back steadily; the highest recent peak, held a
/// moment; the RMS level, averaged; and whether it clipped, latched until
/// reset.
#[derive(Clone)]
pub struct AudioMetrics {
    /// dBFS.
    peak: [f32; 2],
    /// dBFS.
    hold: [f32; 2],
    /// Seconds since the hold was set.
    hold_age: [f32; 2],
    /// Mean square, averaged.
    power: [f32; 2],
    clipped: [bool; 2],
    last_update: Option<Instant>,
    /// Seconds since audio was last processed.
    idle: f32,
    /// Most recent samples, for the scope (the master's only).
    pub samples: [Vec<f32>; 2],
}

impl AudioMetrics {
    pub fn new() -> Self {
        Self {
            peak: [FLOOR_DB; 2],
            hold: [FLOOR_DB; 2],
            hold_age: [0.; 2],
            power: [0.; 2],
            clipped: [false; 2],
            last_update: None,
            idle: 0.,
            samples: [vec![], vec![]],
        }
    }

    /// Pull the latest levels from an engine channel meter, and its recent
    /// samples if `scope`: only the master's are shown, and copying every
    /// channel's each frame adds up.
    pub fn update(&mut self, meter: &ChannelMeter, scope: bool) {
        let now = Instant::now();
        let dt = self
            .last_update
            .map_or(0., |last| now.duration_since(last).as_secs_f32().min(0.25));
        self.last_update = Some(now);
        let levels = meter.take_levels();
        if levels.is_some() {
            self.idle = 0.;
        } else {
            self.idle += dt;
        }
        // The UI can redraw faster than audio is processed: keep the last
        // levels a moment, then fall as if silent.
        if levels.is_some() || self.idle > IDLE {
            let levels = levels.unwrap_or_default();
            self.apply(levels.map(|l| (l.peak, l.rms)), dt);
        } else {
            self.apply([(0., self.power[0].sqrt()), (0., self.power[1].sqrt())], dt);
        }
        if scope {
            for ch in 0..2 {
                self.samples[ch].resize(SCOPE_LEN, 0.);
                meter.read_scope(ch, &mut self.samples[ch]);
            }
        }
    }

    /// Take in each channel's (peak, RMS), linear, over the last `dt`
    /// seconds.
    fn apply(&mut self, levels: [(f32, f32); 2], dt: f32) {
        let rms_step = 1. - (-dt / RMS_TIME).exp();
        for (ch, (peak, rms)) in levels.into_iter().enumerate() {
            let db = to_db(peak);
            self.peak[ch] = db.max(self.peak[ch] - PEAK_FALL * dt);
            if db >= self.hold[ch] {
                self.hold[ch] = db;
                self.hold_age[ch] = 0.;
            } else {
                self.hold_age[ch] += dt;
                if self.hold_age[ch] > PEAK_HOLD {
                    self.hold[ch] = (self.hold[ch] - PEAK_FALL * dt).max(self.peak[ch]);
                }
            }
            self.power[ch] += rms_step * (rms * rms - self.power[ch]);
            self.clipped[ch] |= peak > 1.;
        }
    }

    /// The peak shown, in dBFS ([`FLOOR_DB`] for silence).
    pub fn peak_db(&self) -> [f32; 2] {
        self.peak
    }

    /// The highest recent peak, in dBFS.
    pub fn hold_db(&self) -> [f32; 2] {
        self.hold
    }

    /// The RMS level, averaged, in dBFS.
    pub fn rms_db(&self) -> [f32; 2] {
        self.power.map(|p| to_db(p.sqrt()))
    }

    /// Whether each channel went over full scale since [`Self::reset_clip`].
    pub fn clipped(&self) -> [bool; 2] {
        self.clipped
    }

    /// Forget the clips, and the held peaks.
    pub fn reset_clip(&mut self) {
        self.clipped = [false; 2];
        self.hold = self.peak;
    }
}

fn to_db(level: f32) -> f32 {
    if level > 0. {
        (20. * level.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

#[derive(Clone)]
pub struct GlobalMetrics {
    pub master: AudioMetrics,
    pub tracks: HashMap<TrackId, AudioMetrics>,
    pub latency: f32,
}

impl GlobalMetrics {
    pub fn new() -> Self {
        Self {
            master: AudioMetrics::new(),
            tracks: HashMap::new(),
            latency: 0.,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaks_jump_up_fall_back_and_hold() {
        let mut m = AudioMetrics::new();
        m.apply([(1., 0.5), (0.5, 0.25)], 0.016);
        assert!(m.peak_db()[0].abs() < 1e-4, "jumps to 0 dB at once");
        // Half a second of silence: falls by half the fall rate.
        for _ in 0..50 {
            m.apply([(0., 0.); 2], 0.01);
        }
        assert!((m.peak_db()[0] + PEAK_FALL / 2.).abs() < 0.1);
        assert!(m.hold_db()[0].abs() < 1e-4, "the hold stays");
        for _ in 0..200 {
            m.apply([(0., 0.); 2], 0.01);
        }
        assert!(m.hold_db()[0] < -20., "then falls too");
        assert_eq!(m.clipped(), [false, false], "full scale isn't a clip");
    }

    #[test]
    fn clips_latch_until_reset() {
        let mut m = AudioMetrics::new();
        m.apply([(1.2, 0.5), (0.5, 0.2)], 0.016);
        for _ in 0..100 {
            m.apply([(0., 0.); 2], 0.05);
        }
        assert_eq!(m.clipped(), [true, false]);
        m.reset_clip();
        assert_eq!(m.clipped(), [false, false]);
    }

    #[test]
    fn rms_settles_on_the_level() {
        let mut m = AudioMetrics::new();
        for _ in 0..300 {
            m.apply([(0.5, 0.5); 2], 0.01);
        }
        assert!((m.rms_db()[0] - to_db(0.5)).abs() < 0.1);
    }
}
