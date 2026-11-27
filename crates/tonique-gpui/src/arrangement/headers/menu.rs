//! A track's or a group's menu, as its header's in the egui app, and
//! renaming one in place.

use crate::arrangement::Arrangement;
use crate::model::HoldsProject;
use gpui::{Context, KeyDownEvent, Window, div, prelude::*, px};
use tonique_engine::edit::TrackId;

use crate::{
    app::focus_main,
    icons,
    theme::{Color, Theme, hsla},
    widgets::{FieldKey, Menu, MenuItem, TextField},
};
use tonique_core::{document::track::TrackKind, project::MASTER_TRACK_ID, utils::display_name};

/// A track's name when it's left empty, as in the egui app.
const DEFAULT_NAME: &str = "Audio Track";
const SWATCH: f32 = 14.;

impl Arrangement {
    pub fn track_menu(&self, id: TrackId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let track = self.track_row(id, context)?;
        let theme = &*crate::theme::current(context);
        let index = track.first_track_index;
        let menu = Menu::new(theme).label(display_name(&track.name, index));
        // The master can't be renamed, copied or deleted: only its colour
        // changes, as in the egui app.
        if id == MASTER_TRACK_ID {
            let swatches = self.swatches(id, track.color, theme, context);
            return Some(menu.label("Color").child(swatches).into_element());
        }
        let is_group = track.kind == TrackKind::Group;
        // The whole selection when this row is part of it.
        let selection = self.state(context).selected_tracks().clone();
        let targets = if selection.contains(&id) {
            selection
        } else {
            vec![id]
        };
        let menu = menu
            .item(
                MenuItem::new("Rename").icon(icons::TEXT_T),
                context,
                move |this, window, context| this.rename_track(id, window, context),
            )
            .item(
                MenuItem::new("Add Audio Track").icon(icons::PLUS),
                context,
                move |this, _, context| {
                    this.state_mut(context).add_track_at(index);
                },
            )
            .item(
                MenuItem::new("Group").icon(icons::FOLDER_PLUS),
                context,
                move |this, _, context| {
                    this.state_mut(context).group(&targets);
                },
            );
        let menu = if is_group {
            menu.item(
                MenuItem::new("Ungroup").icon(icons::FOLDER_MINUS),
                context,
                move |this, _, context| this.state_mut(context).ungroup(id),
            )
        } else {
            menu.item(
                MenuItem::new("Duplicate").icon(icons::COPY),
                context,
                move |this, _, context| this.state_mut(context).duplicate_track(&id),
            )
        };
        let delete = if is_group {
            "Delete group and contents"
        } else {
            "Delete"
        };
        let menu = menu
            .label("Color")
            .child(self.swatches(id, track.color, theme, context))
            .separator()
            .item(
                MenuItem::new(delete).icon(icons::TRASH).color(theme.danger),
                context,
                move |this, _, context| this.state_mut(context).delete_track(&id),
            );
        Some(menu.into_element())
    }

    /// The theme's track colours, `current` outlined; one picked recolours
    /// the track and closes the menu.
    fn swatches(
        &self,
        id: TrackId,
        current: Color,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let mut row = div().flex().flex_wrap().gap(px(3.)).px(px(6.)).py(px(3.));
        for (index, &color) in theme.palette.iter().enumerate() {
            let swatch = div()
                .id(("swatch", index))
                .size(px(SWATCH))
                .rounded(px(2.))
                .bg(hsla(color))
                .cursor_pointer()
                .border_1()
                .border_color(hsla(if color == current { theme.text } else { color }))
                .hover(|style| style.border_color(hsla(theme.text_muted)))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    context.listener(move |this, _, _, context| {
                        context.stop_propagation();
                        this.state_mut(context).track_view_mut(&id).color = color;
                        this.state_mut(context).commit_track_view(&id);
                        crate::popup::close(context);
                        context.notify();
                    }),
                );
            row = row.child(swatch);
        }
        row
    }

    /// Starts typing a new name for track `id`.
    pub fn rename_track(&mut self, id: TrackId, window: &mut Window, context: &mut Context<Self>) {
        let Some(track) = self
            .track_row(id, context)
            .filter(|_| id != MASTER_TRACK_ID)
        else {
            return;
        };
        let mut field = TextField::new(context);
        field.start(track.name.clone(), window, context);
        self.headers.rename = Some((id, field));
    }

    pub(super) fn rename_field(&self, id: TrackId) -> Option<&TextField> {
        match &self.headers.rename {
            Some((renaming, field)) if *renaming == id => Some(field),
            _ => None,
        }
    }

    /// Ends renaming: with the name typed when `keep`, else as it was.
    fn finish_rename(&mut self, keep: bool, window: &mut Window, context: &mut Context<Self>) {
        let Some((id, field)) = self.headers.rename.take() else {
            return;
        };
        if keep {
            let name = field.text.trim();
            let name = if name.is_empty() { DEFAULT_NAME } else { name };
            self.state_mut(context).track_view_mut(&id).name = name.to_string();
            self.state_mut(context).commit_track_view(&id);
        }
        focus_main(window, context);
    }

    pub(super) fn rename_view(
        &self,
        field: &TextField,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id("rename")
            .track_focus(&field.focus)
            .flex()
            .items_center()
            .size_full()
            .px(px(2.))
            .rounded(px(1.))
            .bg(hsla(theme.bg_deep))
            .text_color(hsla(theme.text))
            .on_key_down(
                context.listener(|this, event: &KeyDownEvent, window, context| {
                    let Some((_, field)) = this.headers.rename.as_mut() else {
                        return;
                    };
                    match field.key(event) {
                        FieldKey::Submit => this.finish_rename(true, window, context),
                        FieldKey::Cancel => this.finish_rename(false, window, context),
                        FieldKey::Edited => {}
                        FieldKey::Ignored => return,
                    }
                    context.stop_propagation();
                    context.notify();
                }),
            )
            .on_mouse_down(gpui::MouseButton::Left, |_, _, context| {
                context.stop_propagation()
            })
            .on_mouse_down_out(context.listener(|this, _, window, context| {
                this.finish_rename(true, window, context);
                context.notify();
            }))
            .child(field.view_focused())
    }
}
