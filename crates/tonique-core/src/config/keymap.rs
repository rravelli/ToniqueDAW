pub use super::keys::Key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// An app command that can be bound to keyboard shortcuts.
///
/// Copy/cut/paste and zoom are not here: the UI toolkit handles those keys
/// itself, so they can't be rebound (see [`FIXED_SHORTCUTS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    // File
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ExportAudio,
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
    Rename,
    Delete,
    SplitAtCursor,
    AddTrack,
    GroupTracks,
    NudgeLeft,
    NudgeRight,
    MoveTrackUp,
    MoveTrackDown,
    // Grid
    NarrowGrid,
    WidenGrid,
    ToggleTripletGrid,
    ToggleAdaptiveGrid,
    ToggleSnap,
    // View
    ToggleBrowser,
    ToggleEffectsPanel,
    ToggleTrackList,
    ToggleGraphView,
    OpenSettings,
}

impl Action {
    /// In the order shown in the settings.
    pub const ALL: [Action; 32] = [
        Action::NewProject,
        Action::OpenProject,
        Action::SaveProject,
        Action::SaveProjectAs,
        Action::ExportAudio,
        Action::PlayStop,
        Action::Loop,
        Action::ToggleMetronome,
        Action::ToggleFollowPlayhead,
        Action::Undo,
        Action::Redo,
        Action::SelectAll,
        Action::Duplicate,
        Action::Rename,
        Action::Delete,
        Action::SplitAtCursor,
        Action::AddTrack,
        Action::GroupTracks,
        Action::NudgeLeft,
        Action::NudgeRight,
        Action::MoveTrackUp,
        Action::MoveTrackDown,
        Action::NarrowGrid,
        Action::WidenGrid,
        Action::ToggleTripletGrid,
        Action::ToggleAdaptiveGrid,
        Action::ToggleSnap,
        Action::ToggleBrowser,
        Action::ToggleEffectsPanel,
        Action::ToggleTrackList,
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
            Action::Rename => "rename",
            Action::Delete => "delete",
            Action::SplitAtCursor => "split_at_cursor",
            Action::AddTrack => "add_track",
            Action::GroupTracks => "group_tracks",
            Action::NudgeLeft => "nudge_left",
            Action::NudgeRight => "nudge_right",
            Action::MoveTrackUp => "move_track_up",
            Action::MoveTrackDown => "move_track_down",
            Action::NarrowGrid => "narrow_grid",
            Action::WidenGrid => "widen_grid",
            Action::ToggleTripletGrid => "toggle_triplet_grid",
            Action::ToggleAdaptiveGrid => "toggle_adaptive_grid",
            Action::ToggleSnap => "toggle_snap",
            Action::ToggleBrowser => "toggle_browser",
            Action::ToggleEffectsPanel => "toggle_effects_panel",
            Action::ToggleTrackList => "toggle_track_list",
            Action::ToggleGraphView => "toggle_graph_view",
            Action::OpenSettings => "open_settings",
            Action::NewProject => "new_project",
            Action::OpenProject => "open_project",
            Action::SaveProject => "save_project",
            Action::SaveProjectAs => "save_project_as",
            Action::ExportAudio => "export_audio",
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
            Action::Rename => "Rename",
            Action::Delete => "Delete",
            Action::SplitAtCursor => "Split clips at cursor",
            Action::AddTrack => "Add audio track",
            Action::GroupTracks => "Group selected tracks",
            Action::NudgeLeft => "Nudge selection or cursor left",
            Action::NudgeRight => "Nudge selection or cursor right",
            Action::MoveTrackUp => "Move selection to track above",
            Action::MoveTrackDown => "Move selection to track below",
            Action::NarrowGrid => "Narrow grid",
            Action::WidenGrid => "Widen grid",
            Action::ToggleTripletGrid => "Triplet grid",
            Action::ToggleAdaptiveGrid => "Switch fixed / adaptive grid",
            Action::ToggleSnap => "Snap to grid",
            Action::ToggleBrowser => "Show / hide browser",
            Action::ToggleEffectsPanel => "Show / hide effects panel",
            Action::ToggleTrackList => "Expand / collapse track list",
            Action::ToggleGraphView => "Switch timeline / audio graph",
            Action::OpenSettings => "Open / close settings",
            Action::NewProject => "New project",
            Action::OpenProject => "Open project…",
            Action::SaveProject => "Save project",
            Action::SaveProjectAs => "Save project as…",
            Action::ExportAudio => "Export audio…",
        }
    }

    /// Whether this build offers the action: the audio graph is a
    /// development tool, only in debug builds. Unavailable actions have no
    /// shortcut and aren't listed in the shortcut settings.
    pub fn is_available(self) -> bool {
        self != Action::ToggleGraphView || cfg!(debug_assertions)
    }

    pub fn group(self) -> &'static str {
        match self {
            Action::NewProject
            | Action::OpenProject
            | Action::SaveProject
            | Action::SaveProjectAs
            | Action::ExportAudio => "File",
            Action::PlayStop
            | Action::Loop
            | Action::ToggleMetronome
            | Action::ToggleFollowPlayhead => "Transport",
            Action::ToggleBrowser
            | Action::ToggleEffectsPanel
            | Action::ToggleTrackList
            | Action::ToggleGraphView
            | Action::OpenSettings => "View",
            Action::NarrowGrid
            | Action::WidenGrid
            | Action::ToggleTripletGrid
            | Action::ToggleAdaptiveGrid
            | Action::ToggleSnap => "Grid",
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

    pub fn default_bindings(self) -> Vec<Shortcut> {
        let none = |key| Shortcut::new(Mods::NONE, key);
        let cmd = |key| Shortcut::new(Mods::COMMAND, key);
        let cmd_shift = |key| Shortcut::new(Mods::COMMAND.with(Mods::SHIFT), key);
        match self {
            Action::PlayStop => vec![none(Key::Space)],
            Action::Loop => vec![cmd(Key::L)],
            Action::ToggleMetronome => vec![cmd(Key::M)],
            Action::ToggleFollowPlayhead => vec![cmd_shift(Key::F)],
            Action::Undo => vec![cmd(Key::Z)],
            Action::Redo => vec![cmd(Key::Y), cmd_shift(Key::Z)],
            Action::SelectAll => vec![cmd(Key::A)],
            Action::Duplicate => vec![cmd(Key::D)],
            Action::Rename => vec![cmd(Key::R)],
            Action::Delete => vec![none(Key::Delete), none(Key::Backspace)],
            Action::SplitAtCursor => vec![cmd(Key::K)],
            Action::AddTrack => vec![cmd(Key::T)],
            Action::GroupTracks => vec![cmd(Key::G)],
            Action::NudgeLeft => vec![none(Key::ArrowLeft)],
            Action::NudgeRight => vec![none(Key::ArrowRight)],
            Action::MoveTrackUp => vec![none(Key::ArrowUp)],
            Action::MoveTrackDown => vec![none(Key::ArrowDown)],
            Action::NarrowGrid => vec![cmd(Key::Num1)],
            Action::WidenGrid => vec![cmd(Key::Num2)],
            Action::ToggleTripletGrid => vec![cmd(Key::Num3)],
            Action::ToggleAdaptiveGrid => vec![cmd(Key::Num4)],
            Action::ToggleSnap => vec![cmd(Key::Num5)],
            Action::ToggleBrowser => vec![cmd(Key::B)],
            Action::NewProject => vec![cmd(Key::N)],
            Action::OpenProject => vec![cmd(Key::O)],
            Action::SaveProject => vec![cmd(Key::S)],
            Action::SaveProjectAs => vec![cmd_shift(Key::S)],
            Action::ExportAudio => vec![cmd_shift(Key::E)],
            Action::ToggleEffectsPanel => vec![cmd(Key::J)],
            Action::ToggleTrackList => vec![cmd_shift(Key::J)],
            Action::ToggleGraphView => vec![cmd_shift(Key::G)],
            Action::OpenSettings => vec![cmd(Key::Comma)],
        }
    }
}

/// The modifiers of a shortcut. `command` is Ctrl, or Cmd on macOS: both
/// are saved as `Ctrl`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mods {
    pub command: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        command: false,
        alt: false,
        shift: false,
    };
    pub const COMMAND: Mods = Mods {
        command: true,
        alt: false,
        shift: false,
    };
    pub const ALT: Mods = Mods {
        command: false,
        alt: true,
        shift: false,
    };
    pub const SHIFT: Mods = Mods {
        command: false,
        alt: false,
        shift: true,
    };

    /// Both held.
    pub const fn with(self, other: Mods) -> Mods {
        Mods {
            command: self.command || other.command,
            alt: self.alt || other.alt,
            shift: self.shift || other.shift,
        }
    }
}

/// A key pressed with exactly these modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shortcut {
    pub mods: Mods,
    pub key: Key,
}

impl Shortcut {
    pub const fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
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
    overrides: BTreeMap<Action, Vec<Shortcut>>,
}

impl Keymap {
    pub fn bindings(&self, action: Action) -> Vec<Shortcut> {
        self.overrides
            .get(&action)
            .cloned()
            .unwrap_or_else(|| action.default_bindings())
    }

    /// Bind `action` to `bindings`, replacing its current ones.
    pub fn set(&mut self, action: Action, bindings: Vec<Shortcut>) {
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
        shortcut: Shortcut,
    ) -> Option<Action> {
        let previous = self
            .action_for(shortcut.key, shortcut.mods)
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

    /// The action bound to `key` pressed with exactly `mods`.
    pub fn action_for(&self, key: Key, mods: Mods) -> Option<Action> {
        Action::ALL.into_iter().find(|action| {
            action.is_available()
                && self
                    .bindings(*action)
                    .iter()
                    .any(|shortcut| shortcut.key == key && shortcut.mods == mods)
        })
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

/// A shortcut the UI toolkit handles itself, shown but not rebindable.
pub struct FixedShortcut {
    pub label: &'static str,
    pub group: &'static str,
    pub shortcuts: &'static [Shortcut],
}

pub const FIXED_SHORTCUTS: [FixedShortcut; 6] = [
    FixedShortcut {
        label: "Copy",
        group: "Edit",
        shortcuts: &[Shortcut::new(Mods::COMMAND, Key::C)],
    },
    FixedShortcut {
        label: "Cut",
        group: "Edit",
        shortcuts: &[Shortcut::new(Mods::COMMAND, Key::X)],
    },
    FixedShortcut {
        label: "Paste",
        group: "Edit",
        shortcuts: &[Shortcut::new(Mods::COMMAND, Key::V)],
    },
    FixedShortcut {
        label: "Zoom in",
        group: "View",
        shortcuts: &[
            Shortcut::new(Mods::COMMAND, Key::Plus),
            Shortcut::new(Mods::COMMAND, Key::Equals),
        ],
    },
    FixedShortcut {
        label: "Zoom out",
        group: "View",
        shortcuts: &[Shortcut::new(Mods::COMMAND, Key::Minus)],
    },
    FixedShortcut {
        label: "Reset zoom",
        group: "View",
        shortcuts: &[Shortcut::new(Mods::COMMAND, Key::Num0)],
    },
];

/// The fixed shortcut `shortcut` would clash with.
pub fn reserved_by(shortcut: &Shortcut) -> Option<&'static FixedShortcut> {
    FIXED_SHORTCUTS
        .iter()
        .find(|fixed| fixed.shortcuts.contains(shortcut))
}

/// `Ctrl+Alt+Shift+Z`, as saved. `Ctrl` stands for Cmd on macOS.
pub fn format_shortcut(shortcut: &Shortcut) -> String {
    let Mods {
        command,
        alt,
        shift,
    } = shortcut.mods;
    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in [(command, "Ctrl"), (alt, "Alt"), (shift, "Shift")] {
        if held {
            parts.push(name);
        }
    }
    parts.push(shortcut.key.name());
    parts.join("+")
}

/// Inverse of [`format_shortcut`]; also accepts `Cmd` and `Option`, and
/// keys by their symbol (`Ctrl++`).
pub fn parse_shortcut(text: &str) -> Option<Shortcut> {
    // The key may itself be `+` (`Ctrl++`).
    let (modifiers, key) = match text.strip_suffix("++") {
        Some(modifiers) => (modifiers, "+"),
        None => text.rsplit_once('+').unwrap_or(("", text)),
    };
    let mut mods = Mods::NONE;
    for name in modifiers.split('+').filter(|name| !name.is_empty()) {
        mods = mods.with(match name.trim() {
            "Ctrl" | "Cmd" => Mods::COMMAND,
            "Shift" => Mods::SHIFT,
            "Alt" | "Option" => Mods::ALT,
            _ => return None,
        });
    }
    Some(Shortcut::new(mods, Key::from_name(key.trim())?))
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
        let plus = Shortcut::new(Mods::COMMAND, Key::Plus);
        assert_eq!(parse_shortcut("Ctrl++"), Some(plus));
        assert_eq!(parse_shortcut("Cmd+Plus"), Some(plus));
        assert_eq!(parse_shortcut("Hyper+A"), None);
        assert_eq!(parse_shortcut("Ctrl+NotAKey"), None);
    }

    /// The audio graph's shortcut works in debug builds only.
    #[test]
    fn graph_view_only_in_debug_builds() {
        let keymap = Keymap::default();
        let found = keymap.action_for(Key::G, Mods::COMMAND.with(Mods::SHIFT));
        if cfg!(debug_assertions) {
            assert_eq!(found, Some(Action::ToggleGraphView));
        } else {
            assert_eq!(found, None);
        }
        assert!(
            Action::ALL
                .into_iter()
                .all(|action| { action.is_available() || action == Action::ToggleGraphView })
        );
    }

    /// As egui wrote them, so saved bindings read the same.
    #[test]
    fn shortcuts_are_written_as_before() {
        let all = Mods::COMMAND.with(Mods::ALT).with(Mods::SHIFT);
        assert_eq!(
            format_shortcut(&Shortcut::new(all, Key::Z)),
            "Ctrl+Alt+Shift+Z"
        );
        assert_eq!(
            format_shortcut(&Shortcut::new(Mods::NONE, Key::Space)),
            "Space"
        );
        assert_eq!(
            format_shortcut(&Shortcut::new(Mods::COMMAND, Key::Num1)),
            "Ctrl+1"
        );
        assert_eq!(
            format_shortcut(&Shortcut::new(Mods::COMMAND, Key::Comma)),
            "Ctrl+Comma"
        );
        assert_eq!(
            format_shortcut(&Shortcut::new(Mods::NONE, Key::ArrowLeft)),
            "Left"
        );
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
                    keymap.action_for(shortcut.key, shortcut.mods),
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
        let cmd_shift = Mods::COMMAND.with(Mods::SHIFT);
        assert_eq!(keymap.action_for(Key::Z, cmd_shift), Some(Action::Redo));
        assert_eq!(keymap.action_for(Key::Z, Mods::COMMAND), Some(Action::Undo));
        assert_eq!(keymap.action_for(Key::ArrowLeft, Mods::SHIFT), None);
    }

    #[test]
    fn only_overrides_are_saved() {
        let mut keymap = Keymap::default();
        keymap.set(Action::Undo, vec![Shortcut::new(Mods::ALT, Key::U)]);
        keymap.set(Action::Redo, Action::Redo.default_bindings());
        let json = serde_json::to_string(&keymap).unwrap();
        assert_eq!(json, r#"{"undo":["Alt+U"]}"#);
        assert_eq!(serde_json::from_str::<Keymap>(&json).unwrap(), keymap);
    }

    #[test]
    fn assigning_takes_the_shortcut_from_other_actions() {
        let mut keymap = Keymap::default();
        let ctrl_d = Shortcut::new(Mods::COMMAND, Key::D);
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
    fn fixed_shortcuts_are_reserved_and_not_bound_by_default() {
        let keymap = Keymap::default();
        for fixed in &FIXED_SHORTCUTS {
            for shortcut in fixed.shortcuts {
                assert_eq!(keymap.action_for(shortcut.key, shortcut.mods), None);
                assert!(reserved_by(shortcut).is_some());
            }
        }
        assert!(reserved_by(&Shortcut::new(Mods::NONE, Key::C)).is_none());
    }

    #[test]
    fn bad_entries_are_skipped() {
        let keymap: Keymap =
            serde_json::from_str(r#"{"undo":["Alt+U","Bogus"],"not_an_action":["A"]}"#).unwrap();
        assert_eq!(
            keymap.bindings(Action::Undo),
            vec![Shortcut::new(Mods::ALT, Key::U)]
        );
        assert_eq!(keymap.overrides.len(), 1);
    }
}
