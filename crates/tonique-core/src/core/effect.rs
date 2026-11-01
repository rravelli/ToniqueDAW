//! Effects the user can add to a channel, as saved in project files. Each
//! kind maps to an engine plugin; its editor lives in the UI.

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use tonique_engine::{
    edit::{Plugin, PluginKind},
    nodes::{FilterMode, SaturationCurve},
};

/// Kind of effect, as saved in project files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Low- or high-pass filter. Older projects call it `equalizer`.
    #[serde(alias = "equalizer")]
    Filter,
    /// Feedback delay.
    Echo,
    /// Shows the spectrum at its place in the chain; leaves audio as is.
    Spectrum,
    /// Gain, balance, stereo width, mono and phase inversion.
    Utility,
    /// Parametric EQ: cuts, shelves and peaks.
    Eq,
    /// Evens out levels.
    Compressor,
    /// Keeps peaks under a ceiling.
    Limiter,
    /// Silences or lowers what's under a threshold.
    Gate,
    /// Drive into a soft, tape or hard clip.
    Saturation,
    /// Algorithmic room.
    Reverb,
}

/// Delay of a new echo, in seconds.
pub const ECHO_TIME: f32 = 0.25;
/// Delays an echo can have, in seconds.
pub const ECHO_TIMES: RangeInclusive<f32> = 0.01..=2.;
/// Delays a synced echo can have, in beats, and their names as notes.
pub const ECHO_SYNCS: [(f32, &str); 11] = [
    (0.125, "1/32"),
    (0.25, "1/16"),
    (1. / 3., "1/8T"),
    (0.375, "1/16D"),
    (0.5, "1/8"),
    (2. / 3., "1/4T"),
    (0.75, "1/8D"),
    (1., "1/4"),
    (1.5, "1/4D"),
    (2., "1/2"),
    (4., "1/1"),
];
/// Delay of a new synced echo, in beats.
pub const ECHO_SYNC: f32 = 0.5;
/// Lookahead of a new limiter, in ms.
pub const LIMITER_LOOKAHEAD: f32 = 3.;
/// Lookaheads a limiter can have, in ms.
pub const LIMITER_LOOKAHEADS: RangeInclusive<f32> = 0.5..=20.;

impl EffectKind {
    /// In the order the browser lists them.
    pub const ALL: [EffectKind; 10] = [
        EffectKind::Eq,
        EffectKind::Filter,
        EffectKind::Compressor,
        EffectKind::Limiter,
        EffectKind::Gate,
        EffectKind::Saturation,
        EffectKind::Echo,
        EffectKind::Reverb,
        EffectKind::Utility,
        EffectKind::Spectrum,
    ];

    /// Shown in the browser and on the effect's header.
    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Filter => "Filter",
            EffectKind::Echo => "Echo",
            EffectKind::Spectrum => "Spectrum",
            EffectKind::Utility => "Utility",
            EffectKind::Eq => "EQ",
            EffectKind::Compressor => "Compressor",
            EffectKind::Limiter => "Limiter",
            EffectKind::Gate => "Gate",
            EffectKind::Saturation => "Saturation",
            EffectKind::Reverb => "Reverb",
        }
    }

    /// Other words to find it by: what it does, and what other DAWs call
    /// it.
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            EffectKind::Filter => &[
                "low-pass",
                "lowpass",
                "high-pass",
                "highpass",
                "band-pass",
                "bandpass",
                "notch",
                "shelf",
                "cutoff",
                "resonance",
            ],
            EffectKind::Echo => &["delay", "feedback", "repeat", "tempo", "sync"],
            EffectKind::Spectrum => &["analyzer", "analyser", "fft", "frequency", "meter"],
            EffectKind::Utility => &[
                "gain", "volume", "pan", "balance", "width", "stereo", "mono", "phase", "invert",
            ],
            EffectKind::Eq => &[
                "equalizer",
                "equaliser",
                "parametric",
                "shelf",
                "bell",
                "tone",
                "low-cut",
                "high-cut",
            ],
            EffectKind::Compressor => &["dynamics", "compression", "squash", "glue", "punch"],
            EffectKind::Limiter => &["dynamics", "brickwall", "ceiling", "loudness", "master"],
            EffectKind::Gate => &["dynamics", "noise", "expander", "silence"],
            EffectKind::Saturation => {
                &["distortion", "drive", "overdrive", "tape", "warmth", "clip"]
            }
            EffectKind::Reverb => &["room", "hall", "space", "ambience", "decay"],
        }
    }

    /// Whether a search for `query` finds it: each of its words starts the
    /// name or a keyword (or a part of one, like "pass" in "low-pass"),
    /// whatever the case. Only starts: "eq" shouldn't find "frequency". An
    /// empty query finds all.
    pub fn matches(self, query: &str) -> bool {
        let words: Vec<String> = std::iter::once(self.name())
            .chain(self.keywords().iter().copied())
            .map(str::to_lowercase)
            .collect();
        query.to_lowercase().split_whitespace().all(|term| {
            words.iter().any(|word| {
                word.starts_with(term) || word.split('-').any(|part| part.starts_with(term))
            })
        })
    }

    /// The engine plugin a new effect runs on.
    pub fn plugin_kind(self) -> PluginKind {
        match self {
            EffectKind::Filter => PluginKind::Filter(FilterMode::LowPass),
            EffectKind::Echo => PluginKind::Echo {
                time_s: ECHO_TIME,
                sync: None,
            },
            EffectKind::Spectrum => PluginKind::Analyzer,
            EffectKind::Utility => PluginKind::Utility,
            EffectKind::Eq => PluginKind::Eq,
            EffectKind::Compressor => PluginKind::Compressor,
            EffectKind::Limiter => PluginKind::Limiter {
                lookahead_ms: LIMITER_LOOKAHEAD,
            },
            EffectKind::Gate => PluginKind::Gate,
            EffectKind::Saturation => PluginKind::Saturation(SaturationCurve::SoftClip),
            EffectKind::Reverb => PluginKind::Reverb,
        }
    }

    /// The effect a plugin is, if the user can add it.
    pub fn of(plugin: &PluginKind) -> Option<Self> {
        match plugin {
            PluginKind::Filter(_) => Some(EffectKind::Filter),
            PluginKind::Echo { .. } => Some(EffectKind::Echo),
            PluginKind::Analyzer => Some(EffectKind::Spectrum),
            PluginKind::Utility => Some(EffectKind::Utility),
            PluginKind::Eq => Some(EffectKind::Eq),
            PluginKind::Compressor => Some(EffectKind::Compressor),
            PluginKind::Limiter { .. } => Some(EffectKind::Limiter),
            PluginKind::Gate => Some(EffectKind::Gate),
            PluginKind::Saturation(_) => Some(EffectKind::Saturation),
            PluginKind::Reverb => Some(EffectKind::Reverb),
            PluginKind::Synth(_) | PluginKind::Latency { .. } => None,
        }
    }

    /// Parameter values a new effect starts with, where they differ from
    /// the engine's defaults.
    pub fn initial_params(self) -> &'static [(&'static str, f32)] {
        match self {
            EffectKind::Filter => &[("cutoff", 1300.), ("q", 0.5)],
            // Repeats a little thinner and darker than the input. Echoes
            // from before the cuts keep theirs out of the way.
            EffectKind::Echo => &[("low_cut", 80.), ("high_cut", 8000.)],
            EffectKind::Spectrum
            | EffectKind::Utility
            | EffectKind::Eq
            | EffectKind::Compressor
            | EffectKind::Limiter
            | EffectKind::Gate
            | EffectKind::Saturation
            | EffectKind::Reverb => &[],
        }
    }

    /// What a parameter resets to: its value in a new effect.
    pub fn initial_value(self, plugin: &Plugin, param: &str) -> Option<f32> {
        self.initial_params()
            .iter()
            .find(|(name, _)| *name == param)
            .map(|(_, value)| *value)
            .or_else(|| plugin.param(param).map(|p| p.default))
    }
}

/// A choice that picks an effect's engine plugin rather than drives it,
/// like a filter's mode. The engine fixes it when it makes the plugin, so
/// changing it replaces the plugin. Saved among the parameters, by name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    /// Saved as its place in [`FilterMode::ALL`]: 0 for low-pass, 1 for
    /// high-pass, as before the other modes.
    Mode(FilterMode),
    /// Echo delay, in seconds.
    Time(f32),
    /// Echo delay in beats, following the tempo; `None` to go by its time.
    /// Saved as 0 when off.
    Sync(Option<f32>),
    /// Limiter lookahead, in ms.
    Lookahead(f32),
    /// Saved as its place in [`SaturationCurve::ALL`].
    Curve(SaturationCurve),
}

impl Setting {
    /// The settings `plugin` was made with.
    pub fn all_of(plugin: &PluginKind) -> Vec<Self> {
        match *plugin {
            PluginKind::Filter(mode) => vec![Setting::Mode(mode)],
            PluginKind::Echo { time_s, sync } => vec![Setting::Time(time_s), Setting::Sync(sync)],
            PluginKind::Limiter { lookahead_ms } => vec![Setting::Lookahead(lookahead_ms)],
            PluginKind::Saturation(curve) => vec![Setting::Curve(curve)],
            _ => Vec::new(),
        }
    }

    /// Read from a saved parameter of an `effect`. `None` if it isn't one
    /// of its settings.
    pub fn parse(effect: EffectKind, name: &str, value: f32) -> Option<Self> {
        let index = |len: usize| (value.round().max(0.) as usize).min(len - 1);
        match (effect, name) {
            (EffectKind::Filter, "mode") => {
                Some(Setting::Mode(FilterMode::ALL[index(FilterMode::ALL.len())]))
            }
            (EffectKind::Echo, "time") => Some(Setting::Time(
                value.clamp(*ECHO_TIMES.start(), *ECHO_TIMES.end()),
            )),
            (EffectKind::Echo, "sync") => {
                Some(Setting::Sync((value > 0.).then(|| {
                    value.clamp(ECHO_SYNCS[0].0, ECHO_SYNCS[ECHO_SYNCS.len() - 1].0)
                })))
            }
            (EffectKind::Limiter, "lookahead") => Some(Setting::Lookahead(
                value.clamp(*LIMITER_LOOKAHEADS.start(), *LIMITER_LOOKAHEADS.end()),
            )),
            (EffectKind::Saturation, "curve") => Some(Setting::Curve(
                SaturationCurve::ALL[index(SaturationCurve::ALL.len())],
            )),
            _ => None,
        }
    }

    /// Name and value, as saved.
    pub fn saved(self) -> (&'static str, f32) {
        let place = |found: Option<usize>| found.unwrap_or(0) as f32;
        match self {
            Setting::Mode(mode) => (
                "mode",
                place(FilterMode::ALL.iter().position(|m| *m == mode)),
            ),
            Setting::Time(time) => ("time", time),
            Setting::Sync(beats) => ("sync", beats.unwrap_or(0.)),
            Setting::Lookahead(ms) => ("lookahead", ms),
            Setting::Curve(curve) => (
                "curve",
                place(SaturationCurve::ALL.iter().position(|c| *c == curve)),
            ),
        }
    }

    /// `plugin` made with this setting instead. Unchanged if it doesn't
    /// have this setting.
    pub fn apply(self, plugin: PluginKind) -> PluginKind {
        match (self, plugin) {
            (Setting::Mode(mode), PluginKind::Filter(_)) => PluginKind::Filter(mode),
            (Setting::Time(time_s), PluginKind::Echo { sync, .. }) => {
                PluginKind::Echo { time_s, sync }
            }
            (Setting::Sync(sync), PluginKind::Echo { time_s, .. }) => {
                PluginKind::Echo { time_s, sync }
            }
            (Setting::Lookahead(lookahead_ms), PluginKind::Limiter { .. }) => {
                PluginKind::Limiter { lookahead_ms }
            }
            (Setting::Curve(curve), PluginKind::Saturation(_)) => PluginKind::Saturation(curve),
            _ => plugin,
        }
    }
}

/// An effect on a channel: its kind and the engine plugin running it.
#[derive(Clone, Debug)]
pub struct Effect {
    pub kind: EffectKind,
    pub plugin: Plugin,
    /// Shown folded to a strip.
    pub collapsed: bool,
}

impl Effect {
    /// The user's name for it, or its kind's.
    pub fn name(&self) -> &str {
        self.plugin.name.as_deref().unwrap_or(self.kind.name())
    }

    /// Not bypassed.
    pub fn enabled(&self) -> bool {
        !self.plugin.bypassed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_map_back_to_their_effect() {
        for kind in EffectKind::ALL {
            assert_eq!(EffectKind::of(&kind.plugin_kind()), Some(kind));
        }
        assert_eq!(EffectKind::of(&PluginKind::Latency { samples: 4 }), None);
    }

    #[test]
    fn searches_find_effects_by_name_and_purpose() {
        let found = |query| -> Vec<_> {
            EffectKind::ALL
                .into_iter()
                .filter(|k| k.matches(query))
                .collect()
        };
        assert_eq!(found(""), EffectKind::ALL);
        assert_eq!(found("  "), EffectKind::ALL);
        assert_eq!(found("FIL"), [EffectKind::Filter]);
        assert_eq!(found("delay"), [EffectKind::Echo]);
        assert_eq!(found("eq"), [EffectKind::Eq], "not \"frequency\"");
        assert_eq!(found("pass"), [EffectKind::Filter]);
        assert_eq!(
            found("dynamics"),
            [
                EffectKind::Compressor,
                EffectKind::Limiter,
                EffectKind::Gate
            ]
        );
        assert_eq!(found("distortion"), [EffectKind::Saturation]);
        assert_eq!(found("room"), [EffectKind::Reverb]);
        assert_eq!(found("pan"), [EffectKind::Utility]);
        assert_eq!(found("analy"), [EffectKind::Spectrum]);
        // Every word must match.
        assert_eq!(found("stereo width"), [EffectKind::Utility]);
        assert!(found("stereo delay").is_empty());
        assert!(found("chorus").is_empty());
    }

    #[test]
    fn settings_round_trip_through_their_saved_form() {
        let settings = FilterMode::ALL
            .map(Setting::Mode)
            .into_iter()
            .chain(SaturationCurve::ALL.map(Setting::Curve))
            .chain([
                Setting::Time(0.5),
                Setting::Sync(None),
                Setting::Sync(Some(0.75)),
                Setting::Lookahead(5.),
            ]);
        for setting in settings {
            let effect = match setting {
                Setting::Mode(_) => EffectKind::Filter,
                Setting::Time(_) | Setting::Sync(_) => EffectKind::Echo,
                Setting::Lookahead(_) => EffectKind::Limiter,
                Setting::Curve(_) => EffectKind::Saturation,
            };
            let (name, value) = setting.saved();
            assert_eq!(Setting::parse(effect, name, value), Some(setting));
            let plugin = setting.apply(effect.plugin_kind());
            assert!(Setting::all_of(&plugin).contains(&setting), "{setting:?}");
        }
        // Older projects' modes read as they did.
        assert_eq!(
            Setting::parse(EffectKind::Filter, "mode", 1.),
            Some(Setting::Mode(FilterMode::HighPass))
        );
        // Not a setting of that effect: a parameter.
        assert_eq!(Setting::parse(EffectKind::Echo, "mode", 1.), None);
        assert_eq!(Setting::parse(EffectKind::Filter, "cutoff", 1.), None);
        // Out of range values are clamped.
        assert_eq!(
            Setting::parse(EffectKind::Echo, "time", 60.),
            Some(Setting::Time(*ECHO_TIMES.end()))
        );
        assert_eq!(
            Setting::parse(EffectKind::Filter, "mode", 60.),
            Some(Setting::Mode(FilterMode::Peak))
        );
    }
}
