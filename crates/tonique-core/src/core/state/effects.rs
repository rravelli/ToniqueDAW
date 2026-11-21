//! Each channel's effects.

use crate::core::effect::{Effect, EffectKind, Setting};

use tonique_engine::{
    edit::{
        Plugin, PluginId, TrackId,
        commands::{AddPlugin, RemovePlugin, RenamePlugin, SetBypass, SetParam},
    },
    param::ParamId,
};

use super::ProjectState;

impl ProjectState {
    /// Add an effect to a channel, at `index` among its effects; past the
    /// last appends it.
    pub fn add_effect(&mut self, id: &TrackId, kind: EffectKind, index: usize) {
        self.add_effect_with(id, kind, None, index);
    }
    /// Add an effect made with `setting` rather than its default one.
    pub fn add_effect_with(
        &mut self,
        id: &TrackId,
        kind: EffectKind,
        setting: Option<Setting>,
        index: usize,
    ) {
        let plugin_kind = setting.map_or(kind.plugin_kind(), |s| s.apply(kind.plugin_kind()));
        let plugin = self.session.create(|e| Plugin::new(e, plugin_kind));
        for (name, value) in kind.initial_params() {
            if let Some(param) = plugin.param(name) {
                param.set(*value);
            }
        }
        let position = self.effect_position(id, index);
        self.perform(AddPlugin::at(self.channel_ref(*id), plugin, position));
    }
    /// Change an effect's [`Setting`], as an undo step. The engine fixes it
    /// when it makes the plugin, so this swaps in a plugin made with the new
    /// setting; it keeps the ID and the parameters.
    pub fn set_effect_setting(&mut self, id: &TrackId, plugin: PluginId, setting: Setting) {
        let Some(old) = self.effect(id, plugin) else {
            return;
        };
        let kind = setting.apply(old.plugin.kind);
        if kind == old.plugin.kind {
            return;
        }
        let Some(position) = self.plugin_position(id, plugin) else {
            return;
        };
        let new = Plugin { kind, ..old.plugin };
        self.transaction("Change effect", |s| {
            let channel = s.channel_ref(*id);
            s.perform(RemovePlugin::new(channel, plugin));
            s.perform(AddPlugin::at(channel, new, position));
        });
    }
    /// Move the effect at `from` to before the one at `to` (the end past
    /// the last), as an undo step. Indexes as in [`Self::effects`].
    pub fn move_effect(&mut self, id: &TrackId, from: usize, to: usize) {
        let effects = self.effects(id);
        let Some(effect) = effects.get(from) else {
            return;
        };
        if to == from || to == from + 1 {
            return;
        }
        let Some(source) = self.plugin_position(id, effect.plugin.id) else {
            return;
        };
        let mut target = self.effect_position(id, to);
        if target != usize::MAX && target > source {
            // Shifted down by the removal.
            target -= 1;
        }
        let plugin = effect.plugin.clone();
        self.transaction("Move effect", |s| {
            let channel = s.channel_ref(*id);
            s.perform(RemovePlugin::new(channel, plugin.id));
            s.perform(AddPlugin::at(channel, plugin, target));
        });
    }
    /// Move an effect to the end of another channel's chain, as an undo
    /// step. It stays the same effect: settings, name and all.
    pub fn move_effect_to_track(&mut self, from: &TrackId, plugin: PluginId, to: &TrackId) {
        if from == to {
            return;
        }
        let Some(effect) = self.effect(from, plugin) else {
            return;
        };
        self.transaction("Move effect", |s| {
            s.perform(RemovePlugin::new(s.channel_ref(*from), plugin));
            s.perform(AddPlugin::at(s.channel_ref(*to), effect.plugin, usize::MAX));
        });
    }
    /// Where the effect at `index` is among all of a channel's plugins,
    /// past the plugins that aren't effects; `usize::MAX` for the end.
    fn effect_position(&self, id: &TrackId, index: usize) -> usize {
        self.effects(id)
            .get(index)
            .and_then(|e| self.plugin_position(id, e.plugin.id))
            .unwrap_or(usize::MAX)
    }
    fn effect(&self, id: &TrackId, plugin: PluginId) -> Option<Effect> {
        self.effects(id).into_iter().find(|e| e.plugin.id == plugin)
    }
    /// Set parameters, as one undo step: `(param, old, new)`, `old` being
    /// its value before the user started changing it.
    pub fn commit_params(&mut self, changes: &[(ParamId, f32, f32)]) {
        self.transaction("Change parameter", |s| {
            for &(param, old, new) in changes {
                let Ok(p) = s.edit().param(param) else {
                    continue;
                };
                // `SetParam` records the current value for undo: restore it
                // first.
                p.set(old);
                s.perform(SetParam::new(param, new));
            }
        });
    }
    /// Copy the effects at `indexes` (see [`Self::effects`]), each right
    /// after its original, as one undo step. Returns the copies' indexes.
    pub fn duplicate_effects(&mut self, id: &TrackId, indexes: &[usize]) -> Vec<usize> {
        let effects = self.effects(id);
        let mut indexes: Vec<_> = indexes
            .iter()
            .copied()
            .filter(|i| *i < effects.len())
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        let mut copies = Vec::with_capacity(indexes.len());
        self.transaction("Duplicate effects", |s| {
            for (shift, index) in indexes.into_iter().enumerate() {
                let original = &effects[index].plugin;
                let mut copy = s.session.create(|e| Plugin::new(e, original.kind));
                copy.bypassed = original.bypassed;
                copy.name.clone_from(&original.name);
                if effects[index].collapsed {
                    s.collapsed_effects.insert(copy.id);
                }
                for param in &mut copy.params {
                    if let Some(from) = original.param(param.name) {
                        param.set(from.get());
                        param.automation.clone_from(&from.automation);
                    }
                }
                // Past the copies made so far.
                let at = index + shift + 1;
                let position = s.effect_position(id, at);
                s.perform(AddPlugin::at(s.channel_ref(*id), copy, position));
                copies.push(at);
            }
        });
        copies
    }
    /// Remove the effects at `indexes` (see [`Self::effects`]), as one undo
    /// step.
    pub fn remove_effects(&mut self, id: &TrackId, indexes: &[usize]) {
        let effects = self.effects(id);
        let plugins: Vec<_> = indexes
            .iter()
            .filter_map(|i| effects.get(*i).map(|e| e.plugin.id))
            .collect();
        self.transaction("Remove effects", |s| {
            for plugin in plugins {
                s.perform(RemovePlugin::new(s.channel_ref(*id), plugin));
            }
        });
    }
    /// The effects on a channel, in processing order.
    pub fn effects(&self, id: &TrackId) -> Vec<Effect> {
        self.edit()
            .channel(self.channel_ref(*id))
            .map(|c| {
                c.plugins
                    .iter()
                    .filter_map(|plugin| {
                        Some(Effect {
                            kind: EffectKind::of(&plugin.kind)?,
                            plugin: plugin.clone(),
                            collapsed: self.collapsed_effects.contains(&plugin.id),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    /// Name an effect, as an undo step. Blank (or `None`) goes back to its
    /// kind's name.
    pub fn rename_effect(&mut self, id: &TrackId, plugin: PluginId, name: Option<String>) {
        let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        let Some(effect) = self.effect(id, plugin) else {
            return;
        };
        if effect.plugin.name == name {
            return;
        }
        self.perform(RenamePlugin::new(self.channel_ref(*id), plugin, name));
    }
    /// Fold an effect to a strip, or unfold it.
    pub fn set_effect_collapsed(&mut self, plugin: PluginId, collapsed: bool) {
        if collapsed {
            self.collapsed_effects.insert(plugin);
        } else {
            self.collapsed_effects.remove(&plugin);
        }
    }
    /// Power button: bypass the effect or not, as an undo step.
    pub fn set_effect_enabled(&mut self, id: &TrackId, plugin: PluginId, enabled: bool) {
        self.perform(SetBypass::new(self.channel_ref(*id), plugin, !enabled));
    }
    fn plugin_position(&self, id: &TrackId, plugin: PluginId) -> Option<usize> {
        self.edit()
            .channel(self.channel_ref(*id))
            .ok()?
            .plugins
            .iter()
            .position(|p| p.id == plugin)
    }
}
