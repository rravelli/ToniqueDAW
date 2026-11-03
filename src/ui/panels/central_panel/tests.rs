use super::*;

#[test]
fn the_edit_cursor_points_down_at_the_timeline() {
    let [left, right, tip] = edit_cursor_arrow(50., 30.);
    assert_eq!(tip, pos2(50., 30.), "on the ruler's bottom");
    assert!(left.y < tip.y && left.y == right.y);
    assert_eq!((left.x + right.x) / 2., 50., "centred on the cursor");
}
use tonique_engine::engine::{Engine, EngineConfig};

/// The track list and timeline draw nested and collapsed groups without
/// trouble, headless.
#[test]
fn draws_groups_headless() {
    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    let tracks: Vec<_> = (0..5).map(|_| state.add_track()).collect();
    let inner = state.group(&tracks[..2]).unwrap();
    let outer = state.group(&[inner, tracks[2]]).unwrap();
    state.group(&[tracks[4]]).unwrap();
    state.track_view_mut(&outer).collapsed = false;
    state.track_view_mut(&inner).collapsed = true;

    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1200., 800.))),
        ..Default::default()
    };
    let mut panel = CentralPanel::new();
    let mut shapes = 0;
    for _ in 0..2 {
        let mut output = ctx.run_ui(input(), |ui| {
            panel.show(
                ui,
                &mut state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
        shapes = output.shapes.len();
    }
    assert!(shapes > 0);
    assert_eq!(
        state.rows().len(),
        6,
        "outer, inner (collapsed), t3, t4, group, t5"
    );
}

/// The wheel scrolls the tracks down, over their lanes as over their
/// headers, as far as the last one and no further.
#[test]
fn the_wheel_scrolls_tracks_vertically() {
    use egui::{Event, MouseWheelUnit, Pos2, TouchPhase};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    for _ in 0..30 {
        state.add_track();
    }
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    // A dialog over everything.
    let modal = std::cell::Cell::new(false);
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            );
            if modal.get() {
                egui::Modal::new(egui::Id::new("dialog")).show(ui.ctx(), |ui| {
                    ui.label("A dialog");
                });
            }
        });
        output.textures_delta.clear();
    };
    let mut scroll_over = |state: &mut ProjectState, pos: Pos2, delta: f32| {
        frame(state, vec![Event::PointerMoved(pos)]);
        frame(
            state,
            vec![Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0., delta),
                phase: TouchPhase::Move,
                modifiers: Default::default(),
            }],
        );
        // Let the smoothed scroll play out.
        for _ in 0..60 {
            frame(state, Vec::new());
        }
    };
    let lanes = Pos2::new(300., 300.);
    let headers = Pos2::new(1150., 300.);

    scroll_over(&mut state, lanes, -200.);
    assert!(state.grid.offset.y > 0., "scrolls over the lanes");
    scroll_over(&mut state, lanes, 200.);
    assert_eq!(state.grid.offset.y, 0., "and back up");
    scroll_over(&mut state, headers, -200.);
    assert!(state.grid.offset.y > 0., "scrolls over the headers");

    scroll_over(&mut state, lanes, -100_000.);
    let rows = RowLayout::new(&state, 0., 0.).height();
    assert!(state.grid.offset.y > rows - 800., "down to the last track");
    assert!(state.grid.offset.y < rows, "but no further");

    // Not through a dialog.
    modal.set(true);
    let offset = state.grid.offset;
    scroll_over(&mut state, lanes, 200.);
    scroll_over(&mut state, headers, 200.);
    assert_eq!(state.grid.offset, offset, "a dialog is open");
}

/// A long clip can be zoomed out to whole, and scrolled to its end with
/// a panel left of the timeline.
#[test]
fn the_wheel_scrolls_to_the_end_of_a_long_clip() {
    use crate::{analysis::AudioInfo, core::clip::AudioClip};
    use egui::{Event, MouseWheelUnit, Pos2, TouchPhase};
    use tonique_engine::time::BeatPos;

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    let track = state.add_track();
    let id = state.new_clip_id();
    let clip = AudioClip::new(id, AudioInfo::silent(31. * 60.), BeatPos(0.), 120.);
    state.add_clips(&track, vec![clip]);
    let end = state.arrangement_end();

    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    // The lanes where they were drawn.
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| -> Rect {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            egui::Panel::left("browser")
                .exact_size(300.)
                .show(ui, |_| {});
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            );
        });
        output.textures_delta.clear();
        panel.tracks.lanes()
    };
    let lanes = frame(&mut state, Vec::new());
    state.grid.fit(BeatPos::ZERO, end, lanes.width());
    assert!(
        state.grid.beats_to_width(end.0) <= lanes.width(),
        "the whole clip fits"
    );

    state.grid.zoom_by(20., lanes.left(), lanes);
    frame(&mut state, vec![Event::PointerMoved(lanes.center())]);
    frame(
        &mut state,
        vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(-1_000_000., 0.),
            phase: TouchPhase::Move,
            modifiers: Default::default(),
        }],
    );
    for _ in 0..60 {
        frame(&mut state, Vec::new());
    }
    assert!(
        state.grid.beats_to_x(end, lanes) <= lanes.right(),
        "scrolls to the clip's end"
    );
}

/// Dragging a file over a collapsed group expands it; leaving collapses
/// it again; dropping inside leaves it expanded.
#[test]
fn collapsed_groups_expand_while_dragging_over_them() {
    use crate::ui::arrangement::ruler::RULER_HEIGHT;
    use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
    use egui::{Event, PointerButton, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    let tracks: Vec<_> = (0..3).map(|_| state.add_track()).collect();
    let group = state.group(&tracks[..2]).unwrap();
    state.set_collapsed(&group, true);

    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    let audio = AudioInfo {
        name: "a.wav".into(),
        duration: Some(std::time::Duration::from_secs(1)),
        data: Default::default(),
        sample_rate: 48000,
        channels: 2,
        bit_depth: None,
        num_samples: None,
        path: Default::default(),
        missing: false,
    };
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
    };
    let collapsed = |state: &ProjectState| {
        state
            .rows()
            .iter()
            .find(|r| r.id == group)
            .unwrap()
            .collapsed
    };
    // The group's row is the first lane, just below the ruler.
    let over_group = Pos2::new(300., RULER_HEIGHT + 10.);
    let far_below = Pos2::new(300., 600.);

    frame(&mut state, Vec::new()); // lay out once
    egui::DragAndDrop::set_payload(&ctx, DragPayload::File(audio.clone()));
    frame(&mut state, vec![Event::PointerMoved(over_group)]);
    assert!(!collapsed(&state), "expands while dragged over");

    frame(&mut state, vec![Event::PointerMoved(far_below)]);
    assert!(collapsed(&state), "collapses again once left");

    frame(&mut state, vec![Event::PointerMoved(over_group)]);
    assert!(!collapsed(&state));
    // Drop inside.
    let release = Event::PointerButton {
        pos: over_group,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    };
    frame(&mut state, vec![release]);
    frame(&mut state, vec![Event::PointerMoved(over_group)]);
    frame(&mut state, vec![Event::PointerMoved(far_below)]);
    assert!(!collapsed(&state), "stays expanded after a drop inside");
}

/// Dragging a file below the last track shows the track a drop would
/// add: a header with the name it'll get. The track list, a panel shown
/// before the timeline, shows what the timeline found the frame before.
#[test]
fn dragging_past_the_last_track_shows_a_ghost_track() {
    use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
    use egui::{Event, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    state.add_track();
    state.add_track();
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    let mut texts = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    texts(&mut state, Vec::new()); // lay out once
    egui::DragAndDrop::set_payload(&ctx, DragPayload::File(AudioInfo::silent(1.)));
    texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
    let shown = texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
    assert!(
        shown.iter().any(|t| t == "3 Audio Track"),
        "the header: {shown:?}"
    );

    egui::DragAndDrop::clear_payload(&ctx);
    texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
    let shown = texts(&mut state, vec![Event::PointerMoved(Pos2::new(300., 500.))]);
    assert!(!shown.iter().any(|t| t == "3 Audio Track"), "gone after");
}

/// On the frame a drop adds the track, its header is shown where its
/// ghost was, and the ghost is gone: no frame without it.
#[test]
fn dropping_past_the_last_track_shows_its_header_at_once() {
    use crate::{analysis::AudioInfo, ui::dnd::DragPayload};
    use egui::{Event, PointerButton, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    state.add_track();
    state.add_track();
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    // The app's spacing, which the headers stack with.
    crate::ui::theme::Theme::default().install(&ctx);
    let mut panel = CentralPanel::new();
    // Where "3 Audio Track" is shown, each time.
    let mut names = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "3 Audio Track" => Some(t.pos),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let below = Pos2::new(300., 500.);
    names(&mut state, Vec::new()); // lay out once
    egui::DragAndDrop::set_payload(&ctx, DragPayload::File(AudioInfo::silent(1.)));
    names(&mut state, vec![Event::PointerMoved(below)]);
    let ghost = names(&mut state, vec![Event::PointerMoved(below)]);
    assert_eq!(ghost.len(), 1, "the ghost");

    let release = Event::PointerButton {
        pos: below,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    };
    let dropped = names(&mut state, vec![release]);
    assert_eq!(state.track_count(), 3);
    assert_eq!(dropped, ghost, "the real header, in the ghost's place");
}

/// A rubber band starts where the button went down (not snapping, between
/// grid lines as much as on them).
#[test]
fn rubber_bands_start_where_pressed() {
    use crate::ui::arrangement::ruler::RULER_HEIGHT;
    use egui::{Event, PointerButton, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    state.grid.settings.snap = false;
    state.add_track();
    state.add_track();
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    let mut time = 0.;
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
    };
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(&mut state, Vec::new()); // lay out once
    // Halfway between two beats, in the first lane.
    let ppb = state.grid.pixels_per_beat();
    let from = Pos2::new(2.5 * ppb, RULER_HEIGHT + 20.);
    frame(
        &mut state,
        vec![Event::PointerMoved(from), button(from, true)],
    );
    for step in 1..=5 {
        let at = from + vec2(step as f32 * 30., 40.);
        frame(&mut state, vec![Event::PointerMoved(at)]);
    }
    let bounds = state.selection_bounds().expect("a zone");
    assert!(
        (bounds.start_pos.0 - 2.5).abs() < 1e-3,
        "starts at {:?}",
        bounds.start_pos
    );
}

/// An effect dropped on the master's header, or on its lane in the
/// footer, goes on the master.
#[test]
fn effects_drop_on_the_master() {
    use crate::{core::effect::EffectKind, ui::dnd::DragPayload};
    use egui::{Event, PointerButton, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    state.add_track();
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    crate::ui::theme::Theme::default().install(&ctx);
    let mut panel = CentralPanel::new();
    let mut workspace = Workspace::default();
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(ui, state, &mut workspace, &mut Commands::default())
        });
        output.textures_delta.clear();
    };
    // The master's row, at the bottom.
    let y = 800. - state.master_track().height / 2.;
    let effects = |state: &ProjectState| state.effects(&MASTER_TRACK_ID).len();
    frame(&mut state, Vec::new()); // lay out once
    for (x, what) in [(1100., "header"), (400., "lane")] {
        let at = Pos2::new(x, y);
        let before = effects(&state);
        egui::DragAndDrop::set_payload(&ctx, DragPayload::Effect(EffectKind::Echo));
        frame(&mut state, vec![Event::PointerMoved(at)]);
        let release = Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        };
        frame(&mut state, vec![release]);
        frame(&mut state, Vec::new());
        assert_eq!(effects(&state), before + 1, "dropped on the {what}");
    }
}

/// The rename shortcut types a name in the selected track's header, over
/// the old one; Enter keeps it, as an undo step.
#[test]
fn rename_shortcut_renames_the_selected_track() {
    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    state.add_track();
    let track = state.add_track();
    state.select_track(&track);

    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    let mut frame = |state: &mut ProjectState, events: Vec<egui::Event>, actions: &[Action]| {
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
            panel.show(ui, state, &mut Workspace::default(), &mut commands)
        });
        output.textures_delta.clear();
        commands.take_all()
    };
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    frame(&mut state, vec![], &[]);
    assert!(
        frame(&mut state, vec![], &[Action::Rename]).is_empty(),
        "taken"
    );
    frame(&mut state, vec![], &[]);
    frame(
        &mut state,
        vec![
            egui::Event::Text("Bass".into()),
            key(egui::Key::Enter, Default::default()),
        ],
        &[],
    );
    frame(&mut state, vec![], &[]);
    let name = |state: &ProjectState| state.selected_track().unwrap().name;
    assert_eq!(name(&state), "Bass");
    state.undo();
    assert_ne!(name(&state), "Bass");
}

/// Ctrl+wheel zooms over a clip's header as over the empty lane.
#[test]
fn ctrl_wheel_zooms_over_clip_headers() {
    use crate::ui::arrangement::ruler::RULER_HEIGHT;
    use crate::{analysis::AudioInfo, core::clip::AudioClip};
    use egui::{Event, Modifiers, MouseWheelUnit, Pos2, TouchPhase};
    use tonique_engine::time::BeatPos;

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    let track = state.add_track();
    let clip = state.new_clip_id();
    let audio = AudioInfo::silent(60.);
    state.add_clips(
        &track,
        vec![AudioClip::new(clip, audio, BeatPos::ZERO, 120.)],
    );
    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    let mut panel = CentralPanel::new();
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(
                ui,
                state,
                &mut Workspace::default(),
                &mut Commands::default(),
            )
        });
        output.textures_delta.clear();
    };
    // The clip's header, at the top of the first lane.
    let header = Pos2::new(100., RULER_HEIGHT + 6.);
    frame(&mut state, vec![Event::PointerMoved(header)]);
    let before = state.grid.beats_to_width(1.);
    frame(
        &mut state,
        vec![Event::MouseWheel {
            unit: MouseWheelUnit::Line,
            delta: vec2(0., 1.),
            phase: TouchPhase::Move,
            modifiers: Modifiers::CTRL,
        }],
    );
    for _ in 0..30 {
        frame(&mut state, Vec::new());
    }
    assert!(state.grid.beats_to_width(1.) > before);
}

/// An effect from a track's chain dragged to another track's header or
/// lane moves there; on its own track's, nothing happens.
#[test]
fn effects_are_moved_to_other_tracks() {
    use crate::ui::arrangement::ruler::RULER_HEIGHT;
    use crate::{core::effect::EffectKind, ui::dnd::DragPayload};
    use egui::{Event, PointerButton, Pos2};

    let (engine, _processor) = Engine::new(EngineConfig::default());
    let mut state = ProjectState::new(engine);
    let first = state.add_track();
    let second = state.add_track();
    state.add_effect(&first, EffectKind::Echo, 0);
    let plugin = state.effects(&first)[0].plugin.id;

    let ctx = egui::Context::default();
    ctx.set_fonts(crate::ui::font::fonts());
    crate::ui::theme::Theme::default().install(&ctx);
    let mut panel = CentralPanel::new();
    let mut workspace = Workspace::default();
    let mut frame = |state: &mut ProjectState, events: Vec<Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200., 800.))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            panel.show(ui, state, &mut workspace, &mut Commands::default())
        });
        output.textures_delta.clear();
    };
    frame(&mut state, Vec::new()); // lay out once
    let layout = RowLayout::new(&state, RULER_HEIGHT, 0.);
    let on = |index: usize, x: f32| Pos2::new(x, layout.rows()[index].y.center());
    let (first_lane, first_header) = (on(0, 300.), on(0, 1100.));
    let (second_lane, second_header) = (on(1, 300.), on(1, 1100.));
    let mut drop_at = |state: &mut ProjectState, from: tonique_engine::edit::TrackId, at: Pos2| {
        egui::DragAndDrop::set_payload(
            &ctx,
            DragPayload::MovedEffect {
                track: from,
                plugin,
            },
        );
        frame(state, vec![Event::PointerMoved(at)]);
        frame(
            state,
            vec![Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        frame(state, Vec::new());
    };
    let counts = |state: &ProjectState| (state.effects(&first).len(), state.effects(&second).len());

    drop_at(&mut state, first, first_lane);
    assert_eq!(counts(&state), (1, 0), "its own track");

    drop_at(&mut state, first, second_lane);
    assert_eq!(counts(&state), (0, 1), "another track's lane");
    assert_eq!(state.selected_track().unwrap().id, second, "shown there");

    drop_at(&mut state, second, first_header);
    assert_eq!(counts(&state), (1, 0), "another track's header");

    drop_at(&mut state, first, second_header);
    assert_eq!(counts(&state), (0, 1));
    assert_eq!(
        state.effects(&second)[0].plugin.id,
        plugin,
        "the same effect"
    );
}
