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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    Anchor, App, Bounds, Context, DispatchPhase, Entity, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Window, canvas, div, prelude::*, px,
};
use tonique_engine::edit::ClipId;

pub use clip::ClipArrows;
pub use rack::{DraggedEffect, KnobArrows};

use crate::{
    app::PopupKind,
    model::{HoldsProject, Project},
    popup::{self, MenuBuilder},
    theme::{Theme, hsla, size},
    wave::WaveCache,
    widgets::{Edge, Splitter, follow_drag, splitter_handle},
};
use clip::ClipDetails;
use rack::Rack;
use tonique_core::config::keymap::Action;

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

/// Whether the panel is open, and what it shows.
#[derive(Default)]
pub struct Shown {
    pub open: bool,
    pub view: BottomView,
}

impl Shown {
    fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// The clip shown belonged to a replaced project.
    fn forget(&mut self) {
        self.view = BottomView::Effects;
    }
}

pub struct BottomPanel {
    project: Entity<Project>,
    /// The waveforms drawn, shared with the other views.
    waves: Rc<RefCell<WaveCache>>,
    pub shown: Shown,
    height: Splitter,
    rack: Rack,
    clip: ClipDetails,
    /// The last click was in the panel: the effects take the editing keys.
    focused: bool,
    /// The panel, as last laid out.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The window's height, as last drawn: the panel leaves the timeline
    /// half of it.
    window_height: f32,
}

impl HoldsProject for BottomPanel {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl BottomPanel {
    /// What its waveforms take.
    pub fn wave_memory(&self) -> crate::wave::WaveMemory {
        self.waves.borrow().memory()
    }

    pub fn new(project: Entity<Project>) -> Self {
        Self {
            project,
            waves: Rc::default(),
            window_height: 0.,
            shown: Shown::default(),
            height: Splitter::new(Edge::Top, MIN_HEIGHT, MIN_HEIGHT, MAX_HEIGHT),
            rack: Rack::default(),
            clip: ClipDetails::new(),
            focused: false,
            bounds: Rc::default(),
        }
    }

    pub fn toggle(&mut self) {
        self.shown.toggle();
    }

    /// What referred to a replaced project's tracks, clips and effects.
    pub fn forget_project(&mut self) {
        self.shown.forget();
        self.rack.forget();
        self.clip.forget();
    }

    pub fn is_dragging(&self) -> bool {
        self.height.is_dragging() || self.rack.is_dragging() || self.clip.is_dragging()
    }

    /// Whether it shows something that moves on its own: meters, spectra.
    pub fn is_animating(&self) -> bool {
        self.shown.open && self.rack.is_animating()
    }
}

impl BottomPanel {
    /// Opens the panel on the details of clip `id`, and selects its track.
    pub fn show_clip_details(&mut self, id: ClipId, context: &mut App) {
        if let Some((track, _)) = self.state(context).find_clip(id) {
            self.state_mut(context).select_track(&track);
            self.shown.view = BottomView::Clip(id);
            self.shown.open = true;
        }
    }

    /// Opens the panel on the selected track's effects; or, when it shows
    /// them already for that track, closes it. As a double-click on a track
    /// header.
    pub fn toggle_effects_of_selected(&mut self, was_selected: bool) {
        let showing_effects = self.shown.view == BottomView::Effects;
        self.shown.open = !(was_selected && showing_effects && self.shown.open);
        self.shown.view = BottomView::Effects;
    }

    fn show_bottom_view(&mut self, view: BottomView, context: &mut App) {
        match view {
            BottomView::Clip(id) => self.show_clip_details(id, context),
            BottomView::Effects => self.shown.view = BottomView::Effects,
        }
    }

    /// While the panel shows a clip, show the one selected instead, when
    /// exactly one is; and the track's effects once the clip is gone.
    fn follow_clip_selection(&mut self, context: &mut App) {
        let BottomView::Clip(shown) = self.shown.view else {
            return;
        };
        if let [selected] = self.state(context).selected_clips()
            && *selected != shown
            && self.state(context).find_clip(*selected).is_some()
        {
            self.shown.view = BottomView::Clip(*selected);
        } else if self.state(context).find_clip(shown).is_none() {
            self.shown.view = BottomView::Effects;
        }
    }

    /// A press anywhere in the window: whether it's in the panel decides
    /// where the editing keys go.
    fn bottom_press(&mut self, event: &MouseDownEvent) {
        self.focused = self.shown.open && self.bounds.get().contains(&event.position);
    }

    /// Builds one of the panel's menus each frame.
    fn menu_builder(&self, kind: PopupKind, context: &Context<Self>) -> MenuBuilder {
        popup::builder(context.weak_entity(), move |this, _, context| match kind {
            PopupKind::Effect(track, plugin) => this.effect_menu(track, plugin, context),
            PopupKind::ClipRuler(id, at) => this.clip_ruler_popup(id, at, context),
            PopupKind::ClipWarpMode(id) => this.clip_warp_menu(id, context),
            PopupKind::ClipWindow(id) => this.clip_window_menu(id, context),
            _ => None,
        })
    }

    /// Opens one of the panel's menus with its `anchor` corner at `at`; or,
    /// when it's open, closes it.
    fn toggle_popup(
        &mut self,
        kind: PopupKind,
        at: Point<Pixels>,
        anchor: Anchor,
        context: &mut Context<Self>,
    ) {
        let build = self.menu_builder(kind, context);
        popup::toggle(kind, at, anchor, build, context);
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
        if !takes || !self.shown.open || !self.focused {
            return false;
        }
        // Without effects shown (a clip is), they're for the timeline.
        if self.shown.view != BottomView::Effects {
            return false;
        }
        let Some(track) = self.state(context).selected_track() else {
            return false;
        };
        self.rack_action(track.id, action, window, context);
        true
    }

    /// Follows a drag in the panel.
    fn bottom_drag_move(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        // The timeline keeps some room above.
        let room = self.window_height / 2.;
        if self
            .height
            .drag_to(f32::from(event.position.y), room)
            .is_some()
        {
            return;
        }
        self.rack_drag_move(event, context);
        self.clip_drag_move(event, context);
    }

    fn bottom_drag_end(&mut self, _: &MouseUpEvent, context: &mut Context<Self>) {
        self.height.end();
        self.rack_drag_end(context);
        self.clip_drag_end(context);
    }
}

impl Render for BottomPanel {
    /// The panel, while open; the window shows it only then.
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        self.waves.borrow_mut().start_frame(window);
        if !self.shown.open {
            self.focused = false;
            return div().into_any_element();
        }
        // The meters, the analysers and the clip's playhead.
        self.watch_playback(context);
        self.window_height = f32::from(window.viewport_size().height);
        if self.is_animating() {
            window.request_animation_frame();
        }
        self.follow_clip_selection(context);
        let theme = crate::theme::current(context);
        let clip = match self.shown.view {
            BottomView::Clip(id) => self.state(context).clip_with_track(id),
            BottomView::Effects => None,
        };
        let content = match clip {
            Some((track, clip)) => {
                self.rack.leave_track();
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
            None => match self.state(context).selected_track() {
                Some(track) => {
                    // A tab to the clip selected on this track, if one is.
                    let selected_clip = match self.state(context).selected_clips() {
                        [id] if self
                            .state(context)
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
                    self.rack.leave_track();
                    div()
                        .size_full()
                        .child(hint("Select a track to see its effects", &theme))
                }
            },
        };
        let slot = self.bounds.clone();
        let pressed = context.weak_entity();
        let dragging = self.is_dragging();
        div()
            .id("bottom-panel")
            .relative()
            .flex_none()
            .w_full()
            .h(px(self.height.size))
            .bg(hsla(theme.bg_panel))
            .border_t_1()
            .border_color(hsla(theme.separator))
            .child(content)
            // Redraws as the pointer moves over it: its graphs and
            // handles show what's under it.
            .on_mouse_move(context.listener(|_, _, _, context| context.notify()))
            .child(
                canvas(
                    move |bounds, _, _| slot.set(bounds),
                    // Whether the last press anywhere was in the panel
                    // decides where the editing keys go.
                    move |_, _, window, _| {
                        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, context| {
                            if phase == DispatchPhase::Capture {
                                let _ = pressed.update(context, |this, _| this.bottom_press(event));
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(splitter_handle(
                "bottom-panel-edge",
                &self.height,
                &theme,
                context,
                |this| &mut this.height,
            ))
            .when(dragging, |element| {
                element.child(follow_drag(
                    context.weak_entity(),
                    Self::bottom_drag_move,
                    Self::bottom_drag_end,
                ))
            })
            .into_any_element()
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
        let mut shown = Shown::default();
        assert!(!shown.open);
        shown.toggle();
        assert!(shown.open);
        shown.view = BottomView::Clip(ClipId(3));
        shown.forget();
        assert_eq!(shown.view, BottomView::Effects);
    }
}
