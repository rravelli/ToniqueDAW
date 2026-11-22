//! The bottom panel, as the egui app's `BottomPanel`: the selected track's
//! effects side by side ([`rack`]), or the details of one of its clips
//! ([`clip`]), under a header in the track's colour whose tabs switch
//! between the two ([`header`]). Resizable from its top edge, it spans the
//! window under the browser and the timeline. While it was the last
//! clicked and shows effects, Delete, Duplicate, Rename and Select all are
//! for them.

mod clip;
mod header;
pub mod paint;
mod rack;

use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, MouseDownEvent, MouseMoveEvent, Pixels, Window, canvas, div,
    prelude::*, px,
};
use tonique_engine::edit::ClipId;

pub use clip::ClipArrows;
pub use rack::{DraggedEffect, KnobArrows};

use crate::{
    app::Tonique,
    config::keymap::Action,
    theme::{Theme, hsla, size},
    widgets::{Edge, Splitter, splitter_handle},
};
use clip::ClipDetails;
use rack::Rack;

/// Shortest the panel gets: the clip inspector's tallest panel shows whole.
const MIN_HEIGHT: f32 = 220.;
const MAX_HEIGHT: f32 = 400.;

/// What the panel shows.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BottomView {
    /// The selected track's effects.
    #[default]
    Effects,
    /// Details of this clip, while it exists.
    Clip(ClipId),
}

pub struct BottomPanel {
    pub open: bool,
    pub view: BottomView,
    height: Splitter,
    rack: Rack,
    clip: ClipDetails,
    /// The last click was in the panel: the effects take the editing keys.
    focused: bool,
    /// The panel, as last laid out.
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl BottomPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            view: BottomView::Effects,
            height: Splitter::new(Edge::Top, MIN_HEIGHT, MIN_HEIGHT, MAX_HEIGHT),
            rack: Rack::default(),
            clip: ClipDetails::new(),
            focused: false,
            bounds: Rc::default(),
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// What referred to a replaced project's tracks, clips and effects.
    pub fn forget_project(&mut self) {
        self.view = BottomView::Effects;
        self.rack.forget();
        self.clip.forget();
    }

    pub fn is_dragging(&self) -> bool {
        self.height.is_dragging() || self.rack.is_dragging() || self.clip.is_dragging()
    }

    /// Whether it shows something that moves on its own: meters, spectra.
    pub fn is_animating(&self) -> bool {
        self.open && self.rack.is_animating()
    }
}

impl Tonique {
    /// Opens the panel on the details of clip `id`, and selects its track.
    pub fn show_clip_details(&mut self, id: ClipId) {
        if let Some((track, _)) = self.state.find_clip(id) {
            self.state.select_track(&track);
            self.bottom.view = BottomView::Clip(id);
            self.bottom.open = true;
        }
    }

    /// Opens the panel on the selected track's effects; or, when it shows
    /// them already for that track, closes it. As a double-click on a track
    /// header.
    pub fn toggle_effects_of_selected(&mut self, was_selected: bool) {
        let showing_effects = self.bottom.view == BottomView::Effects;
        self.bottom.open = !(was_selected && showing_effects && self.bottom.open);
        self.bottom.view = BottomView::Effects;
    }

    fn show_bottom_view(&mut self, view: BottomView) {
        match view {
            BottomView::Clip(id) => self.show_clip_details(id),
            BottomView::Effects => self.bottom.view = BottomView::Effects,
        }
    }

    /// While the panel shows a clip, show the one selected instead, when
    /// exactly one is; and the track's effects once the clip is gone.
    fn follow_clip_selection(&mut self) {
        let BottomView::Clip(shown) = self.bottom.view else {
            return;
        };
        if let [selected] = self.state.selected_clips()
            && *selected != shown
            && self.state.find_clip(*selected).is_some()
        {
            self.bottom.view = BottomView::Clip(*selected);
        } else if self.state.find_clip(shown).is_none() {
            self.bottom.view = BottomView::Effects;
        }
    }

    /// A press anywhere in the window: whether it's in the panel decides
    /// where the editing keys go.
    pub fn bottom_press(&mut self, event: &MouseDownEvent) {
        self.bottom.focused =
            self.bottom.open && self.bottom.bounds.get().contains(&event.position);
    }

    /// Runs `action` on the effects, when they take it: the panel shows
    /// them and was the last clicked. Returns whether it did.
    pub fn bottom_action(
        &mut self,
        action: Action,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> bool {
        let takes = matches!(
            action,
            Action::Delete | Action::Duplicate | Action::Rename | Action::SelectAll
        );
        if !takes || !self.bottom.open || !self.bottom.focused {
            return false;
        }
        // Without effects shown (a clip is), they're for the timeline.
        if self.bottom.view != BottomView::Effects {
            return false;
        }
        let Some(track) = self.state.selected_track() else {
            return false;
        };
        self.rack_action(track.id, action, window, context);
        true
    }

    /// Follows a drag in the panel. Returns whether it's one.
    pub fn bottom_drag_move(&mut self, event: &MouseMoveEvent, window_height: f32) -> bool {
        // The timeline keeps some room above.
        if self
            .bottom
            .height
            .drag_to(f32::from(event.position.y), window_height / 2.)
            .is_some()
        {
            return true;
        }
        self.rack_drag_move(event) | self.clip_drag_move(event)
    }

    pub fn bottom_drag_end(&mut self) {
        self.bottom.height.end();
        self.rack_drag_end();
        self.clip_drag_end();
    }

    /// Whether the pointer is over the panel, which then redraws as it
    /// moves: its graphs and handles show what's under it.
    pub fn bottom_hover(&self, event: &MouseMoveEvent) -> bool {
        self.bottom.open && self.bottom.bounds.get().contains(&event.position)
    }

    pub fn bottom_panel_view(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.bottom.open {
            self.bottom.focused = false;
            return None;
        }
        self.follow_clip_selection();
        let theme = crate::theme::current(context);
        let clip = match self.bottom.view {
            BottomView::Clip(id) => self.state.clip_with_track(id),
            BottomView::Effects => None,
        };
        let content = match clip {
            Some((track, clip)) => {
                self.bottom.rack.leave_track();
                let header = self.bottom_header(
                    &track,
                    Some(&clip),
                    Some((BottomView::Clip(clip.id), clip.id)),
                    context,
                );
                let details = self.clip_details_view(&clip, &track, window, context);
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(details)
            }
            None => match self.state.selected_track() {
                Some(track) => {
                    // A tab to the clip selected on this track, if one is.
                    let selected_clip = match self.state.selected_clips() {
                        [id] if self
                            .state
                            .find_clip(*id)
                            .is_some_and(|(owner, _)| owner == track.id) =>
                        {
                            Some(*id)
                        }
                        _ => None,
                    };
                    let header = self.bottom_header(
                        &track,
                        None,
                        selected_clip.map(|id| (BottomView::Effects, id)),
                        context,
                    );
                    let rack = self.rack_view(&track, window, context);
                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .child(header)
                        .child(rack)
                }
                None => {
                    self.bottom.rack.leave_track();
                    div()
                        .size_full()
                        .child(hint("Select a track to see its effects", &theme))
                }
            },
        };
        let slot = self.bottom.bounds.clone();
        Some(
            div()
                .id("bottom-panel")
                .relative()
                .flex_none()
                .w_full()
                .h(px(self.bottom.height.size))
                .bg(hsla(theme.bg_panel))
                .border_t_1()
                .border_color(hsla(theme.separator))
                .child(content)
                .child(
                    canvas(move |bounds, _, _| slot.set(bounds), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .child(splitter_handle(
                    "bottom-panel-edge",
                    &self.bottom.height,
                    &theme,
                    context,
                    |this| &mut this.bottom.height,
                ))
                .into_any_element(),
        )
    }
}

/// Muted text in the middle of what it's in.
fn hint(text: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size::LABEL))
        .text_color(hsla(theme.text_muted))
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_toggles_and_forgets_its_clip() {
        let mut panel = BottomPanel::new();
        assert!(!panel.open);
        panel.toggle();
        assert!(panel.open);
        panel.view = BottomView::Clip(ClipId(3));
        panel.forget_project();
        assert_eq!(panel.view, BottomView::Effects);
    }
}
