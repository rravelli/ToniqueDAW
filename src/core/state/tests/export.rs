use std::path::PathBuf;

use crate::core::{
    export::ExportStatus,
    message::GuiToPlayerMsg,
    state::tests::{setup_state, setup_state_with_channels},
};

#[test]
fn test_export_status() {
    let mut state = setup_state();
    state.export_status = ExportStatus::PROCESSING(0.8);

    assert_eq!(*state.export_status(), ExportStatus::PROCESSING(0.8));
}

#[test]
fn test_export() {
    let (mut state, mut rx, _) = setup_state_with_channels();
    state.export(PathBuf::from("/test.wav"));
    assert_eq!(*state.export_status(), ExportStatus::PROCESSING(0.));
    assert!(!rx.is_empty());
    assert!(
        rx.pop()
            .is_ok_and(|m| { matches!(m, GuiToPlayerMsg::Export(_)) })
    );
}
