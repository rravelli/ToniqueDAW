//! Clips moved, copied and trimmed by dragging ([`crate::project::clip_drag`]).

use super::*;
use crate::{
    project::{
        SelectionBounds,
        clip_drag::{ClipEdge, ClipMove, ClipTrim},
    },
    session::{grid::GridStep, timeline_view::TimelineView},
};

/// Snapping to a fixed grid of bars (4 beats), whatever the zoom; at the
/// default zoom, edges snap to targets within 0.8 beats.
fn state_on_bars() -> ProjectState {
    let mut state = setup_state();
    let settings = &mut state.grid_mut().settings;
    settings.adaptive = false;
    settings.step = GridStep::Bar;
    state
}

const NO_SNAP: bool = true;
const SNAP: bool = false;

#[test]
fn grabbing_a_clip_outside_the_selection_takes_it_alone() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.);
    let b = add_clip(&mut state, track, 4., 1.);
    let c = add_clip(&mut state, track, 8., 1.);
    state.select_clips(vec![a, b]);

    let moving = ClipMove::start(&mut state, c, BeatPos(9.), false);
    assert_eq!(moving.ids(), vec![c]);
    assert_eq!(state.selected_clips(), &[c]);

    let moving = ClipMove::start(&mut state, c, BeatPos(9.), false);
    assert_eq!(moving.ids(), vec![c], "grabbing the selection takes it all");
}

#[test]
fn moved_clips_keep_their_tracks_apart_and_stay_below_the_first() {
    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 1.);
    let b = add_clip(&mut state, t1, 4., 1.);
    state.select_clips(vec![a, b]);
    let view = TimelineView::new();

    // `b` grabbed and pulled 8 beats right onto the first track: `a`,
    // a track above it, would go above the first.
    let mut moving = ClipMove::start(&mut state, b, BeatPos(5.), false);
    moving.place(&state, &view, BeatPos(13.), 0, NO_SNAP);
    let placed: Vec<_> = moving
        .clips
        .iter()
        .map(|moved| (moved.clip.position.0, moved.track_index))
        .collect();
    assert_eq!(placed, vec![(8., 0), (12., 1)]);
    assert_eq!(moving.added_tracks(state.track_count()), 0);
}

#[test]
fn moved_clips_never_start_before_the_first_beat() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 2., 1.);
    let mut moving = ClipMove::start(&mut state, a, BeatPos(2.), false);

    moving.place(&state, &TimelineView::new(), BeatPos(-5.), 0, NO_SNAP);
    assert_eq!(moving.clips[0].clip.position, BeatPos(0.));
    assert_eq!(moving.snapped_to, None);
}

#[test]
fn the_nearest_edge_snaps_starts_to_the_grid_ends_to_targets_only() {
    let mut state = state_on_bars();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 0.6); // beats 0..1.2
    add_clip(&mut state, t1, 20., 1.); // its start is a target
    let view = TimelineView::new();
    let mut moving = ClipMove::start(&mut state, a, BeatPos(0.), false);

    // Its end 0.2 beats before the other clip's start: the start is 1.4
    // from the grid's bar at 20.
    moving.place(&state, &view, BeatPos(18.6), 0, SNAP);
    assert!((moving.clips[0].clip.position.0 - 18.8).abs() < 1e-6);
    assert_eq!(moving.snapped_to, Some(BeatPos(20.)));

    // Far from any target, its end doesn't snap to the grid: its start does.
    moving.place(&state, &view, BeatPos(10.6), 0, SNAP);
    assert_eq!(moving.clips[0].clip.position, BeatPos(12.));
    assert_eq!(moving.snapped_to, None);

    // Alt: no snapping at all.
    moving.place(&state, &view, BeatPos(18.6), 0, NO_SNAP);
    assert_eq!(moving.clips[0].clip.position, BeatPos(18.6));
}

#[test]
fn copies_snap_to_their_originals() {
    let mut state = state_on_bars();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 0.6); // beats 0..1.2
    let mut moving = ClipMove::start(&mut state, a, BeatPos(0.), true);
    assert!(moving.left_behind().is_empty());

    // Its start 0.1 beat past the original's end.
    moving.place(&state, &TimelineView::new(), BeatPos(1.3), 0, SNAP);
    assert!((moving.clips[0].clip.position.0 - 1.2).abs() < 1e-6);
}

#[test]
fn landing_moves_the_clips_as_one_undo_step() {
    let mut state = setup_state();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 1.);
    let mut moving = ClipMove::start(&mut state, a, BeatPos(0.), false);
    moving.place(&state, &TimelineView::new(), BeatPos(6.), 1, NO_SNAP);

    moving.land(&mut state);
    assert_eq!(spans(&state, t0), vec![]);
    assert_eq!(spans(&state, t1), vec![(6., 8.)]);
    assert_eq!(state.selected_clips(), &[a]);

    state.undo();
    assert_eq!(spans(&state, t0), vec![(0., 2.)]);
    assert_eq!(spans(&state, t1), vec![]);
}

#[test]
fn landing_copies_selects_them_and_leaves_the_originals() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.);
    let mut moving = ClipMove::start(&mut state, a, BeatPos(0.), true);
    moving.place(&state, &TimelineView::new(), BeatPos(4.), 0, NO_SNAP);

    moving.land(&mut state);
    assert_eq!(spans(&state, track), vec![(0., 2.), (4., 6.)]);
    assert_eq!(state.selected_clips().len(), 1);
    assert_ne!(state.selected_clips(), &[a]);

    state.undo();
    assert_eq!(spans(&state, track), vec![(0., 2.)]);
}

#[test]
fn landing_below_the_rows_adds_tracks() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 1.);
    let mut moving = ClipMove::start(&mut state, a, BeatPos(0.), false);

    moving.place(&state, &TimelineView::new(), BeatPos(0.), 2, NO_SNAP);
    assert_eq!(moving.added_tracks(state.track_count()), 2);

    moving.land(&mut state);
    assert_eq!(state.track_count(), 3);
    let last = state.track_from_index(2).unwrap();
    assert_eq!(
        last.clips.iter().map(|clip| clip.id).collect::<Vec<_>>(),
        vec![a]
    );
}

#[test]
fn with_a_zone_only_its_part_moves() {
    let mut state = setup_state();
    let track = state.add_track();
    let a = add_clip(&mut state, track, 0., 2.); // beats 0..4
    state.select_in_bounds(SelectionBounds::between((0, BeatPos(1.)), (0, BeatPos(3.))));
    let mut moving = ClipMove::start(&mut state, a, BeatPos(2.), false);
    assert_eq!(moving.clips[0].clip.position, BeatPos(1.));

    moving.place(&state, &TimelineView::new(), BeatPos(10.), 0, NO_SNAP);
    moving.land(&mut state);
    assert_eq!(spans(&state, track), vec![(0., 1.), (3., 4.), (9., 11.)]);
}

#[test]
fn trimming_snaps_the_edge_to_other_clips() {
    let mut state = state_on_bars();
    let (t0, t1) = (state.add_track(), state.add_track());
    let a = add_clip(&mut state, t0, 0., 4.); // beats 0..8
    add_clip(&mut state, t1, 5., 1.); // its start is a target
    let original = state.find_clip(a).unwrap().1;
    let view = TimelineView::new();
    let bpm = state.bpm();

    let end = ClipTrim::new(original.clone(), ClipEdge::End);
    assert_eq!(
        end.at(&state, &view, BeatPos(5.5), false, SNAP).end(bpm),
        BeatPos(5.)
    );
    assert_eq!(
        end.at(&state, &view, BeatPos(5.5), false, NO_SNAP).end(bpm),
        BeatPos(5.5)
    );
    // Far from any target: to the grid's bar.
    assert_eq!(
        end.at(&state, &view, BeatPos(2.5), false, SNAP).end(bpm),
        BeatPos(4.)
    );

    let start = ClipTrim::new(original, ClipEdge::Start);
    let trimmed = start.at(&state, &view, BeatPos(5.3), false, SNAP);
    assert_eq!(
        (trimmed.position, trimmed.end(bpm)),
        (BeatPos(5.), BeatPos(8.))
    );
}
