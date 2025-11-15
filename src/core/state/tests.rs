use crossbeam::channel::unbounded;

use crate::core::{state::ToniqueProjectState, track::TrackCore};

fn setup_state() -> ToniqueProjectState {
    let (tx, _) = rtrb::RingBuffer::new(128);
    let (_, rx) = unbounded();
    ToniqueProjectState::new(tx, rx)
}

#[test]
fn test_add_track() {
    let mut state = setup_state();
    let track = TrackCore::new();

    state.add_track(track.clone());
    assert_eq!(state.track_len(), 1);
    assert_eq!(state.tracks().next().unwrap().id, track.id);
}

#[test]
fn test_add_track_at() {
    let mut state = setup_state();
    let track1 = TrackCore::new();

    state.add_track_at(track1.clone(), 0);
    assert_eq!(state.track_len(), 1);
    assert_eq!(state.tracks().nth(0).unwrap().id, track1.id);

    let track2 = TrackCore::new();

    state.add_track_at(track2.clone(), 0);
    assert_eq!(state.track_len(), 2);
    assert_eq!(state.tracks().nth(0).unwrap().id, track2.id);

    let track3 = TrackCore::new();

    state.add_track_at(track3.clone(), 2);
    assert_eq!(state.track_len(), 3);
    assert_eq!(state.tracks().nth(2).unwrap().id, track3.id);
}

#[test]
fn test_delete_track() {
    let mut state = setup_state();
    let track1 = TrackCore::new();
    let track2 = TrackCore::new();
    state.add_track(track1.clone());
    state.add_track(track2.clone());
    assert_eq!(state.track_len(), 2);

    state.delete_track(&track1.id);
    // Should be deleted after state update
    assert_eq!(state.track_len(), 2);
    state.update(0.1);
    assert_eq!(state.track_len(), 1);
    state.delete_track(&track2.id);
    state.update(0.2);
    assert_eq!(state.track_len(), 0);
    // deleting a non existant track should no raise errors
    state.delete_track(&"invalid_id".to_string());
}
