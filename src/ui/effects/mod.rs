use crate::ui::{effect::UIEffectContent, effects::equalizer::EqualizerEffect};

pub mod equalizer;

/// Kind of effect, as saved in project files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectId {
    Equalizer,
}

// Associate effect id to the effect struct
pub fn create_effect_from_id(effect_id: EffectId) -> Box<dyn UIEffectContent> {
    match effect_id {
        EffectId::Equalizer => Box::new(EqualizerEffect::new()),
    }
}
