use crate::{
    config::keymap::Action,
    core::{metrics::AudioMetrics, state::ProjectState, track::TrackRow},
    ui::{
        commands::Commands,
        dnd::DragPayload,
        effects::{EffectEdit, EffectRack},
        theme::ThemeExt,
        workspace::Workspace,
    },
    utils::display_name,
};
use egui::{
    Align, Align2, CursorIcon, DragAndDrop, FontId, Frame, Layout, Margin, Pos2, Rangef, Rect,
    RichText, ScrollArea, Stroke, Ui, vec2,
};
use tonique_engine::edit::TrackId;

/// Between effects, and around them.
const GAP: f32 = 8.;

/// One of the shown effects being dragged to another place, by index.
#[derive(Clone, Copy)]
struct MovedEffect(usize);

pub struct BottomPanel {
    rack: EffectRack,
    /// Selected effects, by index, of `track`'s. Delete removes them.
    selected: Vec<usize>,
    track: Option<TrackId>,
    /// The last click was in the panel: Delete and Duplicate are for the
    /// effects, never for the timeline's selection.
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
            .frame(Frame::new().inner_margin(Margin::ZERO))
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
                match state.selected_track() {
                    Some(track) => self.ui(ui, track, state),
                    None => {
                        self.track = None;
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
        let delete = !commands.take(|a| a == Action::Delete).is_empty();
        let select_all = !commands.take(|a| a == Action::SelectAll).is_empty();
        let duplicate = !commands.take(|a| a == Action::Duplicate).is_empty();
        let Some(track) = self.track else {
            return;
        };
        if delete && !self.selected.is_empty() {
            state.remove_effects(&track, &self.selected);
            self.selected.clear();
        }
        if duplicate && !self.selected.is_empty() {
            self.selected = state.duplicate_effects(&track, &self.selected);
        }
        if select_all {
            self.selected = (0..state.effects(&track).len()).collect();
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, track: TrackRow, state: &mut ProjectState) {
        if self.track != Some(track.id) {
            self.selected.clear();
            self.track = Some(track.id);
        }
        let metrics = state
            .metrics
            .tracks
            .get(&track.id)
            .cloned()
            .unwrap_or_else(AudioMetrics::new);
        let sample_rate = state.engine_config().sample_rate as f32;
        let effects = state.effects(&track.id);

        self.header(ui, &track);
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
                            let response = self.rack.effect_ui(
                                ui,
                                effect,
                                height,
                                sample_rate,
                                &metrics,
                                selected,
                            );
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
        if let Some(index) = removed {
            state.remove_effects(&track.id, &[index]);
            self.selected.clear();
        }
    }

    fn header(&mut self, ui: &mut Ui, track: &TrackRow) {
        let theme = ui.app_theme();
        Frame::new()
            .fill(track.color)
            .stroke(Stroke::new(1.0, theme.border))
            .inner_margin(Margin {
                bottom: 1,
                top: 1,
                left: 5,
                right: 5,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(display_name(&track.name, track.first_track_index))
                            .size(10.)
                            .color(theme.text_on(track.color)),
                    );
                });
            });
    }
}

/// Muted text in the middle of `rect`.
fn hint(ui: &Ui, rect: Rect, text: &str) {
    let theme = ui.app_theme();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(11.),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::effect::EffectKind;
    use egui::{Rect, vec2};
    use tonique_engine::{
        edit::PluginKind,
        engine::{Engine, EngineConfig},
    };

    /// The selected track's effects draw, headless, and their editors don't
    /// touch the parameters by just being shown.
    #[test]
    fn draws_the_selected_tracks_effects_headless() {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let track = state.add_track();
        state.add_effect(&track, EffectKind::Filter, 0);
        state.add_effect(&track, EffectKind::Echo, 1);
        let cutoff = state.effects(&track)[0]
            .plugin
            .param("cutoff")
            .unwrap()
            .clone();
        cutoff.set(440.);
        state.select_track(&track);
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };

        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        let mut panel = BottomPanel::new();
        let mut shapes = 0;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, &mut state, &mut workspace, &mut Commands::default())
            });
            output.textures_delta.clear();
            shapes = output.shapes.len();
        }
        assert!(shapes > 0);
        assert_eq!(panel.rack.len(), 2, "an editor per effect");
        assert_eq!(cutoff.get(), 440.);
    }

    /// Run frames of the panel, the first with `events` (after a frame to
    /// lay it out); returns the actions it left for the timeline and app.
    fn run(
        ctx: &egui::Context,
        panel: &mut BottomPanel,
        state: &mut ProjectState,
        events: Vec<egui::Event>,
        actions: &[Action],
    ) -> Vec<Action> {
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };
        let mut left = Vec::new();
        for (events, actions) in [(vec![], &[][..]), (events, actions)] {
            let mut commands = Commands::default();
            for action in actions {
                commands.push(*action);
            }
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, state, &mut workspace, &mut commands)
            });
            output.textures_delta.clear();
            left = commands.take_all();
        }
        left
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    fn setup() -> (egui::Context, BottomPanel, ProjectState, TrackId) {
        let (engine, _processor) = Engine::new(EngineConfig::default());
        let mut state = ProjectState::new(engine);
        let track = state.add_track();
        state.add_effect(&track, EffectKind::Filter, 0);
        state.add_effect(&track, EffectKind::Echo, 1);
        state.select_track(&track);
        let ctx = egui::Context::default();
        ctx.set_fonts(crate::ui::font::fonts());
        (ctx, BottomPanel::new(), state, track)
    }

    /// Delete removes the selected effects; it's left for the timeline
    /// only when the panel wasn't the last clicked.
    #[test]
    fn delete_goes_to_selected_effects() {
        let (ctx, mut panel, mut state, track) = setup();
        let left = run(&ctx, &mut panel, &mut state, vec![], &[Action::Delete]);
        assert_eq!(left, [Action::Delete]);
        assert_eq!(state.effects(&track).len(), 2);

        panel.focused = true;
        panel.selected = vec![0];
        let left = run(&ctx, &mut panel, &mut state, vec![], &[Action::Delete]);
        assert!(left.is_empty());
        assert_eq!(state.effects(&track).len(), 1);
        assert_eq!(state.effects(&track)[0].kind, EffectKind::Echo);
        assert!(panel.selected.is_empty());
    }

    /// Ctrl+D copies the selected effects rather than the track, and
    /// selects the copies.
    #[test]
    fn duplicate_goes_to_selected_effects() {
        let (ctx, mut panel, mut state, track) = setup();
        let tracks = state.track_count();
        run(&ctx, &mut panel, &mut state, vec![], &[]);
        panel.focused = true;
        panel.selected = vec![0];
        let left = run(&ctx, &mut panel, &mut state, vec![], &[Action::Duplicate]);
        assert!(left.is_empty());
        assert_eq!(state.track_count(), tracks);
        let kinds: Vec<_> = state.effects(&track).iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EffectKind::Filter, EffectKind::Filter, EffectKind::Echo]
        );
        assert_eq!(panel.selected, [1]);

        // Nothing selected: still not the track's.
        panel.selected.clear();
        let left = run(&ctx, &mut panel, &mut state, vec![], &[Action::Duplicate]);
        assert!(left.is_empty());
        assert_eq!(state.track_count(), tracks);
        assert_eq!(state.effects(&track).len(), 3);
    }

    /// Each effect's controls are its own: dragging in one (here a copy's
    /// twin) leaves the others alone.
    #[test]
    fn effects_controls_dont_drive_each_other() {
        let (ctx, mut panel, mut state, track) = setup();
        state.duplicate_effects(&track, &[0]);
        let cutoffs = |state: &ProjectState| -> Vec<f32> {
            state.effects(&track)[..2]
                .iter()
                .map(|e| e.plugin.param("cutoff").unwrap().get())
                .collect()
        };
        assert_eq!(cutoffs(&state), [1300., 1300.]);
        run(&ctx, &mut panel, &mut state, vec![], &[]);

        // Drag right in the first filter's graph.
        let (from, to) = (egui::pos2(100., 690.), egui::pos2(160., 690.));
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };
        for events in [
            vec![egui::Event::PointerMoved(from), button(from, true)],
            vec![egui::Event::PointerMoved(from + vec2(10., 0.))],
            vec![egui::Event::PointerMoved(to)],
            vec![button(to, false)],
        ] {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, &mut state, &mut workspace, &mut Commands::default())
            });
            output.textures_delta.clear();
        }
        let [first, second] = cutoffs(&state)[..] else {
            panic!("two filters");
        };
        assert_ne!(first, 1300., "the dragged filter changed");
        assert_eq!(second, 1300., "its copy didn't");
    }

    /// The echo's time, set on release, is its own too, while dragged and
    /// after.
    #[test]
    fn echo_times_dont_drive_each_other() {
        let (ctx, mut panel, mut state, track) = setup();
        state.duplicate_effects(&track, &[1]);
        let times = |state: &ProjectState| -> Vec<PluginKind> {
            state.effects(&track)[1..]
                .iter()
                .map(|e| e.plugin.kind)
                .collect()
        };
        let before = times(&state);
        run(&ctx, &mut panel, &mut state, vec![], &[]);

        // Drag the first echo's time knob up.
        let knob = panel.rects[1].left_bottom() + vec2(6. + 23., -6. - 26.);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut workspace = Workspace {
            bottom_panel_open: true,
            ..Default::default()
        };
        let mut shown = Vec::new();
        for (i, events) in [
            vec![egui::Event::PointerMoved(knob), button(knob, true)],
            vec![egui::Event::PointerMoved(knob - vec2(0., 10.))],
            vec![egui::Event::PointerMoved(knob - vec2(0., 60.))],
            vec![egui::Event::PointerMoved(knob - vec2(0., 60.))],
            vec![button(knob - vec2(0., 60.), false)],
        ]
        .into_iter()
        .enumerate()
        {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                panel.show(ui, &mut state, &mut workspace, &mut Commands::default())
            });
            output.textures_delta.clear();
            if i == 3 {
                shown = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text().ends_with(" ms") => {
                            Some(t.galley.text().to_string())
                        }
                        _ => None,
                    })
                    .collect();
            }
        }
        let after = times(&state);
        assert_ne!(after[0], before[0], "the dragged echo changed");
        assert_eq!(after[1], before[1], "its copy didn't");
        let knobs: Vec<_> = shown.iter().filter(|t| !t.contains("per")).collect();
        assert_eq!(knobs.len(), 2, "{shown:?}");
        assert_ne!(
            knobs[0], knobs[1],
            "while dragged, only it shows the new time"
        );
    }

    /// The bug: clicking an effect's controls didn't select it, so Delete
    /// went on to the timeline and deleted the track.
    #[test]
    fn delete_after_clicking_an_effect_never_deletes_the_track() {
        let (ctx, mut panel, mut state, track) = setup();
        let tracks = state.track_count();
        // In the first effect's graph, below the panel's and its header.
        let left = run(
            &ctx,
            &mut panel,
            &mut state,
            click(egui::pos2(60., 700.)),
            &[Action::Delete],
        );
        assert!(left.is_empty(), "the panel took Delete: {left:?}");
        assert_eq!(state.track_count(), tracks);
        assert_eq!(state.effects(&track).len(), 1, "the clicked effect");

        // On the panel's background: nothing selected, still not the track.
        let left = run(
            &ctx,
            &mut panel,
            &mut state,
            click(egui::pos2(1100., 700.)),
            &[Action::Delete],
        );
        assert!(left.is_empty());
        assert_eq!(state.track_count(), tracks);
        assert_eq!(state.effects(&track).len(), 1);

        // Clicking elsewhere gives Delete back to the timeline.
        let left = run(
            &ctx,
            &mut panel,
            &mut state,
            click(egui::pos2(600., 100.)),
            &[Action::Delete],
        );
        assert_eq!(left, [Action::Delete]);
    }

    /// Effects are as tall as the panel leaves them, all the same, frame
    /// after frame: they never grow it or each other.
    #[test]
    fn effects_fit_the_panel() {
        let (ctx, mut panel, mut state, track) = setup();
        state.add_effect(&track, EffectKind::Filter, 2);
        let mut first = Vec::new();
        for _ in 0..4 {
            run(&ctx, &mut panel, &mut state, vec![], &[]);
            if first.is_empty() {
                first.clone_from(&panel.rects);
            }
        }
        assert_eq!(panel.rects.len(), 3);
        assert_eq!(panel.rects, first, "stable across frames");
        // The default panel: 200 tall at the bottom of an 800 tall screen.
        for rect in &panel.rects {
            assert_eq!(rect.height(), panel.rects[0].height());
            assert!(rect.top() > 600. && rect.bottom() <= 800. - GAP, "{rect:?}");
            assert!(rect.left() >= GAP, "{rect:?}");
        }
        assert!(!panel.overflows, "the editors fit their frames");
    }
}
