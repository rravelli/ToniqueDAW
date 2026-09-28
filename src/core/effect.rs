//! Effects the user can add to a channel, as saved in project files. Each
//! kind maps to an engine plugin; its editor lives in the UI.

use serde::{Deserialize, Serialize};
use tonique_engine::{
    edit::{Plugin, PluginKind},
    nodes::FilterMode,
};

/// Kind of effect, as saved in project files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    /// Low-pass filter. Older projects call it `equalizer`.
    #[serde(alias = "equalizer")]
    Filter,
}

impl EffectKind {
    /// Shown in the browser and on the effect's header.
    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Filter => "Filter",
        }
    }

    /// The engine plugin doing the processing.
    pub fn plugin_kind(self) -> PluginKind {
        match self {
            EffectKind::Filter => PluginKind::Filter(FilterMode::LowPass),
        }
    }

    /// The effect a plugin is, if the user can add it.
    pub fn of(plugin: &PluginKind) -> Option<Self> {
        match plugin {
            PluginKind::Filter(FilterMode::LowPass) => Some(EffectKind::Filter),
            _ => None,
        }
    }

    /// Parameter values a new effect starts with, where they differ from
    /// the engine's defaults.
    pub fn initial_params(self) -> &'static [(&'static str, f32)] {
        match self {
            EffectKind::Filter => &[("cutoff", 1300.), ("q", 0.5)],
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
