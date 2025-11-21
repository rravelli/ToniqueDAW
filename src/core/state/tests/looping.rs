use crate::core::state::{
    looping::{LoopState, MIN_LOOP_SIZE},
    tests::setup_state,
};

#[test]
fn test_loop_state() {
    let mut state = setup_state();
    let loop_state = LoopState {
        enabled: true,
        start: 11.,
        end: 89.,
    };
    state.loop_state = loop_state.clone();
    assert_eq!(*state.loop_state(), loop_state);
}

#[test]
fn test_set_loop_start() {
    let mut state = setup_state();
    state.loop_state = LoopState {
        enabled: true,
        start: 11.,
        end: 89.,
    };
    state.set_loop_start(30.);
    assert_eq!(state.loop_state.start, 30.);

    state.set_loop_start(100.);
    assert_eq!(state.loop_state.start, state.loop_state.end - MIN_LOOP_SIZE);

    state.set_loop_start(state.loop_state.end - MIN_LOOP_SIZE / 2.0);
    assert_eq!(state.loop_state.start, state.loop_state.end - MIN_LOOP_SIZE);
}

#[test]
fn test_set_loop_end() {
    let mut state = setup_state();
    state.loop_state = LoopState {
        enabled: true,
        start: 11.,
        end: 89.,
    };
    state.set_loop_end(40.);
    assert_eq!(state.loop_state.end, 40.);

    state.set_loop_end(10.);
    assert_eq!(state.loop_state.end, state.loop_state.start + MIN_LOOP_SIZE);

    state.set_loop_end(state.loop_state.start + MIN_LOOP_SIZE / 2.0);
    assert_eq!(state.loop_state.end, state.loop_state.start + MIN_LOOP_SIZE);
}

#[test]
fn test_toggle_loop() {
    let mut state = setup_state();
    state.loop_state = LoopState {
        enabled: true,
        start: 11.,
        end: 89.,
    };
    state.toggle_loop();
    assert_eq!(state.loop_state.enabled, false);

    state.toggle_loop();
    assert_eq!(state.loop_state.enabled, true);
}
