//! A fade's menu, right-clicked on its handle or curve: its shapes, and
//! a reset. The inspector's shape pickers list the shapes alone.

use gpui::Context;
use tonique_engine::{edit::ClipId, nodes::FadeShape};

use crate::{
    app::Tonique,
    icons,
    timeline::fades::{FadeEnd, shape_name},
    widgets::{Menu, MenuItem},
};

impl Tonique {
    /// The shapes for clip `id`'s fade at `end` (its crossfade, with none),
    /// the one it has ticked; and a reset when `reset`.
    pub fn fade_menu(
        &self,
        id: ClipId,
        end: Option<FadeEnd>,
        reset: bool,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (_, clip) = self.state.clip_with_track(id)?;
        let fade = match end {
            Some(end) => end.fade(&clip.fades),
            None => clip.fades.crossfade,
        };
        let mut menu = Menu::new(&self.theme);
        for shape in FadeShape::ALL {
            menu = menu.item(
                MenuItem::new(shape_name(shape)).checked(fade.curve.shape == shape),
                context,
                move |this, _, _| this.edit_fade_curve(id, end, |fade, _| fade.curve.shape = shape),
            );
        }
        if reset {
            menu = menu.separator().item(
                MenuItem::new("Reset").icon(icons::ARROW_COUNTER_CLOCKWISE),
                context,
                move |this, _, _| this.edit_fade_curve(id, end, |fade, default| *fade = default),
            );
        }
        Some(menu.into_element())
    }
}
