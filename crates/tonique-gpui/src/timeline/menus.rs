//! The timeline's menus: the grid's settings (also the transport bar's),
//! and a clip's.

use gpui::Context;
use tonique_engine::edit::ClipId;

use crate::{
    app::Tonique,
    core::grid::{GridDensity, GridStep},
    icons,
    widgets::{Menu, MenuItem},
};

impl Tonique {
    /// Adaptive densities, fixed steps, triplets and snapping; the chosen
    /// ones ticked, as `grid_menu` in the egui app.
    pub fn grid_menu(&self, context: &mut Context<Self>) -> gpui::Div {
        let settings = &self.state.grid.settings;
        let mut menu = Menu::new(&self.theme).label("Adaptive");
        for density in GridDensity::ALL {
            let checked = settings.adaptive && settings.density == density;
            menu = menu.item(
                MenuItem::new(density.name()).checked(checked),
                context,
                move |this, _, _| {
                    let settings = &mut this.state.grid.settings;
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
                move |this, _, _| {
                    let settings = &mut this.state.grid.settings;
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
                |this, _, _| this.state.grid.settings.triplet ^= true,
            )
            .item(
                MenuItem::new("Snap to grid")
                    .checked(settings.snap)
                    .keep_open(),
                context,
                |this, _, _| this.state.grid.settings.snap ^= true,
            )
            .into_element()
    }

    /// A clip's menu, while it exists.
    pub fn clip_menu(&self, id: ClipId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let (_, clip) = self.state.clip_with_track(id)?;
        let theme = &self.theme;
        let mut menu = Menu::new(theme).label(clip.audio.name.clone());
        if clip.audio.missing {
            let path = clip.audio.path.clone();
            menu = menu.item(
                MenuItem::new("Locate file…").icon(icons::MAGNIFYING_GLASS),
                context,
                move |this, _, _| this.show_missing_files(Some(path.clone())),
            );
        }
        Some(
            menu.item(
                MenuItem::new("Delete")
                    .icon(icons::TRASH)
                    .color(theme.danger),
                context,
                move |this, _, _| this.state.delete_clips(&[id]),
            )
            .into_element(),
        )
    }
}
