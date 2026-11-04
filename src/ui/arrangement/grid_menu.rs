//! The grid's settings as menu items, from the transport bar and the
//! timeline's right-click menu.

use egui::Ui;

use crate::{
    core::{
        grid::{GridDensity, GridStep},
        state::ProjectState,
    },
    ui::widget::context_menu::{ContextMenuButton, ContextMenuLabel, ContextMenuSeparator},
};

/// Adaptive densities, fixed steps, triplets and snapping; the chosen ones
/// ticked.
pub fn grid_menu(ui: &mut Ui, state: &mut ProjectState) {
    let settings = &mut state.grid.settings;
    ui.vertical(|ui| {
        ui.add(ContextMenuLabel::new("Adaptive"));
        for density in GridDensity::ALL {
            let checked = settings.adaptive && settings.density == density;
            if ui
                .add(ContextMenuButton::new("", density.name()).checked(checked))
                .clicked()
            {
                settings.adaptive = true;
                settings.density = density;
                ui.close();
            }
        }
        ui.add(ContextMenuSeparator::new());
        ui.add(ContextMenuLabel::new("Fixed"));
        for step in GridStep::ALL {
            let checked = !settings.adaptive && settings.step == step;
            if ui
                .add(ContextMenuButton::new("", &step.name(settings.triplet)).checked(checked))
                .clicked()
            {
                settings.adaptive = false;
                settings.step = step;
                ui.close();
            }
        }
        ui.add(ContextMenuSeparator::new());
        if ui
            .add(ContextMenuButton::new("", "Triplets").checked(settings.triplet))
            .clicked()
        {
            settings.triplet ^= true;
        }
        if ui
            .add(ContextMenuButton::new("", "Snap to grid").checked(settings.snap))
            .clicked()
        {
            settings.snap ^= true;
        }
    });
}
