use crate::{
    analysis::{AudioData, AudioInfo},
    config::settings::Settings,
    core::{
        clip::AudioClip,
        effect::EffectKind,
        state::{MASTER_TRACK_ID, PlaybackState, ProjectState},
    },
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tonique_engine::{
    edit::{ClipId, TrackId},
    engine::{Engine, EngineConfig},
    sample::SampleBuffer,
    time::BeatPos,
};

fn setup_state() -> ProjectState {
    let (engine, _processor) = Engine::new(EngineConfig::default());
    ProjectState::new(engine)
}

/// A decoded (silent) file of `seconds`. At 120 bpm that's `2 * seconds` beats.
fn audio(seconds: f32) -> AudioInfo {
    AudioInfo {
        name: "test.wav".into(),
        duration: Some(Duration::from_secs_f32(seconds)),
        data: Arc::new(AudioData::from_samples(SampleBuffer::new(
            vec![vec![0.; 480], vec![0.; 480]],
            48000.,
        ))),
        sample_rate: 48000,
        channels: 2,
        bit_depth: None,
        num_samples: None,
        path: PathBuf::from(format!("test-{seconds}.wav")),
    }
}

fn add_clip(state: &mut ProjectState, track: TrackId, position: f64, seconds: f32) -> ClipId {
    let id = state.new_clip_id();
    state.add_clips(
        &track,
        vec![AudioClip::new(id, audio(seconds), BeatPos(position))],
    );
    id
}

/// (position, end) of each clip on the track, sorted.
fn spans(state: &ProjectState, track: TrackId) -> Vec<(f64, f64)> {
    let bpm = state.bpm();
    let track = state.tracks().find(|t| t.id == track).unwrap();
    let mut spans: Vec<_> = track
        .clips
        .iter()
        .map(|c| {
            let round = |x: BeatPos| (x.0 * 1000.).round() / 1000.;
            (round(c.position), round(c.end(bpm)))
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    spans
}

#[test]
fn test_add_track() {
    let mut state = setup_state();
    let track = state.add_track();
    assert_eq!(state.track_count(), 1);
    assert_eq!(state.tracks().next().unwrap().id, track);
}

#[test]
fn test_add_track_at() {
    let mut state = setup_state();
    let track1 = state.add_track_at(0);
    assert_eq!(state.tracks().next().unwrap().id, track1);

    let track2 = state.add_track_at(0);
    assert_eq!(state.track_count(), 2);
    assert_eq!(state.tracks().next().unwrap().id, track2);

    let track3 = state.add_track_at(2);
    assert_eq!(state.track_count(), 3);
    assert_eq!(state.tracks().nth(2).unwrap().id, track3);
}

#[test]
fn test_delete_track() {
    let mut state = setup_state();
    let track1 = state.add_track();
    let track2 = state.add_track();
    assert_eq!(state.track_count(), 2);

    state.delete_track(&track1);
    // Should be deleted after state update
    assert_eq!(state.track_count(), 2);
    state.update();
    assert_eq!(state.track_count(), 1);
    state.delete_track(&track2);
    state.update();
    assert_eq!(state.track_count(), 0);
    // deleting a non existant track should no raise errors
    state.delete_track(&TrackId(12345));
    state.update();

    state.undo();
    state.undo();
    assert_eq!(
        state.tracks().map(|t| t.id).collect::<Vec<_>>(),
        [track1, track2]
    );
}

#[test]
fn adding_clips_trims_overlaps_in_one_undo_step() {
    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 4.); // beats 0..8
    add_clip(&mut state, track, 4., 1.); // beats 4..6, splits the first
    assert_eq!(spans(&state, track), [(0., 4.), (4., 6.), (6., 8.)]);

    state.undo();
    assert_eq!(spans(&state, track), [(0., 8.)]);
    state.redo();
    assert_eq!(spans(&state, track), [(0., 4.), (4., 6.), (6., 8.)]);
}

#[test]
fn move_resize_cut_and_duplicate_undo() {
    let mut state = setup_state();
    let a = state.add_track();
    let b = state.add_track();
    let clip = add_clip(&mut state, a, 0., 2.); // beats 0..4

    state.move_clip(&clip, &b, BeatPos(8.), &[]);
    assert!(spans(&state, a).is_empty());
    assert_eq!(spans(&state, b), [(8., 12.)]);

    state.commit_resize_clip(&clip, 0.25, 1., BeatPos(9.)); // drop the first beat
    assert_eq!(spans(&state, b), [(9., 12.)]);

    state.cut_clip_at(&b, BeatPos(10.));
    assert_eq!(spans(&state, b), [(9., 10.), (10., 12.)]);

    state.duplicate_clips(&[clip], None);
    assert_eq!(spans(&state, b), [(9., 10.), (10., 11.), (11., 12.)]);

    for _ in 0..4 {
        state.undo();
    }
    assert_eq!(spans(&state, a), [(0., 4.)]);
    assert!(spans(&state, b).is_empty());
}

#[test]
fn a_batch_is_one_undo_step() {
    let mut state = setup_state();
    state.begin_batch();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 1.);
    state.commit_batch();
    assert_eq!(state.track_count(), 1);
    state.undo();
    assert_eq!(state.track_count(), 0);
    assert!(!state.can_undo());
}

#[test]
fn tempo_changes_keep_audio_length_in_seconds() {
    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 2., 1.); // 2 beats at 120 bpm
    state.set_bpm(60.);
    assert_eq!(state.bpm(), 60.);
    assert_eq!(spans(&state, track), [(2., 3.)]);
    state.undo();
    assert_eq!(state.bpm(), 120.);
    assert_eq!(spans(&state, track), [(2., 4.)]);
}

#[test]
fn mixer_changes_undo() {
    let mut state = setup_state();
    let a = state.add_track();
    let b = state.add_track();

    state.set_volume(a, 0.3); // live drag, not undoable by itself
    state.commit_volume(a, 1.0, 0.5);
    assert_eq!(state.tracks().next().unwrap().volume, 0.5);
    state.undo();
    assert_eq!(state.tracks().next().unwrap().volume, 1.0);

    state.set_mute(b, true);
    assert!(state.tracks().nth(1).unwrap().muted);

    state.toggle_solo(a, false);
    state.toggle_solo(b, true);
    let soloed = |s: &ProjectState| {
        s.tracks()
            .map(|t| matches!(t.solo, crate::core::track::TrackSoloState::Solo))
            .collect::<Vec<_>>()
    };
    assert_eq!(soloed(&state), [true, true]);
    state.toggle_solo(a, false); // plain click on a soloed track clears solo
    assert_eq!(soloed(&state), [false, false]);
    state.undo();
    assert_eq!(soloed(&state), [true, true]);
}

#[test]
fn renames_are_undoable() {
    let mut state = setup_state();
    let track = state.add_track();
    state.track_view_mut(&track).name = "Drums".into();
    state.commit_track_view(&track);
    assert_eq!(state.tracks().next().unwrap().name, "Drums");
    state.undo();
    assert_eq!(state.tracks().next().unwrap().name, "# Audio Track");
    assert_eq!(state.track_view_mut(&track).name, "# Audio Track");
}

#[test]
fn only_tracks_can_be_armed() {
    let mut state = setup_state();
    let track = state.add_track();
    state.set_armed(&track, true);
    assert!(state.tracks().next().unwrap().armed);
    state.set_armed(&track, false);
    assert!(!state.tracks().next().unwrap().armed);

    let group = state.group(&[track]).unwrap();
    state.set_armed(&group, true);
    assert!(!state.rows()[0].armed, "a group");
    state.set_armed(&MASTER_TRACK_ID, true);
    assert!(!state.master_track().armed);
}

#[test]
fn effects_follow_the_plugin_chain() {
    let mut state = setup_state();
    let track = state.add_track();
    state.add_effect(&track, EffectKind::Filter, 0);
    state.add_effect(&track, EffectKind::Filter, 1);
    assert_eq!(state.effects(&track).len(), 2);
    // New effects start at their kind's values, not the engine's defaults.
    let cutoff = state.effects(&track)[0]
        .plugin
        .param("cutoff")
        .unwrap()
        .get();
    assert_eq!(cutoff, 1300.);

    state.remove_effects(&track, &[0]);
    assert_eq!(state.effects(&track).len(), 1);
    state.undo();
    assert_eq!(state.effects(&track).len(), 2);

    // Power button: bypass in the engine, undoable.
    let plugin = state.effects(&track)[0].plugin.id;
    state.set_effect_enabled(&track, plugin, false);
    assert!(!state.effects(&track)[0].enabled());
    state.undo();
    assert!(state.effects(&track)[0].enabled());

    state.duplicate_track(&track);
    let copy = state.tracks().nth(1).unwrap().id;
    assert_eq!(state.effects(&copy).len(), 2);
}

#[test]
fn clips_play_through_the_engine_once_loaded() {
    use tonique_engine::engine::render_offline;

    let (engine, mut processor) = Engine::new(EngineConfig {
        sample_rate: 48000.,
        housekeeping_thread: false,
        ..Default::default()
    });
    let mut state = ProjectState::new(engine);
    let track = state.add_track();
    // One second of a loud square wave, decoded at another rate so it's resampled.
    let mut info = audio(1.);
    let wave: Vec<f32> = (0..44100)
        .map(|i| if i / 50 % 2 == 0 { 0.5 } else { -0.5 })
        .collect();
    info.data = Arc::new(AudioData::from_samples(SampleBuffer::new(
        vec![wave.clone(), wave],
        44100.,
    )));
    info.sample_rate = 44100;
    let id = state.new_clip_id();
    state.add_clips(&track, vec![AudioClip::new(id, info, BeatPos(0.))]);
    state.play();

    // The file is converted on a background thread; the clip is silent until then.
    let mut peak = 0.0f32;
    for _ in 0..200 {
        state.update();
        state.session.engine().collect_garbage(); // hand over pending graphs
        let out = render_offline(&mut processor, 256, 2);
        peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
        if peak > 0. {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(peak > 0.4, "clip never became audible (peak {peak})");

    state.update();
    let [left, _] = state.metrics.tracks[&track].peak();
    assert!(left > 0.4, "track meter shows {left}");
    assert!(state.playhead().0 > 0.);
}

#[test]
fn edit_cursor_moves_the_playhead_only_when_stopped() {
    let mut state = setup_state();
    state.set_edit_cursor(BeatPos(4.));
    assert_eq!(
        (state.edit_cursor(), state.playhead()),
        (BeatPos(4.), BeatPos(4.))
    );

    state.play();
    state.set_edit_cursor(BeatPos(8.));
    assert_eq!(
        (state.edit_cursor(), state.playhead()),
        (BeatPos(8.), BeatPos(4.))
    );

    // Stopping returns to the edit cursor.
    state.stop();
    assert_eq!(state.playhead(), BeatPos(8.));
}

#[test]
fn seeking_moves_both_cursors() {
    let mut state = setup_state();
    state.play();
    state.seek(BeatPos(2.));
    assert_eq!(
        (state.edit_cursor(), state.playhead()),
        (BeatPos(2.), BeatPos(2.))
    );
    state.seek(BeatPos(-1.));
    assert_eq!(
        (state.edit_cursor(), state.playhead()),
        (BeatPos(0.), BeatPos(0.))
    );
}

#[test]
fn loop_range_is_ordered_and_never_empty() {
    let mut state = setup_state();
    state.set_loop_range(BeatPos(8.), BeatPos(4.));
    assert_eq!(state.loop_range(), (BeatPos(4.), BeatPos(8.)));
    state.set_loop_range(BeatPos(-2.), BeatPos(1.));
    assert_eq!(state.loop_range(), (BeatPos(0.), BeatPos(1.)));
    state.set_loop_range(BeatPos(3.), BeatPos(3.));
    assert_eq!(state.loop_range(), (BeatPos(0.), BeatPos(1.)));
}

#[test]
fn playback_wraps_inside_the_loop_after_a_tempo_change() {
    use tonique_engine::engine::render_offline;

    let (engine, mut processor) = Engine::new(EngineConfig {
        sample_rate: 48000.,
        housekeeping_thread: false,
        ..Default::default()
    });
    let mut state = ProjectState::new(engine);
    state.set_loop_range(BeatPos(0.), BeatPos(1.));
    state.set_looping(true);
    // The loop was sent in samples at 120 bpm: it must follow the new tempo.
    state.set_bpm(60.);
    state.play();

    // Two seconds, with the loop being one second long at 60 bpm.
    let mut furthest = BeatPos::ZERO;
    for _ in 0..(2 * 48000 / 256) {
        state.update();
        render_offline(&mut processor, 256, 2);
        state.update();
        furthest = furthest.max(state.playhead());
        assert!(
            state.playhead().0 < 1.,
            "played past the loop: {:?}",
            state.playhead()
        );
    }
    assert!(
        furthest.0 > 0.9,
        "never reached the loop end ({furthest:?})"
    );
}

#[test]
fn zone_selects_overlapping_clips_on_its_tracks() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let (t0, t1, t2) = (state.add_track(), state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 1.); // beats 0..2
    let b = add_clip(&mut state, t1, 3., 1.); // beats 3..5
    let _outside_range = add_clip(&mut state, t1, 8., 1.);
    let _outside_tracks = add_clip(&mut state, t2, 0., 1.);

    // Dragged from bottom right to top left: corners in any order.
    state.select_in_bounds(SelectionBounds::between((1, BeatPos(4.)), (0, BeatPos(1.))));
    assert_eq!(state.selected_clips(), &[a, b]);
    assert_eq!(state.selection_range(), Some((BeatPos(1.), BeatPos(4.))));
}

#[test]
fn clicking_toggles_and_replaces_the_selection() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.);
    let b = add_clip(&mut state, track, 4., 1.);

    state.select_clips(vec![a]);
    state.toggle_clip_selected(b);
    assert_eq!(state.selected_clips(), &[a, b]);
    state.toggle_clip_selected(a);
    assert_eq!(state.selected_clips(), &[b]);
    // Without a zone, the range spans the selected clips.
    assert_eq!(state.selection_range(), Some((BeatPos(4.), BeatPos(6.))));
    state.clear_clip_selection();
    assert_eq!(state.selection_range(), None);
}

#[test]
fn loop_selection_loops_over_the_selected_clips() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 2., 1.);
    let b = add_clip(&mut state, track, 6., 1.);
    assert!(!state.loop_selection());

    state.select_clips(vec![a, b]);
    assert!(state.loop_selection());
    assert!(state.looping());
    assert_eq!(state.loop_range(), (BeatPos(2.), BeatPos(8.)));
}

#[test]
fn duplicating_selects_the_copies() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 1.);
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(0.)), (0, BeatPos(4.))));
    let originals = state.selected_clips().to_vec();

    state.duplicate_selected_clips();
    assert_eq!(state.selected_clips().len(), 1);
    assert_ne!(state.selected_clips(), originals.as_slice());
    assert_eq!(state.selection_range(), Some((BeatPos(4.), BeatPos(8.))));
    assert_eq!(spans(&state, track), vec![(0., 2.), (4., 6.)]);
}

#[test]
fn deleted_or_undone_clips_leave_the_selection() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.);
    let b = add_clip(&mut state, track, 4., 1.);

    state.select_clips(vec![a, b]);
    state.undo(); // removes b
    assert_eq!(state.selected_clips(), &[a]);

    state.delete_selected_clips();
    assert!(state.selected_clips().is_empty());
    assert!(spans(&state, track).is_empty());
}

#[test]
fn arrangement_end_covers_clips_loop_and_playhead() {
    let mut state = setup_state();
    let track = state.add_track();
    assert_eq!(state.arrangement_end(), BeatPos(0.));
    add_clip(&mut state, track, 4., 1.);
    assert_eq!(state.arrangement_end(), BeatPos(6.));
    state.set_loop_range(BeatPos(0.), BeatPos(12.));
    assert_eq!(state.arrangement_end(), BeatPos(6.)); // the loop counts only when on
    state.set_looping(true);
    assert_eq!(state.arrangement_end(), BeatPos(12.));
    state.seek(BeatPos(20.));
    assert_eq!(state.arrangement_end(), BeatPos(20.));
}

#[test]
fn paste_goes_to_the_edit_cursor_on_the_selected_track_and_appends() {
    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 2., 1.); // beats 2..4
    state.select_clips(vec![a]);
    assert!(state.copy_selection());

    state.select_track(&t1);
    state.set_edit_cursor(BeatPos(8.));
    state.paste();
    state.paste(); // the cursor moved past the first paste
    assert_eq!(spans(&state, t1), vec![(8., 10.), (10., 12.)]);
    assert_eq!(state.edit_cursor(), BeatPos(12.));
    assert_eq!(state.selected_clips().len(), 1);

    state.undo(); // each paste is one step
    assert_eq!(spans(&state, t1), vec![(8., 10.)]);
    assert_eq!(spans(&state, t0), vec![(2., 4.)]);
}

#[test]
fn paste_keeps_track_offsets_and_creates_missing_tracks() {
    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 1.);
    let b = add_clip(&mut state, t1, 2., 1.);
    state.select_clips(vec![a, b]);
    state.copy_selection();

    state.select_track(&t1);
    state.set_edit_cursor(BeatPos(8.));
    state.paste();
    assert_eq!(state.track_count(), 3);
    let t2 = state.tracks().nth(2).unwrap().id;
    assert_eq!(spans(&state, t1), vec![(2., 4.), (8., 10.)]);
    assert_eq!(spans(&state, t2), vec![(10., 12.)]);
}

#[test]
fn cutting_a_zone_removes_only_its_part() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 2.); // beats 0..4
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (0, BeatPos(3.))));
    state.cut_selection();
    assert_eq!(spans(&state, track), vec![(0., 1.), (3., 4.)]);
    assert!(state.selected_clips().is_empty());

    state.set_edit_cursor(BeatPos(8.));
    state.paste();
    assert_eq!(spans(&state, track), vec![(0., 1.), (3., 4.), (8., 10.)]);
}

#[test]
fn a_zone_only_picks_clips_inside_it_not_touching_it() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 1.); // beats 0..2, ends on the zone
    let inside = add_clip(&mut state, track, 2., 1.); // beats 2..4
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(2.)), (0, BeatPos(3.))));
    assert_eq!(state.selected_clips(), &[inside]);
}

#[test]
fn deleting_a_zone_removes_only_its_part() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    add_clip(&mut state, t0, 0., 2.); // beats 0..4
    add_clip(&mut state, t1, 2., 2.); // beats 2..6
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (1, BeatPos(3.))));
    state.delete_selected_clips();
    assert_eq!(spans(&state, t0), vec![(0., 1.), (3., 4.)]);
    assert_eq!(spans(&state, t1), vec![(3., 6.)]);
    assert!(state.selected_clips().is_empty());

    state.undo(); // one step
    assert_eq!(spans(&state, t0), vec![(0., 4.)]);
    assert_eq!(spans(&state, t1), vec![(2., 6.)]);
}

#[test]
fn nudging_a_zone_moves_only_its_part() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let track = state.add_track();
    add_clip(&mut state, track, 0., 3.); // beats 0..6
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (0, BeatPos(3.))));

    state.nudge_selection(1.);
    // The part 1..3 moved to 2..4, over what was left of the clip.
    assert_eq!(spans(&state, track), vec![(0., 1.), (2., 4.), (4., 6.)]);
    assert_eq!(state.selection_range(), Some((BeatPos(2.), BeatPos(4.))));
    assert_eq!(state.selected_clips().len(), 1);

    // Stops at the start: the moved part, not the clip, starts at 0.
    state.nudge_selection(-10.);
    assert_eq!(spans(&state, track), vec![(0., 2.), (4., 6.)]);

    state.undo();
    state.undo();
    assert_eq!(spans(&state, track), vec![(0., 6.)]);
}

#[test]
fn moving_a_zone_between_tracks_moves_only_its_part() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    add_clip(&mut state, t0, 0., 2.); // beats 0..4
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (0, BeatPos(3.))));

    state.move_selection_tracks(1);
    assert_eq!(spans(&state, t0), vec![(0., 1.), (3., 4.)]);
    assert_eq!(spans(&state, t1), vec![(1., 3.)]);

    state.undo();
    assert_eq!(spans(&state, t0), vec![(0., 4.)]);
    assert!(spans(&state, t1).is_empty());
}

#[test]
fn splitting_at_a_zone_keeps_the_ids_inside_for_a_drop() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let clip = add_clip(&mut state, t0, 0., 2.); // beats 0..4
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (0, BeatPos(3.))));

    // What dropping dragged clips does: split, then move by id.
    state.begin_batch();
    state.split_at_zone();
    assert_eq!(state.selected_clips(), &[clip]);
    state.move_clip(&clip, &t1, BeatPos(5.), &[clip]);
    state.commit_batch();
    assert_eq!(spans(&state, t0), vec![(0., 1.), (3., 4.)]);
    assert_eq!(spans(&state, t1), vec![(5., 7.)]);

    state.undo(); // one step
    assert_eq!(spans(&state, t0), vec![(0., 4.)]);
    assert!(spans(&state, t1).is_empty());
}

#[test]
fn nudging_moves_the_selection_as_a_block() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 1., 1.); // beats 1..3
    let b = add_clip(&mut state, track, 3., 1.); // beats 3..5
    add_clip(&mut state, track, 6., 1.); // unselected, beats 6..8
    state.select_clips(vec![a, b]);

    state.nudge_selection(2.);
    // Moved onto the unselected clip, which gets trimmed.
    assert_eq!(spans(&state, track), vec![(3., 5.), (5., 7.), (7., 8.)]);
    state.nudge_selection(-10.); // stops at the start
    assert_eq!(spans(&state, track), vec![(0., 2.), (2., 4.), (7., 8.)]);

    state.undo();
    state.undo();
    assert_eq!(spans(&state, track), vec![(1., 3.), (3., 5.), (6., 8.)]);
}

#[test]
fn clips_never_start_before_the_first_beat() {
    let mut state = setup_state();
    let track = state.add_track();
    let id = state.new_clip_id();
    state.add_clips(&track, vec![AudioClip::new(id, audio(1.), BeatPos(-3.))]);
    assert_eq!(spans(&state, track), vec![(0., 2.)]);

    state.move_clip(&id, &track, BeatPos(-1.), &[]);
    assert_eq!(spans(&state, track), vec![(0., 2.)]);
}

#[test]
fn trimming_the_start_stops_at_the_first_beat() {
    let bpm = 120.;
    let mut clip = AudioClip::new(ClipId(1), audio(2.), BeatPos(1.)); // beats 1..5
    clip.trim_start_at(BeatPos(2.), bpm); // hide the first 1 beat of the file: 2..5
    clip.position -= 1.5; // moved near the start: 0.5..3.5, file starts at -0.5

    clip.trim_start_at(BeatPos(-4.), bpm);
    assert_eq!(clip.position, BeatPos(0.));
    assert!(
        (clip.end(bpm).0 - 3.5).abs() < 1e-4,
        "the end moved: {:?}",
        clip.end(bpm)
    );
}

#[test]
fn trimming_a_clip_of_unknown_length_does_nothing() {
    let bpm = 120.;
    let mut info = audio(2.);
    info.duration = None;
    let mut clip = AudioClip::new(ClipId(1), info, BeatPos(1.));
    clip.trim_start_at(BeatPos(2.), bpm);
    clip.trim_end_at(BeatPos(1.5), bpm);
    clip.crop(BeatPos(0.), BeatPos(3.), bpm);
    assert_eq!(
        (clip.position, clip.trim_start, clip.trim_end),
        (BeatPos(1.), 0., 1.)
    );
}

#[test]
fn select_all_selects_every_clip() {
    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 1.);
    let b = add_clip(&mut state, t1, 4., 1.);
    state.select_all_clips();
    assert_eq!(state.selected_clips(), &[a, b]);
}

#[test]
fn moving_between_tracks_keeps_the_block_and_stays_in_range() {
    use crate::core::state::SelectionBounds;

    let mut state = setup_state();
    let (t0, t1, t2) = (state.add_track(), state.add_track(), state.add_track());
    add_clip(&mut state, t0, 0., 1.); // beats 0..2
    add_clip(&mut state, t1, 0., 1.);
    add_clip(&mut state, t2, 1., 1.); // unselected, beats 1..3
    // Covering the clips whole: they move whole.
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(0.)), (1, BeatPos(2.))));

    state.move_selection_tracks(5); // only one track of room below
    assert!(spans(&state, t0).is_empty());
    assert_eq!(spans(&state, t1), vec![(0., 2.)]);
    // Landed on the unselected clip, which gets trimmed.
    assert_eq!(spans(&state, t2), vec![(0., 2.), (2., 3.)]);
    let bounds = state.selection_bounds().unwrap();
    assert_eq!((bounds.start_track_index, bounds.end_track_index), (1, 2));

    state.move_selection_tracks(-1);
    state.move_selection_tracks(-1); // already on the first track: no-op
    assert_eq!(spans(&state, t0), vec![(0., 2.)]);
    state.undo(); // back down
    assert!(spans(&state, t0).is_empty());
    state.undo(); // back to the start
    assert_eq!(spans(&state, t0), vec![(0., 2.)]);
    assert_eq!(spans(&state, t2), vec![(1., 3.)]);
}

#[test]
fn snap_targets_are_other_clips_loop_and_edit_cursor() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.); // beats 0..2
    add_clip(&mut state, track, 3., 1.); // beats 3..5
    state.set_loop_range(BeatPos(8.), BeatPos(12.));
    state.set_edit_cursor(BeatPos(6.));
    let mut targets = state.snap_targets(&[a]);
    targets.sort_by(BeatPos::total_cmp);
    assert_eq!(targets, [3., 5., 6., 8., 12.].map(BeatPos));
}

#[test]
fn snapping_prefers_targets_in_reach_over_the_grid() {
    use crate::core::grid::{GridService, TARGET_REACH};

    let grid = GridService::new(); // one grid line per beat
    let reach = grid.width_to_beats(TARGET_REACH);
    let target = BeatPos(2.1 + reach * 0.9);
    // A target in reach wins over a nearer grid line.
    assert_eq!(
        grid.snap_to_targets(BeatPos(2.1), &[target]),
        Some((target, true))
    );
    // Out of reach: the grid, when close enough to a line.
    assert_eq!(
        grid.snap_to_targets(BeatPos(2.1), &[BeatPos(2.1 + reach * 2.)]),
        Some((BeatPos(2.), false))
    );
    assert_eq!(grid.snap_to_targets(BeatPos(2.5), &[]), None);
}

#[test]
fn new_tracks_go_through_the_palette() {
    use egui::Color32;
    let mut state = setup_state();
    state.set_track_palette(&[Color32::RED, Color32::BLUE]);
    let colors: Vec<_> = (0..3)
        .map(|_| {
            let id = state.add_track();
            state.track_view_mut(&id).color
        })
        .collect();
    assert_eq!(colors, [Color32::RED, Color32::BLUE, Color32::RED]);
}

mod projects {
    use super::*;
    use crate::{
        cache::AUDIO_ANALYSIS_CACHE,
        core::{
            project::{ClipFile, ProjectFile},
            state::MASTER_TRACK_ID,
        },
    };
    use egui::Color32;
    use std::{fs, path::Path};
    use tonique_engine::sample::WavWriter;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tonique-save-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A 2 s stereo WAV of silence.
    fn write_wav(path: &Path) {
        let mut wav = WavWriter::create(path, 2, 48000.).unwrap();
        wav.write_interleaved(&vec![0.; 2 * 96000]).unwrap();
        wav.finalize().unwrap();
    }

    /// Floats to 1e-4, so values recomputed on load compare equal.
    fn rounded(mut p: ProjectFile) -> ProjectFile {
        let r = |x: &mut f32| *x = (*x * 1e4).round() / 1e4;
        r(&mut p.bpm);
        let r64 = |x: &mut f64| *x = (*x * 1e4).round() / 1e4;
        r64(&mut p.loop_range.0);
        r64(&mut p.loop_range.1);
        for channel in
            std::iter::once(&mut p.master).chain(p.tracks.iter_mut().map(|t| &mut t.channel))
        {
            r(&mut channel.volume);
            r(&mut channel.pan);
            channel
                .effects
                .iter_mut()
                .flat_map(|e| e.params.values_mut())
                .for_each(r);
        }
        for track in &mut p.tracks {
            r(&mut track.height);
            for clip in &mut track.clips {
                r64(&mut clip.position);
                r(&mut clip.trim_start);
                r(&mut clip.trim_end);
            }
        }
        p
    }

    #[test]
    fn projects_round_trip_through_a_file() {
        let dir = temp_dir("round-trip");
        write_wav(&dir.join("loop.wav"));

        let mut state = setup_state();
        state.set_bpm(100.);
        state.set_loop_range(BeatPos(4.), BeatPos(12.));
        state.set_looping(true);
        let track = state.add_track();
        let audio = AUDIO_ANALYSIS_CACHE
            .get_or_analyze(dir.join("loop.wav"))
            .unwrap();
        let mut clip = AudioClip::new(state.new_clip_id(), audio, BeatPos(2.));
        clip.trim_start = 0.25;
        clip.trim_end = 0.75;
        state.add_clips(&track, vec![clip]);
        state.commit_volume(track, 1.0, 0.5);
        state.commit_volume(MASTER_TRACK_ID, 1.0, 0.8);
        state.set_mute(track, true);
        state.toggle_solo(track, false);
        let view = state.track_view_mut(&track);
        view.name = "Drums".into();
        #[allow(clippy::disallowed_methods)]
        {
            view.color = Color32::from_rgb(10, 20, 30);
        }
        view.height = 90.;
        view.collapsed = true;
        state.commit_track_view(&track);
        state.add_effect(&track, EffectKind::Filter, 0);
        let plugin = state.edit().track(track).unwrap().channel.plugins[0].clone();
        plugin.param("cutoff").unwrap().set(800.);
        state.set_effect_enabled(&track, plugin.id, false);

        let saved = state.project(Some(&dir));
        let track_file = &saved.tracks[0];
        assert_eq!(track_file.clips[0].path, Path::new("loop.wav"));
        assert_eq!(track_file.color, "#0a141e");
        assert!(!track_file.channel.effects[0].enabled);
        let path = dir.join("song.tonique");
        saved.write(&path).unwrap();

        let mut loaded = setup_state();
        let problems = loaded.load_project(&ProjectFile::read(&path).unwrap(), &dir);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(rounded(loaded.project(Some(&dir))), rounded(saved));
        assert!(!loaded.can_undo(), "loading isn't undoable");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_audio_is_reported_and_the_rest_loads() {
        let dir = temp_dir("missing");
        let mut project = setup_state().project(Some(&dir));
        project.tracks.push(crate::core::project::TrackFile {
            name: "Gone".into(),
            color: "#ffffff".into(),
            height: 60.,
            collapsed: false,
            soloed: false,
            group: None,
            channel: project.master.clone(),
            clips: vec![ClipFile {
                path: "gone.wav".into(),
                position: 0.,
                trim_start: 0.,
                trim_end: 1.,
            }],
        });
        let mut state = setup_state();
        let problems = state.load_project(&project, &dir);
        assert_eq!(problems, ["Missing audio file: gone.wav"]);
        let tracks: Vec<_> = state.tracks().collect();
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].name, "Gone");
        assert!(tracks[0].clips.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_project_is_empty() {
        let mut state = setup_state();
        let track = state.add_track();
        add_clip(&mut state, track, 0., 1.);
        state.set_bpm(90.);
        state.set_looping(true);
        state.new_project();
        assert_eq!(state.tracks().count(), 0);
        assert_eq!(state.bpm(), 120.);
        assert!(!state.looping());
        assert!(!state.can_undo());
        assert_eq!(state.project(None), setup_state().project(None));
    }
}

mod groups {
    use super::*;
    use crate::core::{
        state::RowTarget,
        track::{TrackKind, TrackSoloState},
    };
    use std::path::Path;

    /// A track named `name`, added last.
    fn track(state: &mut ProjectState, name: &str) -> TrackId {
        let id = state.add_track();
        state.track_view_mut(&id).name = name.into();
        state.commit_track_view(&id);
        id
    }

    fn rename(state: &mut ProjectState, id: TrackId, name: &str) {
        state.track_view_mut(&id).name = name.into();
        state.commit_track_view(&id);
    }

    /// The visible rows, indented by depth.
    fn tree(state: &ProjectState) -> Vec<String> {
        state
            .rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    /// Every group appears once and its tracks are contiguous.
    fn assert_tree_order(state: &ProjectState) {
        let groups: Vec<_> = state
            .rows()
            .into_iter()
            .filter(|r| r.kind == TrackKind::Group)
            .map(|r| r.id)
            .collect();
        for (i, g) in groups.iter().enumerate() {
            assert!(!groups[i + 1..].contains(g), "group listed twice");
            let indices: Vec<usize> = state
                .tracks_in(*g)
                .iter()
                .map(|t| state.track_index(*t).unwrap())
                .collect();
            assert!(!indices.is_empty(), "empty group left behind");
            assert!(
                indices.windows(2).all(|w| w[1] == w[0] + 1),
                "group's tracks aren't together: {indices:?}"
            );
        }
    }

    #[test]
    fn grouping_and_ungrouping_are_single_steps() {
        let mut state = setup_state();
        let [_, b, _, d] = ["a", "b", "c", "d"].map(|n| track(&mut state, n));

        let g = state.group(&[b, d]).unwrap();
        rename(&mut state, g, "G");
        assert_eq!(tree(&state), ["a", "G", "  b", "  d", "c"]);
        assert_tree_order(&state);
        state.undo(); // rename
        state.undo(); // group
        assert_eq!(tree(&state), ["a", "b", "c", "d"]);
        state.redo();
        state.redo();

        state.ungroup(g);
        assert_eq!(tree(&state), ["a", "b", "d", "c"]);
        assert!(!state.is_group(g));
        state.undo();
        assert_eq!(tree(&state), ["a", "G", "  b", "  d", "c"]);
    }

    #[test]
    fn groups_nest_without_limit_and_across_branches() {
        let mut state = setup_state();
        let [t1, t2, t3, t4, t5] = ["t1", "t2", "t3", "t4", "t5"].map(|n| track(&mut state, n));
        // Five levels: each group goes into a new one.
        let inner = state.group(&[t1, t2]).unwrap();
        let mut outer = state.group(&[inner, t3]).unwrap();
        for _ in 0..3 {
            outer = state.group(&[outer]).unwrap();
        }
        assert_eq!(state.rows().iter().find(|r| r.id == t1).unwrap().depth, 5);
        assert_eq!(state.ancestors(t1).len(), 5);
        assert_tree_order(&state);

        // A deep group and a track from another branch: the new group goes
        // where the branches meet (the top level).
        let side = state.group(&[t4, t5]).unwrap();
        let both = state.group(&[inner, t4]).unwrap();
        assert_eq!(state.parent(both), None);
        assert_eq!(state.parent(inner), Some(both));
        assert_eq!(state.parent(t4), Some(both));
        assert_eq!(state.tracks_in(side), [t5]);
        assert_eq!(state.parent(t3).map(|g| state.ancestors(g).len()), Some(3));
        assert_tree_order(&state);

        // Grouping a group together with what's inside it: the inside moves with it.
        let again = state.group(&[both, t1]).unwrap();
        assert_eq!(state.parent(both), Some(again));
        assert_eq!(state.parent(t1), Some(inner));
        assert_tree_order(&state);
    }

    #[test]
    fn rows_move_in_and_out_of_groups() {
        let mut state = setup_state();
        let [a, b, c, d] = ["a", "b", "c", "d"].map(|n| track(&mut state, n));
        let g1 = state.group(&[a, b]).unwrap();
        rename(&mut state, g1, "G1");
        let g2 = state.group(&[c]).unwrap();
        rename(&mut state, g2, "G2");
        assert_eq!(tree(&state), ["G1", "  a", "  b", "G2", "  c", "d"]);

        // A group into another group, first inside.
        assert!(state.move_row(g2, RowTarget::Into(g1)));
        assert_eq!(tree(&state), ["G1", "  G2", "    c", "  a", "  b", "d"]);
        // Not into itself or its own subgroup.
        assert!(!state.move_row(g1, RowTarget::Into(g2)));
        assert!(!state.move_row(g1, RowTarget::Before(c)));
        assert!(!state.move_row(g1, RowTarget::Into(g1)));
        // A track out, before another row.
        assert!(state.move_row(b, RowTarget::Before(d)));
        assert_eq!(tree(&state), ["G1", "  G2", "    c", "  a", "b", "d"]);
        // Last track out of G2: the group goes, in the same step.
        assert!(state.move_row(c, RowTarget::End));
        assert_eq!(tree(&state), ["G1", "  a", "b", "d", "c"]);
        assert!(!state.is_group(g2));
        assert_tree_order(&state);
        state.undo();
        assert_eq!(tree(&state), ["G1", "  G2", "    c", "  a", "b", "d"]);
    }

    #[test]
    fn deleting_groups_and_their_last_tracks() {
        let mut state = setup_state();
        let [a, b, c] = ["a", "b", "c"].map(|n| track(&mut state, n));
        let inner = state.group(&[a]).unwrap();
        let outer = state.group(&[inner, b]).unwrap();

        state.delete_group(outer);
        assert_eq!(tree(&state), ["c"]);
        state.undo();
        assert_eq!(state.tracks().count(), 3);
        assert_eq!(state.parent(a), Some(inner));
        assert_tree_order(&state);

        // Deleting a group's last track removes the group too.
        state.delete_track(&a);
        state.update();
        assert!(!state.is_group(inner));
        assert_eq!(state.parent(b), Some(outer));
        state.undo();
        assert_eq!(state.parent(a), Some(inner));
        let _ = c;
    }

    #[test]
    fn collapsed_groups_hide_their_rows() {
        let mut state = setup_state();
        let [a, b, _] = ["a", "b", "c"].map(|n| track(&mut state, n));
        let inner = state.group(&[a]).unwrap();
        rename(&mut state, inner, "in");
        let outer = state.group(&[inner, b]).unwrap();
        rename(&mut state, outer, "out");
        state.track_view_mut(&inner).collapsed = true;
        assert_eq!(tree(&state), ["out", "  in", "  b", "c"]);
        state.track_view_mut(&outer).collapsed = true;
        assert_eq!(tree(&state), ["out", "c"]);
        assert!(state.is_hidden(a) && state.is_hidden(inner));
        assert!(!state.is_hidden(outer));

        // A rubber band across the collapsed group doesn't pick what's hidden.
        let clip = add_clip(&mut state, b, 0., 1.);
        state.select_in_bounds(crate::core::state::SelectionBounds::between(
            (0, BeatPos(0.)),
            (2, BeatPos(8.)),
        ));
        assert!(!state.selected_clips().contains(&clip));
    }

    #[test]
    fn soloing_and_muting_a_group() {
        let mut state = setup_state();
        let [a, b, c] = ["a", "b", "c"].map(|n| track(&mut state, n));
        let inner = state.group(&[a]).unwrap();
        let outer = state.group(&[inner, b]).unwrap();

        state.toggle_solo(outer, false);
        let solo = |s: &ProjectState, id| s.rows().into_iter().find(|r| r.id == id).unwrap().solo;
        assert!(
            matches!(solo(&state, a), TrackSoloState::NotSoloing),
            "audible through its group"
        );
        assert!(matches!(solo(&state, c), TrackSoloState::Soloing));
        assert!(matches!(solo(&state, outer), TrackSoloState::Solo));
        // A plain click elsewhere clears the group's solo.
        state.toggle_solo(c, false);
        assert!(matches!(solo(&state, outer), TrackSoloState::NotSoloing));
        state.toggle_solo(c, false);

        state.set_mute(inner, true);
        assert!(
            state
                .rows()
                .into_iter()
                .find(|r| r.id == inner)
                .unwrap()
                .muted
        );
        // The group's bus is silenced; what's inside plays into it.
        let bus = tonique_engine::edit::BusId(inner.0);
        assert!(!state.edit().bus(bus).unwrap().channel.audible());
        assert!(state.edit().track(a).unwrap().channel.audible());
    }

    #[test]
    fn nested_groups_round_trip_through_a_project() {
        let mut state = setup_state();
        let [a, b, c] = ["a", "b", "c"].map(|n| track(&mut state, n));
        let inner = state.group(&[a]).unwrap();
        rename(&mut state, inner, "in");
        let outer = state.group(&[inner, b]).unwrap();
        rename(&mut state, outer, "out");
        state.toggle_solo(inner, false);
        state.set_mute(outer, true);
        state.track_view_mut(&inner).collapsed = true;
        let _ = c;

        let saved = state.project(None);
        assert_eq!(saved.groups.len(), 2);
        assert_eq!(saved.groups[0].name, "out", "parents first");
        assert_eq!(saved.groups[1].parent, Some(0));
        let json = serde_json::to_string(&saved).unwrap();

        let mut loaded = setup_state();
        let file = serde_json::from_str(&json).unwrap();
        assert!(loaded.load_project(&file, Path::new("/")).is_empty());
        assert_eq!(loaded.project(None), saved);
        assert_eq!(tree(&loaded), ["out", "  in", "  b", "c"]);
        assert_tree_order(&loaded);
    }

    #[test]
    fn projects_without_groups_still_open() {
        // Version 1: no `groups`, no `group` on tracks.
        let v1 = r##"{
            "version": 1, "bpm": 120.0, "loop_range": [0.0, 16.0], "looping": false,
            "master": {"volume": 1.0, "pan": 0.0, "muted": false},
            "tracks": [{"name": "a", "color": "#ffffff", "height": 60.0, "closed": false,
                        "soloed": false, "volume": 1.0, "pan": 0.0, "muted": false, "clips": []}]
        }"##;
        let mut state = setup_state();
        let file = serde_json::from_str(v1).unwrap();
        assert!(state.load_project(&file, Path::new("/")).is_empty());
        assert_eq!(tree(&state), ["a"]);
    }

    #[test]
    fn projects_with_old_keys_still_open() {
        // Before `collapsed`: `folded` on groups, `closed` on tracks. Before
        // `filter`: `equalizer`.
        let old = r##"{
            "version": 1, "bpm": 120.0, "loop_range": [0.0, 16.0], "looping": false,
            "master": {"volume": 1.0, "pan": 0.0, "muted": false},
            "groups": [{"name": "g", "color": "#ffffff", "height": 60.0, "folded": true,
                        "soloed": false, "parent": null, "volume": 1.0, "pan": 0.0,
                        "muted": false}],
            "tracks": [{"name": "a", "color": "#ffffff", "height": 60.0, "closed": true,
                        "soloed": false, "group": 0, "volume": 1.0, "pan": 0.0,
                        "muted": false, "clips": [],
                        "effects": [{"kind": "equalizer", "enabled": true, "params": {}}]}]
        }"##;
        let file: crate::core::project::ProjectFile = serde_json::from_str(old).unwrap();
        assert!(file.groups[0].collapsed);
        assert!(file.tracks[0].collapsed);
        assert_eq!(file.tracks[0].channel.effects[0].kind, EffectKind::Filter);
    }

    #[test]
    fn collapsing_keeps_the_expanded_height() {
        let mut state = setup_state();
        let a = track(&mut state, "a");
        let g = state.group(&[a]).unwrap();
        state.track_view_mut(&g).height = 120.;
        state.set_collapsed(&g, true);
        assert_eq!(
            state.track_view_mut(&g).height,
            crate::core::track::TRACK_COLLAPSED_HEIGHT
        );
        state.set_collapsed(&g, false);
        assert_eq!(state.track_view_mut(&g).height, 120.);
    }

    #[test]
    fn new_tracks_join_the_group_around_them() {
        let mut state = setup_state();
        let [a, b, _] = ["a", "b", "c"].map(|n| track(&mut state, n));
        let g = state.group(&[a, b]).unwrap();
        let between = state.add_track_at(1);
        assert_eq!(state.parent(between), Some(g));
        let after = state.add_track_at(2 + 1);
        assert_eq!(state.parent(after), None, "between the group and c");
        assert_tree_order(&state);
    }
}

/// Without an output device the project still opens, stopped, and says why
/// while it keeps trying to open one.
#[test]
fn starts_without_audio_and_waits_for_a_device() {
    let mut state = setup_state();
    state.play();
    state.attach_audio(Err("no device".into()), Settings::default());
    assert!(state.audio().is_none());
    assert!(state.audio_lost());
    assert_eq!(state.audio_error.as_deref(), Some("no device"));
    assert_eq!(state.playback_state(), PlaybackState::Paused);
}
