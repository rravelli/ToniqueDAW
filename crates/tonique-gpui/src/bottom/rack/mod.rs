//! The selected track's effects, as the egui app's `EffectRack`: side by
//! side, scrolled across, each in a frame ([`frame`]) with its editor
//! ([`editors`]) and its output's meter. Effects are dragged in from the
//! browser and moved by their header; clicked to select them (Ctrl+click
//! adds one), and then deleted, duplicated or renamed.

mod editors;
mod filter;
mod frame;
mod graph;
mod knob;

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, DragMoveEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Window, canvas, div, prelude::*, px,
};
use tonique_engine::edit::{PluginId, TrackId};

use crate::{
    app::Tonique,
    bottom::{hint, paint::fill},
    config::keymap::Action,
    core::{effect::EffectKind, metrics::AudioMetrics, track::TrackRow},
    geometry::Rect,
    widgets::TextField,
};
use editors::{Editor, EditorInput};
use filter::GraphDrag;
use knob::KnobDrag;

/// Between effects, and around them.
const GAP: f32 = 8.;
/// A header dragged this far moves its effect.
const MOVE_THRESHOLD: f32 = 3.;

/// An effect dragged from the browser.
#[derive(Clone, Copy)]
pub struct DraggedEffect {
    pub kind: EffectKind,
}

/// An effect's editor, and the levels its meter shows.
struct Slot {
    editor: Rc<RefCell<Editor>>,
    /// What comes out of the effect.
    levels: AudioMetrics,
}

enum RackDrag {
    Knob(KnobDrag),
    Graph(GraphDrag),
    /// An effect by its header: moved once dragged a little.
    Move {
        plugin: PluginId,
        from: usize,
        start_x: f32,
        /// Where the pointer was last, as a release doesn't tell.
        last_x: f32,
        moved: bool,
    },
}

#[derive(Default)]
pub struct Rack {
    /// The editors of the effects shown, made when first shown.
    slots: HashMap<PluginId, Slot>,
    /// Selected effects, by index, of `track`'s.
    selected: Vec<usize>,
    track: Option<TrackId>,
    /// An effect being renamed, and its name so far.
    renaming: Option<(TrackId, PluginId, TextField)>,
    drag: Option<RackDrag>,
    /// Where an effect dragged from the browser would go.
    insert: Option<usize>,
    /// The effects' frames, as last laid out.
    frames: Rc<RefCell<Vec<Bounds<Pixels>>>>,
    /// A time knob's value while dragged, not applied yet.
    pending: Option<(PluginId, f32)>,
    /// Something shown moves on its own.
    animating: bool,
}

impl Rack {
    pub fn forget(&mut self) {
        self.leave_track();
        self.renaming = None;
        self.drag = None;
    }

    /// Effects aren't shown: nothing of them is kept.
    pub fn leave_track(&mut self) {
        self.track = None;
        self.selected.clear();
        // An analyser's buffers are large.
        self.slots.clear();
        self.animating = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub fn is_animating(&self) -> bool {
        self.animating
    }

    /// Where an effect dropped at `pointer_x` would go: before the effect
    /// under the pointer, or after it past its middle; at the end elsewhere.
    fn insert_index(&self, pointer_x: f32) -> usize {
        let frames = self.frames.borrow();
        frames
            .iter()
            .position(|bounds| pointer_x < f32::from(bounds.center().x))
            .unwrap_or(frames.len())
    }
}

/// `Hz` below a kilohertz, `kHz` above.
pub fn format_hz(hz: f32) -> String {
    if hz < 1000. {
        format!("{hz:.0} Hz")
    } else if hz < 10_000. {
        format!("{:.1} kHz", hz / 1000.)
    } else {
        format!("{:.0} kHz", hz / 1000.)
    }
}

pub fn format_percent(ratio: f32) -> String {
    format!("{:.0}%", ratio * 100.)
}

pub fn format_ms(seconds: f32) -> String {
    format!("{:.0} ms", seconds * 1000.)
}

impl Tonique {
    pub(super) fn rack_view(
        &mut self,
        track: &TrackRow,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let rack = &mut self.bottom.rack;
        if rack.track != Some(track.id) {
            rack.selected.clear();
            rack.track = Some(track.id);
        }
        if !context.has_active_drag() {
            rack.insert = None;
        }
        let sample_rate = self.state.engine_config().sample_rate as f32;
        let effects = self.state.effects(&track.id);
        let rack = &mut self.bottom.rack;
        rack.slots
            .retain(|id, _| effects.iter().any(|effect| effect.plugin.id == *id));
        rack.selected.retain(|index| *index < effects.len());
        rack.frames
            .borrow_mut()
            .resize(effects.len(), Bounds::default());

        // Their outputs, while shown: a bypassed effect isn't heard.
        let mut animating = false;
        for effect in &effects {
            let slot = rack.slots.entry(effect.plugin.id).or_insert_with(|| Slot {
                editor: Rc::new(RefCell::new(Editor::new(effect.kind))),
                levels: AudioMetrics::new(),
            });
            if effect.collapsed || !effect.enabled() {
                continue;
            }
            let output = &effect.plugin.tap;
            output.watch_output_for((sample_rate / 2.) as u32);
            slot.levels.update(&output.output, false);
            let mut editor = slot.editor.borrow_mut();
            editor.update(effect, sample_rate);
            animating |= editor.is_animated() || crate::meter::is_live(&slot.levels);
        }
        rack.animating = animating;

        let theme = self.theme.clone();
        let mut frames = Vec::with_capacity(effects.len());
        for (index, effect) in effects.iter().enumerate() {
            let selected = self.bottom.rack.selected.contains(&index);
            let Some(slot) = self.bottom.rack.slots.get(&effect.plugin.id) else {
                continue;
            };
            let frame = if effect.collapsed {
                self.effect_strip(index, effect, track.id, selected, &theme, context)
            } else {
                let input = EditorInput {
                    effect,
                    track: track.id,
                    sample_rate,
                    editor: slot.editor.clone(),
                    pending: self
                        .bottom
                        .rack
                        .pending
                        .filter(|(plugin, _)| *plugin == effect.plugin.id)
                        .map(|(_, value)| value),
                };
                let levels = slot.levels.clone();
                self.effect_frame(index, &input, levels, selected, &theme, context)
            };
            frames.push(frame);
        }

        // Where a dragged effect would go.
        let pointer_x = f32::from(window.mouse_position().x);
        let marker = match &self.bottom.rack.drag {
            Some(RackDrag::Move {
                from, moved: true, ..
            }) => {
                let index = self.bottom.rack.insert_index(pointer_x);
                (index != *from && index != from + 1).then_some(index)
            }
            _ => self.bottom.rack.insert,
        };
        let frame_bounds = self.bottom.rack.frames.clone();
        let accent = theme.accent;
        let marker_canvas = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let Some(index) = marker else {
                    return;
                };
                let frames = frame_bounds.borrow();
                let area = Rect::of(bounds);
                let x = match (frames.get(index), frames.last()) {
                    (Some(next), _) => f32::from(next.left()) - GAP / 2.,
                    (None, Some(last)) => f32::from(last.right()) + GAP / 2.,
                    (None, None) => area.left + GAP / 2.,
                };
                fill(
                    window,
                    Rect::new(x - 1., area.top + GAP, 2., area.height - 2. * GAP),
                    accent,
                );
            },
        )
        // Over the row, not after it.
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let row = div()
            .id("rack-row")
            .size_full()
            .overflow_x_scroll()
            .flex()
            .gap(px(GAP))
            .p(px(GAP))
            .children(frames);
        let track_id = track.id;
        div()
            .id("rack")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(row)
            .when(effects.is_empty(), |element| {
                element.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(hint("Drop effects here from the browser", &theme)),
                )
            })
            .child(marker_canvas)
            .capture_any_mouse_down(context.listener(Self::rack_press))
            .on_drag_move(context.listener(
                |this, event: &DragMoveEvent<DraggedEffect>, _, context| {
                    let over = event.bounds.contains(&event.event.position);
                    let index = this
                        .bottom
                        .rack
                        .insert_index(f32::from(event.event.position.x));
                    this.bottom.rack.insert = over.then_some(index);
                    context.notify();
                },
            ))
            .on_drop(
                context.listener(move |this, dragged: &DraggedEffect, window, context| {
                    let index = this
                        .bottom
                        .rack
                        .insert_index(f32::from(window.mouse_position().x));
                    this.state.add_effect(&track_id, dragged.kind, index);
                    let rack = &mut this.bottom.rack;
                    rack.insert = None;
                    rack.selected = vec![index];
                    this.bottom.focused = true;
                    context.notify();
                }),
            )
            .into_any_element()
    }

    /// Click an effect to select it, Ctrl+click to add it; elsewhere to
    /// select none.
    fn rack_press(&mut self, event: &MouseDownEvent, _: &mut Window, _: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        let rack = &mut self.bottom.rack;
        let hit = rack
            .frames
            .borrow()
            .iter()
            .position(|bounds| bounds.contains(&event.position));
        let command = event.modifiers.control || event.modifiers.platform;
        match hit {
            Some(index) if command => {
                if let Some(at) = rack.selected.iter().position(|chosen| *chosen == index) {
                    rack.selected.remove(at);
                } else {
                    rack.selected.push(index);
                }
            }
            Some(index) if !rack.selected.contains(&index) => rack.selected = vec![index],
            Some(_) => {}
            None => rack.selected.clear(),
        }
    }

    pub(super) fn rack_action(
        &mut self,
        track: TrackId,
        action: Action,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let selected = self.bottom.rack.selected.clone();
        match action {
            Action::Delete if !selected.is_empty() => {
                self.state.remove_effects(&track, &selected);
                self.bottom.rack.selected.clear();
            }
            Action::Duplicate if !selected.is_empty() => {
                self.bottom.rack.selected = self.state.duplicate_effects(&track, &selected);
            }
            Action::SelectAll => {
                self.bottom.rack.selected = (0..self.state.effects(&track).len()).collect();
            }
            Action::Rename => {
                if let Some(&index) = selected.first()
                    && let Some(effect) = self.state.effects(&track).get(index)
                {
                    let plugin = effect.plugin.id;
                    self.rename_effect(track, plugin, window, context);
                }
            }
            _ => {}
        }
    }

    pub(super) fn rack_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        let (pointer_x, pointer_y) = (f32::from(event.position.x), f32::from(event.position.y));
        let fine = event.modifiers.shift;
        let mut knob = None;
        match self.bottom.rack.drag.as_mut() {
            None => return false,
            Some(RackDrag::Knob(drag)) => {
                if let Some(value) = drag.follow(pointer_y, fine) {
                    knob = Some((drag.target.clone(), drag.before, value));
                }
            }
            Some(RackDrag::Graph(drag)) => Self::follow_graph(drag, pointer_x, pointer_y),
            Some(RackDrag::Move {
                start_x,
                last_x,
                moved,
                ..
            }) => {
                *last_x = pointer_x;
                *moved |= (pointer_x - *start_x).abs() > MOVE_THRESHOLD;
            }
        }
        if let Some((target, before, value)) = knob {
            self.set_knob(&target, before, value, false);
        }
        true
    }

    pub(super) fn rack_drag_end(&mut self) {
        let Some(drag) = self.bottom.rack.drag.take() else {
            return;
        };
        match drag {
            RackDrag::Knob(drag) => self.set_knob(&drag.target, drag.before, drag.value, true),
            RackDrag::Graph(drag) => {
                let (cutoff, q) = (drag.cutoff.get(), drag.q.get());
                if (cutoff, q) != drag.before {
                    self.state.commit_params(&[
                        (drag.cutoff.id, drag.before.0, cutoff),
                        (drag.q.id, drag.before.1, q),
                    ]);
                }
            }
            RackDrag::Move {
                plugin,
                from,
                last_x,
                moved: true,
                ..
            } => {
                let Some(track) = self.bottom.rack.track else {
                    return;
                };
                // Still where it was picked up: nothing else moved it.
                let effects = self.state.effects(&track);
                if effects.get(from).map(|effect| effect.plugin.id) != Some(plugin) {
                    return;
                }
                let index = self.bottom.rack.insert_index(last_x);
                if index != from && index != from + 1 {
                    self.state.move_effect(&track, from, index);
                    let to = if index > from { index - 1 } else { index };
                    self.bottom.rack.selected = vec![to];
                }
            }
            RackDrag::Move { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_frequencies() {
        assert_eq!(format_hz(440.), "440 Hz");
        assert_eq!(format_hz(1500.), "1.5 kHz");
        assert_eq!(format_hz(12_000.), "12 kHz");
    }

    #[test]
    fn effects_drop_by_the_middles_of_the_frames() {
        let rack = Rack::default();
        let frame =
            |left: f32| Bounds::new(gpui::point(px(left), px(0.)), gpui::size(px(100.), px(50.)));
        *rack.frames.borrow_mut() = vec![frame(0.), frame(110.)];
        assert_eq!(rack.insert_index(20.), 0);
        assert_eq!(rack.insert_index(60.), 1);
        assert_eq!(rack.insert_index(300.), 2);
    }
}
