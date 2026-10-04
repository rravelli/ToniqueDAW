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
}

/// Delay of a new echo, in seconds.
pub const ECHO_TIME: f32 = 0.25;
/// Delays an echo can have, in seconds.
pub const ECHO_TIMES: RangeInclusive<f32> = 0.01..=2.;

impl EffectKind {
    /// In the order the browser lists them.
    pub const ALL: [EffectKind; 2] = [EffectKind::Filter, EffectKind::Echo];

    /// Shown in the browser and on the effect's header.
    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Filter => "Filter",
            EffectKind::Echo => "Echo",
        }
    }

    /// The engine plugin a new effect runs on.
    pub fn plugin_kind(self) -> PluginKind {
        match self {
            EffectKind::Filter => PluginKind::Filter(FilterMode::LowPass),
            EffectKind::Echo => PluginKind::Echo { time_s: ECHO_TIME },
        }
    }

    /// The effect a plugin is, if the user can add it.
    pub fn of(plugin: &PluginKind) -> Option<Self> {
        match plugin {
            PluginKind::Filter(_) => Some(EffectKind::Filter),
            PluginKind::Echo { .. } => Some(EffectKind::Echo),
            _ => None,
        }
    }

    /// Parameter values a new effect starts with, where they differ from
    /// the engine's defaults.
    pub fn initial_params(self) -> &'static [(&'static str, f32)] {
        match self {
            EffectKind::Filter => &[("cutoff", 1300.), ("q", 0.5)],
            EffectKind::Echo => &[],
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
}

impl Effect {
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
