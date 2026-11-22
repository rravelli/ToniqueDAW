//! The inspector's Fades panel: how long the fades are, dragged, stepped
//! or typed as the clip panel's positions, and their shapes. At an end with
//! a crossfade, what's shown is the crossfade, which the handle on its seam
//! sets: greyed out.

use gpui::{AnyElement, Context, div, prelude::*, px};

use crate::{
    app::{PopupKind, Tonique},
    bottom::clip::inspector::{CELL_GAP, Field, cell, cells, section},
    core::clip::AudioClip,
    theme::Theme,
    timeline::{FadeEnd, shape_name},
};

impl Tonique {
    pub(super) fn fades_panel(
        &self,
        shown: &AudioClip,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> AnyElement {
        let crossed = self.state.crossfades_of(shown.id);
        let (fade_in, fade_out) = shown.heard_fades(crossed, self.state.bpm());
        let (open_in, open_out) = (
            !FadeEnd::In.crossed(crossed),
            !FadeEnd::Out.crossed(crossed),
        );
        let id = shown.id;
        let shape = |this: &Self, end, name, enabled, context: &mut Context<Self>| {
            let element_id = match end {
                FadeEnd::In => "fade-in-shape",
                FadeEnd::Out => "fade-out-shape",
            };
            let picker = this.picker(
                element_id,
                name,
                PopupKind::FadeShape(id, end),
                enabled,
                0.,
                theme,
                context,
            );
            cell("Shape", picker, enabled, theme)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(section("Fades", theme))
            .child(cells(
                self.field_cell("In", Field::FadeIn, shown, open_in, theme, context),
                self.field_cell("Out", Field::FadeOut, shown, open_out, theme, context),
            ))
            .child(div().h(px(CELL_GAP)))
            .child(cells(
                shape(
                    self,
                    FadeEnd::In,
                    shape_name(fade_in.curve.shape),
                    open_in,
                    context,
                ),
                shape(
                    self,
                    FadeEnd::Out,
                    shape_name(fade_out.curve.shape),
                    open_out,
                    context,
                ),
            ))
            .into_any_element()
    }
}
