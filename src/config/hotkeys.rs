use std::collections::HashMap;

use egui::{Key, Modifiers, Ui};

// (or whatever input types you’re using)

#[derive(PartialEq, Hash, Eq, Debug)]
pub enum Action {
    Undo,
    Redo,
    PlayPause,
    // ... etc.
}

pub struct Hotkey {
    pub key: Key,
    pub modifiers: Modifiers,
}

pub struct HotkeyManager {
    bindings: HashMap<Action, Hotkey>,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let bindings = HashMap::new();
        // initialize some default bindings ...
        Self { bindings }
    }

    pub fn triggered(&self, input: &egui::InputState, action: Action) -> bool {
        if let Some(binding) = self.bindings.get(&action) {
            input.modifiers == binding.modifiers && input.key_pressed(binding.key)
        } else {
            false
        }
    }
}
