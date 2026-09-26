use crate::{
    analysis::AudioInfo,
    core::{clip::ClipCore, state::ToniqueProjectState},
    ui::effects::EffectId,
};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};
use tonique_engine::{
    edit::{ClipId, TrackId},
    engine::{Engine, EngineConfig},
};

fn setup_state() -> ToniqueProjectState {
    let (engine, _processor) = Engine::new(EngineConfig::default());
    ToniqueProjectState::new(engine)
}

/// A decoded (silent) file of `seconds`. At 120 bpm that's `2 * seconds` beats.
fn audio(seconds: f32) -> AudioInfo {
    AudioInfo {
        name: "test.wav".into(),
        duration: Some(Duration::from_secs_f32(seconds)),
        data: Arc::new(RwLock::new((vec![0.; 480], vec![0.; 480]))),
        ready: Arc::new(RwLock::new(true)),
        sample_rate: 48000,
        channels: 2,
        bit_depth: None,
        num_samples: None,
        path: PathBuf::from(format!("test-{seconds}.wav")),
    }
}

fn add_clip(
    state: &mut ToniqueProjectState,
    track: TrackId,
    position: f32,
    seconds: f32,
) -> ClipId {
    let id = state.new_clip_id();
    state.add_clips(&track, vec![ClipCore::new(id, audio(seconds), position)]);
    id
}

/// (position, end) of each clip on the track, sorted.
fn spans(state: &ToniqueProjectState, track: TrackId) -> Vec<(f32, f32)> {
    let bpm = state.bpm();
    let track = state.tracks().find(|t| t.id == track).unwrap();
    let mut spans: Vec<_> = track
        .clips
        .iter()
        .map(|c| {
            let round = |x: f32| (x * 1000.).round() / 1000.;
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
    assert_eq!(state.track_len(), 1);
    assert_eq!(state.tracks().next().unwrap().id, track);
}

#[test]
fn test_add_track_at() {
    let mut state = setup_state();
    let track1 = state.add_track_at(0);
    assert_eq!(state.tracks().nth(0).unwrap().id, track1);

    let track2 = state.add_track_at(0);
    assert_eq!(state.track_len(), 2);
    assert_eq!(state.tracks().nth(0).unwrap().id, track2);

    let track3 = state.add_track_at(2);
    assert_eq!(state.track_len(), 3);
    assert_eq!(state.tracks().nth(2).unwrap().id, track3);
}

#[test]
fn test_delete_track() {
    let mut state = setup_state();
    let track1 = state.add_track();
    let track2 = state.add_track();
    assert_eq!(state.track_len(), 2);

    state.delete_track(&track1);
    // Should be deleted after state update
    assert_eq!(state.track_len(), 2);
    state.update();
    assert_eq!(state.track_len(), 1);
    state.delete_track(&track2);
    state.update();
    assert_eq!(state.track_len(), 0);
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

    state.move_clip(&clip, &b, 8., &[]);
    assert!(spans(&state, a).is_empty());
    assert_eq!(spans(&state, b), [(8., 12.)]);

    state.commit_resize_clip(&clip, 0.25, 1., 9.); // drop the first beat
    assert_eq!(spans(&state, b), [(9., 12.)]);

    state.cut_clip_at(&b, 10.);
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
    assert_eq!(state.track_len(), 1);
    state.undo();
    assert_eq!(state.track_len(), 0);
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
    let soloed = |s: &ToniqueProjectState| {
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
    state.track_mut(&track).name = "Drums".into();
    state.commit_track_mut(&track);
    assert_eq!(state.tracks().next().unwrap().name, "Drums");
    state.undo();
    assert_eq!(state.tracks().next().unwrap().name, "# Audio Track");
    assert_eq!(state.track_mut(&track).name, "# Audio Track");
}

#[test]
fn effect_editors_follow_the_plugin_chain() {
    let mut state = setup_state();
    let track = state.add_track();
    state.add_effect(&track, EffectId::Equalizer, 0);
    state.add_effect(&track, EffectId::Equalizer, 1);
    assert_eq!(state.effects_mut(&track).unwrap().len(), 2);

    state.remove_effects(&track, &[0]);
    assert_eq!(state.effects_mut(&track).unwrap().len(), 1);
    state.undo();
    assert_eq!(state.effects_mut(&track).unwrap().len(), 2);

    // Power button: bypass in the engine, undoable.
    state.effects_mut(&track).unwrap()[0].toggle();
    state.update();
    assert!(!state.effects_mut(&track).unwrap()[0].enabled);
    state.undo();
    state.update();
    assert!(state.effects_mut(&track).unwrap()[0].enabled);

    state.duplicate_track(&track);
    let copy = state.tracks().nth(1).unwrap().id;
    assert_eq!(state.effects_mut(&copy).unwrap().len(), 2);
}

#[test]
fn clips_play_through_the_engine_once_loaded() {
    use tonique_engine::engine::render_offline;

    let (engine, mut processor) = Engine::new(EngineConfig {
        sample_rate: 48000.,
        housekeeping_thread: false,
        ..Default::default()
    });
    let mut state = ToniqueProjectState::new(engine);
    let track = state.add_track();
    // One second of a loud square wave, decoded at another rate so it's resampled.
    let mut info = audio(1.);
    let wave: Vec<f32> = (0..44100)
        .map(|i| if i / 50 % 2 == 0 { 0.5 } else { -0.5 })
        .collect();
    info.data = Arc::new(RwLock::new((wave.clone(), wave)));
    info.sample_rate = 44100;
    let id = state.new_clip_id();
    state.add_clips(&track, vec![ClipCore::new(id, info, 0.)]);
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
    let [left, _] = state.metrics.tracks[&track].get_peak();
    assert!(left > 0.4, "track meter shows {left}");
    assert!(state.playhead() > 0.);
}

#[test]
fn edit_cursor_moves_the_playhead_only_when_stopped() {
    let mut state = setup_state();
    state.set_edit_cursor(4.);
    assert_eq!((state.edit_cursor(), state.playhead()), (4., 4.));

    state.play();
    state.set_edit_cursor(8.);
    assert_eq!((state.edit_cursor(), state.playhead()), (8., 4.));

    // Stopping returns to the edit cursor.
    state.stop();
    assert_eq!(state.playhead(), 8.);
}

#[test]
fn seeking_moves_both_cursors() {
    let mut state = setup_state();
    state.play();
    state.seek(2.);
    assert_eq!((state.edit_cursor(), state.playhead()), (2., 2.));
    state.seek(-1.);
    assert_eq!((state.edit_cursor(), state.playhead()), (0., 0.));
}

#[test]
fn loop_range_is_ordered_and_never_empty() {
    let mut state = setup_state();
    state.set_loop_range(8., 4.);
    assert_eq!(state.loop_range(), (4., 8.));
    state.set_loop_range(-2., 1.);
    assert_eq!(state.loop_range(), (0., 1.));
    state.set_loop_range(3., 3.);
    assert_eq!(state.loop_range(), (0., 1.));
}

#[test]
fn playback_wraps_inside_the_loop_after_a_tempo_change() {
    use tonique_engine::engine::render_offline;

    let (engine, mut processor) = Engine::new(EngineConfig {
        sample_rate: 48000.,
        housekeeping_thread: false,
        ..Default::default()
    });
    let mut state = ToniqueProjectState::new(engine);
    state.set_loop_range(0., 1.);
    state.set_looping(true);
    // The loop was sent in samples at 120 bpm: it must follow the new tempo.
    state.set_bpm(60.);
    state.play();

    // Two seconds, with the loop being one second long at 60 bpm.
    let mut furthest = 0.0f32;
    for _ in 0..(2 * 48000 / 256) {
        state.update();
        render_offline(&mut processor, 256, 2);
        state.update();
        furthest = furthest.max(state.playhead());
        assert!(state.playhead() < 1., "played past the loop: {}", state.playhead());
    }
    assert!(furthest > 0.9, "never reached the loop end ({furthest})");
}
