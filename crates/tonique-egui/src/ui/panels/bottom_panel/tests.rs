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
    run_in(ctx, panel, state, &mut workspace, events, actions)
}

/// [`run`] with the panel laid out as in `workspace`.
fn run_in(
    ctx: &egui::Context,
    panel: &mut BottomPanel,
    state: &mut ProjectState,
    workspace: &mut Workspace,
    events: Vec<egui::Event>,
    actions: &[Action],
) -> Vec<Action> {
    let mut left = Vec::new();
    for (events, actions) in [(vec![], &[][..]), (events, actions)] {
        let mut commands = Commands::default();
        for action in actions {
            commands.push(*action);
        }
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
            events,
            // A second a frame: clicks a frame apart are never one
            // double-click.
            time: Some(ctx.input(|i| i.time) + 1.),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| panel.show(ui, state, workspace, &mut commands));
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

/// Showing a clip, the panel has no use for Delete and the like: they
/// go on to the timeline, even after a click in it.
#[test]
fn shortcuts_in_the_clip_view_are_for_the_timeline() {
    let (ctx, mut panel, mut state, track) = setup();
    let clip = state.new_clip_id();
    state.add_clips(
        &track,
        vec![crate::core::clip::AudioClip::new(
            clip,
            crate::analysis::AudioInfo::silent(1.),
            tonique_engine::time::BeatPos(0.),
            120.,
        )],
    );
    let mut workspace = Workspace::default();
    workspace.show_clip_details(&mut state, clip);
    let actions = [Action::Delete, Action::SelectAll, Action::Duplicate];
    let left = run_in(
        &ctx,
        &mut panel,
        &mut state,
        &mut workspace,
        click(egui::pos2(600., 760.)),
        &actions,
    );
    assert_eq!(left, actions);
    assert_eq!(workspace.bottom_view, BottomView::Clip(clip));
    assert_eq!(state.effects(&track).len(), 2, "effects untouched");
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
    state.add_effect(&track, EffectKind::Spectrum, 3);
    state.add_effect(&track, EffectKind::Utility, 4);
    let mut first = Vec::new();
    for _ in 0..4 {
        run(&ctx, &mut panel, &mut state, vec![], &[]);
        if first.is_empty() {
            first.clone_from(&panel.rects);
        }
    }
    assert_eq!(panel.rects.len(), 5);
    assert_eq!(panel.rects, first, "stable across frames");
    // The default panel: 200 tall at the bottom of an 800 tall screen.
    for rect in &panel.rects {
        assert_eq!(rect.height(), panel.rects[0].height());
        assert!(rect.top() > 600. && rect.bottom() <= 800. - GAP, "{rect:?}");
        assert!(rect.left() >= GAP, "{rect:?}");
    }
    assert!(!panel.overflows, "the editors fit their frames");
}

/// Editors go with their effects: an analyser's buffers are large.
#[test]
fn removed_effects_editors_are_dropped() {
    let (ctx, mut panel, mut state, track) = setup();
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    assert_eq!(panel.rack.len(), 2);
    state.remove_effects(&track, &[0]);
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    assert_eq!(panel.rack.len(), 1);
}

/// The rename shortcut types a name in the selected effect's header, over
/// the old one;
/// Enter keeps it. Double-click the header to collapse an effect to a
/// strip, and the strip to expand it.
#[test]
fn effects_are_renamed_and_collapsed_in_place() {
    let (ctx, mut panel, mut state, track) = setup();
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let header = panel.rects[0].left_top() + vec2(100., 10.);
    run(
        &ctx,
        &mut panel,
        &mut state,
        click(header),
        &[Action::Rename],
    );
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    run(
        &ctx,
        &mut panel,
        &mut state,
        vec![
            egui::Event::Text("Lows".into()),
            key(egui::Key::Enter, Default::default()),
        ],
        &[],
    );
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    assert_eq!(state.effects(&track)[0].name(), "Lows");

    let double_click = |pos| {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![
            egui::Event::PointerMoved(pos),
            button(true),
            button(false),
            button(true),
            button(false),
        ]
    };
    run(&ctx, &mut panel, &mut state, double_click(header), &[]);
    assert!(state.effects(&track)[0].collapsed);
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    assert_eq!(panel.rects[0].width(), crate::ui::effects::STRIP_WIDTH);
    assert!(!panel.overflows);
    let strip = panel.rects[0].center();
    run(&ctx, &mut panel, &mut state, double_click(strip), &[]);
    assert!(!state.effects(&track)[0].collapsed);
}

/// Renaming a collapsed effect expands it, to type in its header.
#[test]
fn renaming_a_collapsed_effect_expands_it() {
    let (ctx, mut panel, mut state, track) = setup();
    let plugin = state.effects(&track)[0].plugin.id;
    state.set_effect_collapsed(plugin, true);
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let strip = panel.rects[0].center();
    let left = run(
        &ctx,
        &mut panel,
        &mut state,
        click(strip),
        &[Action::Rename],
    );
    assert!(left.is_empty(), "the panel took it: {left:?}");
    assert!(!state.effects(&track)[0].collapsed);
}

/// The utility's switches set their parameter, as an undo step.
#[test]
fn utility_switches_are_undoable() {
    let (ctx, mut panel, mut state, track) = setup();
    state.remove_effects(&track, &[0, 1]);
    state.add_effect(&track, EffectKind::Utility, 0);
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let mono = |state: &ProjectState| state.effects(&track)[0].plugin.param("mono").unwrap().get();
    assert_eq!(mono(&state), 0.);
    // The first switch, under the knobs, centred in the editor: in the
    // frame (left of its meter, 3 + 4), within its 6 margins.
    let frame = panel.rects[0];
    let body = Rect::from_min_max(
        frame.min + vec2(6., 20. + 6.),
        frame.max - vec2(3. + 4. + 6., 6.),
    );
    let switches_top = body.center().y - (52. + 6. + 18.) / 2. + 52. + 6.;
    let at = egui::pos2(
        body.center().x - (3. * 28. + 2. * 2.) / 2. + 14.,
        switches_top + 9.,
    );
    run(&ctx, &mut panel, &mut state, click(at), &[]);
    assert_eq!(mono(&state), 1.);
    state.undo();
    assert_eq!(mono(&state), 0.);
}

/// Every shown effect's output is recorded for its meter; a bypassed
/// one's isn't, as it isn't heard.
#[test]
fn shown_effects_meter_their_output() {
    let (ctx, mut panel, mut state, track) = setup();
    let echo = state.effects(&track)[1].plugin.id;
    state.set_effect_enabled(&track, echo, false);
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let effects = state.effects(&track);
    assert_eq!(
        effects[0].plugin.tap.take_block(1),
        [true, true],
        "filter: its spectrum's input, and its meter"
    );
    assert_eq!(effects[1].plugin.tap.take_block(1), [false, false]);
}

/// Dragging an effect's header picks that effect up, for the track list and
/// the lanes to take.
#[test]
fn dragged_effects_say_where_they_come_from() {
    let (ctx, mut panel, mut state, track) = setup();
    run(&ctx, &mut panel, &mut state, vec![], &[]);
    let header = panel.rects[0].left_top() + vec2(100., 10.);
    let plugin = state.effects(&track)[0].plugin.id;
    let press = egui::Event::PointerButton {
        pos: header,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Default::default(),
    };
    run(
        &ctx,
        &mut panel,
        &mut state,
        vec![egui::Event::PointerMoved(header)],
        &[],
    );
    run(&ctx, &mut panel, &mut state, vec![press], &[]);
    let away = header + vec2(30., -30.);
    run(
        &ctx,
        &mut panel,
        &mut state,
        vec![egui::Event::PointerMoved(away)],
        &[],
    );
    let payload = egui::DragAndDrop::payload::<DragPayload>(&ctx).expect("picked up");
    assert!(matches!(
        *payload,
        DragPayload::MovedEffect { track: t, plugin: p } if t == track && p == plugin
    ));
}
