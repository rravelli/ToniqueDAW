//! The timeline's menus: the grid's settings (also the transport bar's),
//! and a clip's.

use crate::arrangement::{Arrangement, ArrangementEvent};
use crate::model::HoldsProject;
use gpui::Context;
use tonique_engine::edit::ClipId;

use crate::{
    icons,
    widgets::{Menu, MenuItem},
};
use tonique_core::core::grid::{GridDensity, GridStep};

impl Arrangement {
    /// Adaptive densities, fixed steps, triplets and snapping; the chosen
    /// ones ticked, as `grid_menu` in the egui app.
    pub fn grid_menu(&self, context: &mut Context<Self>) -> gpui::Div {
        let settings = self.state(context).grid.settings;
        let mut menu = Menu::new(&crate::theme::current(context)).label("Adaptive");
        for density in GridDensity::ALL {
            let checked = settings.adaptive && settings.density == density;
            menu = menu.item(
                MenuItem::new(density.name()).checked(checked),
                context,
                move |this, _, context| {
                    let settings = &mut this.state_mut(context).grid.settings;
                    settings.adaptive = true;
                    settings.density = density;
                },
            );
        }
        menu = menu.separator().label("Fixed");
        for step in GridStep::ALL {
            let checked = !settings.adaptive && settings.step == step;
            menu = menu.item(
                MenuItem::new(step.name(settings.triplet)).checked(checked),
                context,
                move |this, _, context| {
                    let settings = &mut this.state_mut(context).grid.settings;
                    settings.adaptive = false;
                    settings.step = step;
                },
            );
        }
        menu.separator()
            .item(
                MenuItem::new("Triplets")
                    .checked(settings.triplet)
                    .keep_open(),
                context,
                |this, _, context| this.state_mut(context).grid.settings.triplet ^= true,
            )
            .item(
                MenuItem::new("Snap to grid")
                    .checked(settings.snap)
                    .keep_open(),
                context,
                |this, _, context| this.state_mut(context).grid.settings.snap ^= true,
            )
            .into_element()
    }

    /// A clip's menu, while it exists.
    pub fn clip_menu(&self, id: ClipId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let (_, clip) = self.state(context).clip_with_track(id)?;
        let theme = &*crate::theme::current(context);
        let mut menu = Menu::new(theme).label(clip.audio.name.clone());
        if clip.audio.missing {
            let path = clip.audio.path.clone();
            menu = menu.item(
                MenuItem::new("Locate file…").icon(icons::MAGNIFYING_GLASS),
                context,
                move |_, _, context| {
                    context.emit(ArrangementEvent::ShowMissingFiles(Some(path.clone())));
                },
            );
        }
        Some(
            menu.item(
                MenuItem::new("Delete")
                    .icon(icons::TRASH)
                    .color(theme.danger),
                context,
                move |this, _, context| this.state_mut(context).delete_clips(&[id]),
            )
            .into_element(),
        )
    }
}
