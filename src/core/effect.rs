//! Effects the user can add to a channel, as saved in project files. Each
//! kind maps to an engine plugin; its editor lives in the UI.

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use tonique_engine::{
    edit::{Plugin, PluginKind},
    nodes::FilterMode,
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
}

/// Delay of a new echo, in seconds.
pub const ECHO_TIME: f32 = 0.25;
/// Delays an echo can have, in seconds.
pub const ECHO_TIMES: RangeInclusive<f32> = 0.01..=2.;

impl EffectKind {
    /// In the order the browser lists them.
    pub const ALL: [EffectKind; 4] = [
        EffectKind::Filter,
        EffectKind::Echo,
        EffectKind::Spectrum,
        EffectKind::Utility,
    ];

    /// Shown in the browser and on the effect's header.
    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Filter => "Filter",
            EffectKind::Echo => "Echo",
            EffectKind::Spectrum => "Spectrum",
            EffectKind::Utility => "Utility",
        }
    }

    /// Other words to find it by: what it does, and what other DAWs call
    /// it.
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            EffectKind::Filter => &[
                "eq",
                "equalizer",
                "low-pass",
                "lowpass",
                "high-pass",
                "highpass",
                "cutoff",
                "resonance",
            ],
            EffectKind::Echo => &["delay", "feedback", "repeat"],
            EffectKind::Spectrum => &["analyzer", "analyser", "fft", "frequency", "meter"],
            EffectKind::Utility => &[
                "gain", "volume", "pan", "balance", "width", "stereo", "mono", "phase", "invert",
            ],
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
            EffectKind::Echo => PluginKind::Echo { time_s: ECHO_TIME },
            EffectKind::Spectrum => PluginKind::Analyzer,
            EffectKind::Utility => PluginKind::Utility,
        }
    }

    /// The effect a plugin is, if the user can add it.
    pub fn of(plugin: &PluginKind) -> Option<Self> {
        match plugin {
            PluginKind::Filter(_) => Some(EffectKind::Filter),
            PluginKind::Echo { .. } => Some(EffectKind::Echo),
            PluginKind::Analyzer => Some(EffectKind::Spectrum),
            PluginKind::Utility => Some(EffectKind::Utility),
            _ => None,
        }
    }

    /// Parameter values a new effect starts with, where they differ from
    /// the engine's defaults.
    pub fn initial_params(self) -> &'static [(&'static str, f32)] {
        match self {
            EffectKind::Filter => &[("cutoff", 1300.), ("q", 0.5)],
            EffectKind::Echo | EffectKind::Spectrum | EffectKind::Utility => &[],
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
    /// Saved as 0 for low-pass, 1 for high-pass.
    Mode(FilterMode),
    /// Echo delay, in seconds.
    Time(f32),
}

impl Setting {
    /// The setting `plugin` was made with.
    pub fn of(plugin: &PluginKind) -> Option<Self> {
        match *plugin {
            PluginKind::Filter(mode) => Some(Setting::Mode(mode)),
            PluginKind::Echo { time_s } => Some(Setting::Time(time_s)),
            _ => None,
        }
    }

    /// Read from a saved parameter of an `effect`. `None` if it isn't one
    /// of its settings.
    pub fn parse(effect: EffectKind, name: &str, value: f32) -> Option<Self> {
        match (effect, name) {
            (EffectKind::Filter, "mode") => Some(Setting::Mode(if value >= 0.5 {
                FilterMode::HighPass
            } else {
                FilterMode::LowPass
            })),
            (EffectKind::Echo, "time") => Some(Setting::Time(
                value.clamp(*ECHO_TIMES.start(), *ECHO_TIMES.end()),
            )),
            _ => None,
        }
    }

    /// Name and value, as saved.
    pub fn saved(self) -> (&'static str, f32) {
        match self {
            Setting::Mode(FilterMode::LowPass) => ("mode", 0.),
            Setting::Mode(FilterMode::HighPass) => ("mode", 1.),
            Setting::Time(time) => ("time", time),
        }
    }

    /// `plugin` made with this setting instead. Unchanged if it doesn't
    /// have this setting.
    pub fn apply(self, plugin: PluginKind) -> PluginKind {
        match (self, plugin) {
            (Setting::Mode(mode), PluginKind::Filter(_)) => PluginKind::Filter(mode),
            (Setting::Time(time_s), PluginKind::Echo { .. }) => PluginKind::Echo { time_s },
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
        assert_eq!(found("eq"), [EffectKind::Filter], "not \"frequency\"");
        assert_eq!(found("pass"), [EffectKind::Filter]);
        assert_eq!(found("pan"), [EffectKind::Utility]);
        assert_eq!(found("analy"), [EffectKind::Spectrum]);
        // Every word must match.
        assert_eq!(found("stereo width"), [EffectKind::Utility]);
        assert!(found("stereo delay").is_empty());
        assert!(found("reverb").is_empty());
    }

    #[test]
    fn settings_round_trip_through_their_saved_form() {
        for setting in [
            Setting::Mode(FilterMode::LowPass),
            Setting::Mode(FilterMode::HighPass),
            Setting::Time(0.5),
        ] {
            let effect = match setting {
                Setting::Mode(_) => EffectKind::Filter,
                Setting::Time(_) => EffectKind::Echo,
            };
            let (name, value) = setting.saved();
            assert_eq!(Setting::parse(effect, name, value), Some(setting));
            let plugin = setting.apply(effect.plugin_kind());
            assert_eq!(Setting::of(&plugin), Some(setting));
        }
        // Not a setting of that effect: a parameter.
        assert_eq!(Setting::parse(EffectKind::Echo, "mode", 1.), None);
        assert_eq!(Setting::parse(EffectKind::Filter, "cutoff", 1.), None);
        // Out of range times are clamped.
        assert_eq!(
            Setting::parse(EffectKind::Echo, "time", 60.),
            Some(Setting::Time(*ECHO_TIMES.end()))
        );
    }
}
