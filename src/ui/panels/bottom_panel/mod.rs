mod header;
#[cfg(test)]
mod tests;

use crate::ui::theme::font_size;
use crate::{
    config::keymap::Action,
    core::{state::ProjectState, track::TrackRow},
    ui::{
        clip_details::ClipDetails,
        commands::Commands,
        dnd::DragPayload,
        effects::{EffectEdit, EffectRack},
        theme::ThemeExt,
        workspace::{BottomView, Workspace},
    },
};
use egui::{
    Align, Align2, CursorIcon, DragAndDrop, FontId, Frame, Layout, Margin, Pos2, Rangef, Rect,
    ScrollArea, Stroke, Ui, vec2,
};
use tonique_engine::edit::TrackId;

/// Height of the header's row: the track's name, and tabs.
const HEADER_ROW: f32 = 14.;
/// Space each side of a tab's name.
const TAB_PADDING: f32 = 6.;
/// Rounding of a tab's top corners, and of the curves at its foot.
const TAB_RADIUS: f32 = 4.;
/// Between effects, and around them.
const GAP: f32 = 8.;

/// One of the shown effects being dragged to another place, by index.
#[derive(Clone, Copy)]
struct MovedEffect(usize);

pub struct BottomPanel {
    rack: EffectRack,
    clip_details: ClipDetails,
    /// Selected effects, by index, of `track`'s. Delete removes them.
    selected: Vec<usize>,
    track: Option<TrackId>,
    /// The last click was in the panel: Delete, Duplicate and Rename are
    /// for the effects, never for the timeline's selection.
    focused: bool,
    /// The effects' frames, as last drawn.
    #[cfg(test)]
    rects: Vec<Rect>,
    /// Some effect's editor drew past its frame, last drawn.
    #[cfg(test)]
    overflows: bool,
}

impl BottomPanel {
    pub fn new() -> Self {
        Self {
            rack: EffectRack::default(),
            clip_details: ClipDetails::new(),
            selected: vec![],
            track: None,
            focused: false,
            #[cfg(test)]
            rects: Vec::new(),
            #[cfg(test)]
            overflows: false,
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        state: &mut ProjectState,
        workspace: &mut Workspace,
        commands: &mut Commands,
    ) {
        let mut open = workspace.bottom_panel_open;
        egui::Panel::bottom("bottom-panel")
            .size_range(Rangef::new(160., 400.))
            .default_size(200.)
            .resizable(true)
            .frame(
                Frame::new()
                    .inner_margin(Margin::ZERO)
                    .fill(ui.app_theme().bg_panel),
            )
            .show_collapsible(ui, &mut open, |ui| {
                ui.set_height(ui.available_height());
                let panel = ui.max_rect();
                if let Some(pos) = ui.input(|i| {
                    i.pointer
                        .primary_pressed()
                        .then(|| i.pointer.interact_pos())
                        .flatten()
                }) {
                    self.focused = panel.contains(pos);
                }
                workspace.follow_clip_selection(state);
                let clip = match workspace.bottom_view {
                    BottomView::Clip(id) => state.clip_with_track(id),
                    BottomView::Effects => None,
                };
                if let Some((track, clip)) = clip {
                    self.track = None;
                    self.selected.clear();
                    let tabs = Some((BottomView::Clip(clip.id), clip.id));
                    if let Some(view) =
                        self.header(ui, &track, Some((&clip.audio.name, &clip.audio.path)), tabs)
                    {
                        workspace.bottom_view = view;
                    }
                    self.clip_details.ui(ui, &clip, &track, state);
                    return;
                }
                match state.selected_track() {
                    Some(track) => {
                        if let Some(view) = self.ui(ui, track, state) {
                            workspace.show_view(state, view);
                        }
                    }
                    None => {
                        self.track = None;
                        self.rack.retain(&[]);
                        hint(ui, panel, "Select a track to see its effects");
                    }
                }
            });
        workspace.bottom_panel_open = open;
        // The selection only holds while its effects are shown and the
        // panel is the last clicked.
        if !open {
            self.focused = false;
        }
        if !self.focused {
            self.selected.clear();
            return;
        }
        // Without effects shown (a clip is), they're for the timeline.
        let Some(track) = self.track else {
            return;
        };
        let delete = !commands.take(|a| a == Action::Delete).is_empty();
        let select_all = !commands.take(|a| a == Action::SelectAll).is_empty();
        let duplicate = !commands.take(|a| a == Action::Duplicate).is_empty();
        let rename = !commands.take(|a| a == Action::Rename).is_empty();
        if delete && !self.selected.is_empty() {
            state.remove_effects(&track, &self.selected);
            self.selected.clear();
        }
        // Typed in the header: expand it first.
        if rename
            && let Some(&index) = self.selected.first()
            && let Some(effect) = state.effects(&track).get(index)
        {
            if effect.collapsed {
                state.set_effect_collapsed(effect.plugin.id, false);
            }
            self.rack.rename(effect);
        }
        if duplicate && !self.selected.is_empty() {
            self.selected = state.duplicate_effects(&track, &self.selected);
        }
        if select_all {
            self.selected = (0..state.effects(&track).len()).collect();
        }
    }

    /// The effects of `track`. Returns the view picked in the header, if
    /// any.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        track: TrackRow,
        state: &mut ProjectState,
    ) -> Option<BottomView> {
        if self.track != Some(track.id) {
            self.selected.clear();
            self.track = Some(track.id);
        }
        let sample_rate = state.engine_config().sample_rate as f32;
        let effects = state.effects(&track.id);
        let shown: Vec<_> = effects.iter().map(|e| e.plugin.id).collect();
        self.rack.retain(&shown);

        // A tab to the clip selected on this track, if one is.
        let clip = match state.selected_clips() {
            [id] if state.find_clip(*id).is_some_and(|(t, _)| t == track.id) => Some(*id),
            _ => None,
        };
        let picked = self.header(ui, &track, None, clip.map(|c| (BottomView::Effects, c)));
        let area = ui.available_rect_before_wrap();

        // An effect dragged from the browser, or one of these being moved.
        let ctx = ui.ctx().clone();
        let added = DragAndDrop::payload::<DragPayload>(&ctx).and_then(|p| match *p {
            DragPayload::Effect(kind) => Some(kind),
            DragPayload::File(_) => None,
        });
        let moved = DragAndDrop::payload::<MovedEffect>(&ctx).map(|m| m.0);
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| area.contains(*p));
        let pressed = ui.input(|i| {
            i.pointer
                .primary_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });

        let mut rects = Vec::with_capacity(effects.len());
        #[cfg(test)]
        let mut overflows = false;
        let mut pressed_effect = None;
        let mut toggled = None;
        let mut removed = None;
        let mut duplicated = None;
        let mut collapse = Vec::new();
        let mut renamed = Vec::new();
        let mut edits = Vec::new();
        // Exact heights: sizing from what's left would let the effects
        // push the panel.
        let height = (area.height() - 2. * GAP).max(0.);
        ScrollArea::horizontal()
            .max_height(area.height())
            .show(ui, |ui| {
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), area.height()),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        ui.add_space(GAP);
                        for (i, effect) in effects.iter().enumerate() {
                            let selected = self.selected.contains(&i);
                            let response =
                                self.rack
                                    .effect_ui(ui, effect, height, sample_rate, selected);
                            response.header.dnd_set_drag_payload(MovedEffect(i));
                            response.header.on_hover_cursor(CursorIcon::Grab);
                            if pressed.is_some_and(|p| response.rect.contains(p)) {
                                pressed_effect = Some(i);
                            }
                            if response.toggled {
                                toggled = Some(effect);
                            }
                            if response.removed {
                                removed = Some(i);
                            }
                            if response.duplicated {
                                duplicated = Some(i);
                            }
                            if let Some(collapsed) = response.collapse {
                                collapse.push((effect.plugin.id, collapsed));
                            }
                            if let Some(name) = response.renamed {
                                renamed.push((effect.plugin.id, name));
                            }
                            if !response.edits.is_empty() {
                                edits.push((effect.plugin.id, response.edits));
                            }
                            #[cfg(test)]
                            {
                                overflows |= response.overflows;
                            }
                            rects.push(response.rect);
                        }
                        ui.add_space(GAP);
                    },
                );
            });
        #[cfg(test)]
        {
            self.rects.clone_from(&rects);
            self.overflows = overflows;
        }
        if effects.is_empty() {
            hint(ui, area, "Drop effects here from the browser");
        }

        // Where a dragged effect would go: before the effect under the
        // pointer, or after it past its middle; at the end elsewhere.
        let insert_index = pointer
            .filter(|_| added.is_some() || moved.is_some())
            .map(|p| {
                rects
                    .iter()
                    .position(|r| p.x < r.center().x)
                    .unwrap_or(rects.len())
            });
        if let Some(index) = insert_index
            && moved.is_none_or(|from| index != from && index != from + 1)
        {
            paint_insert_marker(ui, area, &rects, index);
        }
        if let Some(index) = insert_index
            && ui.input(|i| i.pointer.any_released())
        {
            if let Some(kind) = added {
                state.add_effect(&track.id, kind, index);
                self.selected = vec![index];
            } else if let Some(from) = moved {
                state.move_effect(&track.id, from, index);
                let to = if index > from { index - 1 } else { index };
                self.selected = vec![to];
            }
            DragAndDrop::clear_payload(&ctx);
            self.focused = true;
        }

        // Click an effect to select it, Ctrl+click to add it; elsewhere to
        // select none.
        if let Some(pos) = pressed
            && area.contains(pos)
        {
            match pressed_effect {
                Some(i) if ui.input(|i| i.modifiers.command) => {
                    if let Some(at) = self.selected.iter().position(|s| *s == i) {
                        self.selected.remove(at);
                    } else {
                        self.selected.push(i);
                    }
                }
                Some(i) if !self.selected.contains(&i) => self.selected = vec![i],
                Some(_) => {}
                None => self.selected.clear(),
            }
        }

        if let Some(effect) = toggled {
            state.set_effect_enabled(&track.id, effect.plugin.id, !effect.enabled());
        }
        let params: Vec<_> = edits
            .iter()
            .flat_map(|(_, edits)| edits)
            .filter_map(|edit| match *edit {
                EffectEdit::Param { id, old, new } => Some((id, old, new)),
                EffectEdit::Setting(_) => None,
            })
            .collect();
        if !params.is_empty() {
            state.commit_params(&params);
        }
        for (plugin, edits) in &edits {
            for edit in edits {
                if let EffectEdit::Setting(setting) = *edit {
                    state.set_effect_setting(&track.id, *plugin, setting);
                }
            }
        }
        for (plugin, collapsed) in collapse {
            state.set_effect_collapsed(plugin, collapsed);
        }
        for (plugin, name) in renamed {
            state.rename_effect(&track.id, plugin, name);
        }
        if let Some(index) = duplicated {
            self.selected = state.duplicate_effects(&track.id, &[index]);
        }
        if let Some(index) = removed {
            state.remove_effects(&track.id, &[index]);
            self.selected.clear();
        }
        picked
    }
}

/// Muted text in the middle of `rect`.
fn hint(ui: &Ui, rect: Rect, text: &str) {
    let theme = ui.app_theme();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(font_size::LABEL),
        theme.text_muted,
    );
}

/// Line in the gap where a dragged effect would go.
fn paint_insert_marker(ui: &Ui, area: Rect, rects: &[Rect], index: usize) {
    let x = match (rects.get(index), rects.last()) {
        (Some(next), _) => next.left() - GAP / 2.,
        (None, Some(last)) => last.right() + GAP / 2.,
        (None, None) => area.left() + GAP / 2.,
    };
    let y = area.y_range().shrink(GAP);
    ui.painter().line_segment(
        [Pos2::new(x, y.min), Pos2::new(x, y.max)],
        Stroke::new(2., ui.app_theme().accent),
    );
}
