use crate::ui::{effect::UIEffectContent, effects::equalizer::EqualizerEffect};

pub mod equalizer;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectId {
    Equalizer,
}

// Associate effect id to the effect struct
pub fn create_effect_from_id(effect_id: EffectId) -> Box<dyn UIEffectContent> {
    match effect_id {
        EffectId::Equalizer => Box::new(EqualizerEffect::new()),
    }
}
