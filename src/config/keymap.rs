use egui::{Context, Event, InputState, Key, KeyboardShortcut, ModifierNames, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An app command that can be bound to keyboard shortcuts.
///
/// Copy/cut/paste and zoom are not here: eframe and egui handle those keys
/// themselves, so they can't be rebound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    // Transport
    PlayStop,
    Loop,
    ToggleMetronome,
    ToggleFollowPlayhead,
    // Edit
    Undo,
    Redo,
    SelectAll,
    Duplicate,
    Delete,
    SplitAtCursor,
    AddTrack,
    NudgeLeft,
    NudgeRight,
    MoveTrackUp,
    MoveTrackDown,
    // View
    ToggleBrowser,
    ToggleEffectsPanel,
    ToggleGraphView,
    OpenSettings,
}

impl Action {
    /// In the order shown in the settings.
    pub const ALL: [Action; 19] = [
        Action::PlayStop,
        Action::Loop,
        Action::ToggleMetronome,
        Action::ToggleFollowPlayhead,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::Duplicate,
        Action::Delete,
        Action::SplitAtCursor,
        Action::AddTrack,
        Action::NudgeLeft,
        Action::NudgeRight,
        Action::MoveTrackUp,
        Action::MoveTrackDown,
        Action::ToggleBrowser,
        Action::ToggleEffectsPanel,
        Action::ToggleGraphView,
        Action::OpenSettings,
    ];

    /// Stable name used in `settings.json`.
    pub fn id(self) -> &'static str {
        match self {
            Action::PlayStop => "play_stop",
            Action::Loop => "loop",
            Action::ToggleMetronome => "toggle_metronome",
            Action::ToggleFollowPlayhead => "toggle_follow_playhead",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::SelectAll => "select_all",
            Action::Duplicate => "duplicate",
            Action::Delete => "delete",
            Action::SplitAtCursor => "split_at_cursor",
            Action::AddTrack => "add_track",
            Action::NudgeLeft => "nudge_left",
            Action::NudgeRight => "nudge_right",
            Action::MoveTrackUp => "move_track_up",
            Action::MoveTrackDown => "move_track_down",
            Action::ToggleBrowser => "toggle_browser",
            Action::ToggleEffectsPanel => "toggle_effects_panel",
            Action::ToggleGraphView => "toggle_graph_view",
            Action::OpenSettings => "open_settings",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::PlayStop => "Play / stop",
            Action::Loop => "Loop selection / toggle loop",
            Action::ToggleMetronome => "Metronome",
            Action::ToggleFollowPlayhead => "Follow playhead",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::SelectAll => "Select all clips",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::SplitAtCursor => "Split clips at cursor",
            Action::AddTrack => "Add audio track",
            Action::NudgeLeft => "Nudge selection or cursor left",
            Action::NudgeRight => "Nudge selection or cursor right",
            Action::MoveTrackUp => "Move selection to track above",
            Action::MoveTrackDown => "Move selection to track below",
            Action::ToggleBrowser => "Show / hide browser",
            Action::ToggleEffectsPanel => "Show / hide effects panel",
            Action::ToggleGraphView => "Switch timeline / audio graph",
            Action::OpenSettings => "Open / close settings",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            Action::PlayStop
            | Action::Loop
            | Action::ToggleMetronome
            | Action::ToggleFollowPlayhead => "Transport",
            Action::ToggleBrowser
            | Action::ToggleEffectsPanel
            | Action::ToggleGraphView
            | Action::OpenSettings => "View",
            _ => "Edit",
        }
    }

    /// Whether the action acts on the timeline's selection or cursor, and
    /// so only works while the timeline is shown. The others work anywhere.
    pub fn is_timeline(self) -> bool {
        matches!(
            self,
            Action::SelectAll
                | Action::Duplicate
                | Action::Delete
                | Action::SplitAtCursor
                | Action::NudgeLeft
                | Action::NudgeRight
                | Action::MoveTrackUp
                | Action::MoveTrackDown
        )
    }

    /// Whether holding the key down repeats the action.
    pub fn repeats(self) -> bool {
        matches!(
            self,
            Action::NudgeLeft
                | Action::NudgeRight
                | Action::MoveTrackUp
                | Action::MoveTrackDown
                | Action::Undo
                | Action::Redo
        )
    }

    pub fn default_bindings(self) -> Vec<KeyboardShortcut> {
        let none = |key| KeyboardShortcut::new(Modifiers::NONE, key);
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let cmd_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        match self {
            Action::PlayStop => vec![none(Key::Space)],
            Action::Loop => vec![cmd(Key::L)],
            Action::ToggleMetronome => vec![cmd(Key::M)],
            Action::ToggleFollowPlayhead => vec![cmd_shift(Key::F)],
            Action::Undo => vec![cmd(Key::Z)],
            Action::Redo => vec![cmd(Key::Y), cmd_shift(Key::Z)],
            Action::SelectAll => vec![cmd(Key::A)],
            Action::Duplicate => vec![cmd(Key::D)],
            Action::Delete => vec![none(Key::Delete), none(Key::Backspace)],
            Action::SplitAtCursor => vec![cmd(Key::K)],
            Action::AddTrack => vec![cmd(Key::T)],
            Action::NudgeLeft => vec![none(Key::ArrowLeft)],
            Action::NudgeRight => vec![none(Key::ArrowRight)],
            Action::MoveTrackUp => vec![none(Key::ArrowUp)],
            Action::MoveTrackDown => vec![none(Key::ArrowDown)],
            Action::ToggleBrowser => vec![cmd(Key::B)],
            Action::ToggleEffectsPanel => vec![cmd(Key::J)],
            Action::ToggleGraphView => vec![cmd(Key::G)],
            Action::OpenSettings => vec![cmd(Key::Comma)],
        }
    }
}

/// Keyboard shortcuts of every [`Action`]. Only the bindings the user
/// changed are stored, so new actions and changed defaults reach existing
/// settings files.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<String, Vec<String>>",
    into = "BTreeMap<String, Vec<String>>"
)]
pub struct Keymap {
    overrides: BTreeMap<Action, Vec<KeyboardShortcut>>,
}

impl Keymap {
    pub fn bindings(&self, action: Action) -> Vec<KeyboardShortcut> {
        self.overrides
            .get(&action)
            .cloned()
            .unwrap_or_else(|| action.default_bindings())
    }

    /// Bind `action` to `bindings`, replacing its current ones.
    pub fn set(&mut self, action: Action, bindings: Vec<KeyboardShortcut>) {
        if bindings == action.default_bindings() {
            self.overrides.remove(&action);
        } else {
            self.overrides.insert(action, bindings);
        }
    }

    /// Bind `shortcut` to `action`, replacing the binding at `index` or
    /// adding one when `index` is `None`. The shortcut is taken away from
    /// any other action, which is returned.
    pub fn assign(
        &mut self,
        action: Action,
        index: Option<usize>,
        shortcut: KeyboardShortcut,
    ) -> Option<Action> {
        let previous = self
            .action_for(shortcut.logical_key, shortcut.modifiers)
            .filter(|other| *other != action);
        if let Some(other) = previous {
            let mut bindings = self.bindings(other);
            bindings.retain(|b| *b != shortcut);
            self.set(other, bindings);
        }
        let mut bindings = self.bindings(action);
        match index.filter(|i| *i < bindings.len()) {
            Some(i) => bindings[i] = shortcut,
            None => bindings.push(shortcut),
        }
        // Rebinding to a shortcut the action already has leaves a duplicate.
        let mut seen = Vec::new();
        bindings.retain(|b| {
            let new = !seen.contains(b);
            seen.push(*b);
            new
        });
        self.set(action, bindings);
        previous
    }

    pub fn remove(&mut self, action: Action, index: usize) {
        let mut bindings = self.bindings(action);
        if index < bindings.len() {
            bindings.remove(index);
            self.set(action, bindings);
        }
    }

    pub fn reset(&mut self, action: Action) {
        self.overrides.remove(&action);
    }

    pub fn is_default(&self, action: Action) -> bool {
        !self.overrides.contains_key(&action)
    }

    pub fn is_all_default(&self) -> bool {
        self.overrides.is_empty()
    }

    /// First binding of `action` as shown in menus, e.g. `Ctrl+Z`.
    pub fn shortcut_text(&self, ctx: &Context, action: Action) -> String {
        self.bindings(action)
            .first()
            .map(|shortcut| ctx.format_shortcut(shortcut))
            .unwrap_or_default()
    }

    /// `text` followed by the first binding of `action`, for tooltips and
    /// menu labels: `Loop (Ctrl+L)`.
    pub fn with_shortcut(&self, ctx: &Context, text: &str, action: Action) -> String {
        match self.shortcut_text(ctx, action).as_str() {
            "" => text.to_string(),
            shortcut => format!("{text} ({shortcut})"),
        }
    }

    /// The action bound to `key` pressed with exactly `modifiers`.
    pub fn action_for(&self, key: Key, modifiers: Modifiers) -> Option<Action> {
        Action::ALL.into_iter().find(|action| {
            self.bindings(*action).iter().any(|shortcut| {
                shortcut.logical_key == key && modifiers.matches_exact(shortcut.modifiers)
            })
        })
    }

    /// Actions triggered by this frame's key presses, in the order pressed.
    pub fn triggered(&self, input: &InputState) -> Vec<Action> {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } => self
                    .action_for(*key, *modifiers)
                    .filter(|action| !repeat || action.repeats()),
                _ => None,
            })
            .collect()
    }
}

impl From<BTreeMap<String, Vec<String>>> for Keymap {
    /// Unknown actions and unparsable shortcuts are skipped, so a bad entry
    /// doesn't reset the rest of the settings.
    fn from(saved: BTreeMap<String, Vec<String>>) -> Self {
        let overrides = saved
            .into_iter()
            .filter_map(|(id, bindings)| {
                let bindings = bindings.iter().filter_map(|s| parse_shortcut(s)).collect();
                Some((Action::from_id(&id)?, bindings))
            })
            .collect();
        Self { overrides }
    }
}

impl From<Keymap> for BTreeMap<String, Vec<String>> {
    fn from(keymap: Keymap) -> Self {
        keymap
            .overrides
            .into_iter()
            .map(|(action, bindings)| {
                (
                    action.id().to_string(),
                    bindings.iter().map(format_shortcut).collect(),
                )
            })
            .collect()
    }
}

/// A shortcut handled by eframe or egui, shown but not rebindable.
pub struct FixedShortcut {
    pub label: &'static str,
    pub group: &'static str,
    pub shortcuts: &'static [KeyboardShortcut],
}

pub const FIXED_SHORTCUTS: [FixedShortcut; 6] = [
    FixedShortcut {
        label: "Copy",
        group: "Edit",
        shortcuts: &[KeyboardShortcut::new(Modifiers::COMMAND, Key::C)],
    },
    FixedShortcut {
        label: "Cut",
        group: "Edit",
        shortcuts: &[KeyboardShortcut::new(Modifiers::COMMAND, Key::X)],
    },
    FixedShortcut {
        label: "Paste",
        group: "Edit",
        shortcuts: &[KeyboardShortcut::new(Modifiers::COMMAND, Key::V)],
    },
    FixedShortcut {
        label: "Zoom in",
        group: "View",
        shortcuts: &[
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus),
            KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals),
        ],
    },
    FixedShortcut {
        label: "Zoom out",
        group: "View",
        shortcuts: &[KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus)],
    },
    FixedShortcut {
        label: "Reset zoom",
        group: "View",
        shortcuts: &[KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0)],
    },
];

/// The fixed shortcut `shortcut` would clash with.
pub fn reserved_by(shortcut: &KeyboardShortcut) -> Option<&'static FixedShortcut> {
    FIXED_SHORTCUTS
        .iter()
        .find(|fixed| fixed.shortcuts.contains(shortcut))
}

/// The shortcut for `key` pressed with `modifiers`, in the form bindings
/// are stored: Ctrl and Cmd both become [`Modifiers::COMMAND`].
pub fn shortcut_from_press(key: Key, modifiers: Modifiers) -> KeyboardShortcut {
    let mut normalized = Modifiers::NONE;
    normalized.alt = modifiers.alt;
    normalized.shift = modifiers.shift;
    normalized.command = modifiers.command || modifiers.ctrl || modifiers.mac_cmd;
    KeyboardShortcut::new(normalized, key)
}

/// `Ctrl+Shift+Z`. `Ctrl` stands for Cmd on macOS.
pub fn format_shortcut(shortcut: &KeyboardShortcut) -> String {
    shortcut.format(&ModifierNames::NAMES, false)
}

/// Inverse of [`format_shortcut`]; also accepts `Cmd` and `Option`.
pub fn parse_shortcut(text: &str) -> Option<KeyboardShortcut> {
    // The key may itself be `+` (`Ctrl++`).
    let (modifiers, key) = match text.strip_suffix("++") {
        Some(modifiers) => (modifiers, "+"),
        None => text.rsplit_once('+').unwrap_or(("", text)),
    };
    let mut parsed = Modifiers::NONE;
    for name in modifiers.split('+').filter(|name| !name.is_empty()) {
        parsed = parsed
            | match name.trim() {
                "Ctrl" | "Cmd" => Modifiers::COMMAND,
                "Shift" => Modifiers::SHIFT,
                "Alt" | "Option" => Modifiers::ALT,
                _ => return None,
            };
    }
    Some(KeyboardShortcut::new(parsed, Key::from_name(key.trim())?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_round_trip_through_text() {
        for action in Action::ALL {
            for shortcut in action.default_bindings() {
                let text = format_shortcut(&shortcut);
                assert_eq!(parse_shortcut(&text), Some(shortcut), "{text}");
            }
        }
        let plus = KeyboardShortcut::new(Modifiers::COMMAND, Key::Plus);
        assert_eq!(parse_shortcut("Ctrl++"), Some(plus));
        assert_eq!(parse_shortcut("Cmd+Plus"), Some(plus));
        assert_eq!(parse_shortcut("Hyper+A"), None);
        assert_eq!(parse_shortcut("Ctrl+NotAKey"), None);
    }

    #[test]
    fn action_ids_are_unique() {
        for action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
    }

    #[test]
    fn default_bindings_do_not_conflict() {
        let keymap = Keymap::default();
        for action in Action::ALL {
            for shortcut in keymap.bindings(action) {
                assert_eq!(
                    keymap.action_for(shortcut.logical_key, shortcut.modifiers),
                    Some(action),
                    "{}",
                    format_shortcut(&shortcut)
                );
            }
        }
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let keymap = Keymap::default();
        let ctrl_shift = Modifiers::CTRL | Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(keymap.action_for(Key::Z, ctrl_shift), Some(Action::Redo));
        assert_eq!(
            keymap.action_for(Key::Z, Modifiers::CTRL | Modifiers::COMMAND),
            Some(Action::Undo)
        );
        assert_eq!(keymap.action_for(Key::ArrowLeft, Modifiers::SHIFT), None);
    }

    #[test]
    fn only_overrides_are_saved() {
        let mut keymap = Keymap::default();
        keymap.set(
            Action::Undo,
            vec![KeyboardShortcut::new(Modifiers::ALT, Key::U)],
        );
        keymap.set(Action::Redo, Action::Redo.default_bindings());
        let json = serde_json::to_string(&keymap).unwrap();
        assert_eq!(json, r#"{"undo":["Alt+U"]}"#);
        assert_eq!(serde_json::from_str::<Keymap>(&json).unwrap(), keymap);
    }

    #[test]
    fn assigning_takes_the_shortcut_from_other_actions() {
        let mut keymap = Keymap::default();
        let ctrl_d = KeyboardShortcut::new(Modifiers::COMMAND, Key::D);
        assert_eq!(
            keymap.assign(Action::Loop, Some(0), ctrl_d),
            Some(Action::Duplicate)
        );
        assert_eq!(keymap.bindings(Action::Loop), vec![ctrl_d]);
        assert!(keymap.bindings(Action::Duplicate).is_empty());

        // Adding a binding the action already has doesn't duplicate it.
        assert_eq!(keymap.assign(Action::Loop, None, ctrl_d), None);
        assert_eq!(keymap.bindings(Action::Loop), vec![ctrl_d]);

        keymap.reset(Action::Loop);
        keymap.reset(Action::Duplicate);
        assert!(keymap.is_all_default());
    }

    #[test]
    fn presses_normalize_to_command() {
        let ctrl = Modifiers::CTRL | Modifiers::COMMAND;
        let mac_cmd = Modifiers::MAC_CMD | Modifiers::COMMAND;
        let expected = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(
            shortcut_from_press(Key::Z, ctrl | Modifiers::SHIFT),
            expected
        );
        assert_eq!(
            shortcut_from_press(Key::Z, mac_cmd | Modifiers::SHIFT),
            expected
        );
        assert!(reserved_by(&shortcut_from_press(Key::C, ctrl)).is_some());
        assert!(reserved_by(&shortcut_from_press(Key::C, Modifiers::NONE)).is_none());
    }

    #[test]
    fn fixed_shortcuts_are_not_bound_by_default() {
        let keymap = Keymap::default();
        for fixed in &FIXED_SHORTCUTS {
            for shortcut in fixed.shortcuts {
                assert_eq!(
                    keymap.action_for(shortcut.logical_key, shortcut.modifiers),
                    None
                );
            }
        }
    }

    #[test]
    fn bad_entries_are_skipped() {
        let keymap: Keymap =
            serde_json::from_str(r#"{"undo":["Alt+U","Bogus"],"not_an_action":["A"]}"#).unwrap();
        assert_eq!(
            keymap.bindings(Action::Undo),
            vec![KeyboardShortcut::new(Modifiers::ALT, Key::U)]
        );
        assert_eq!(keymap.overrides.len(), 1);
    }
}
