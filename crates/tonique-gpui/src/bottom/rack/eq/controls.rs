//! The EQ's controls under its graph, its bands' cards in a full window,
//! and its menus.

use super::*;

impl Tonique {
    /// The picked band's dropdown, its number and shape, and its knobs.
    pub(super) fn eq_controls(
        &self,
        input: &EditorInput,
        params: &[BandParams; BANDS],
        value: BandValues,
        band: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let (track, plugin) = (input.track, input.effect.plugin.id);
        let color = band_color(theme, band, value.on && input.enabled());
        let band_kind = PopupKind::EqBand(track, plugin);
        let open = self
            .popup
            .as_ref()
            .is_some_and(|popup| popup.kind == band_kind);
        let pick = select(
            "eq-band",
            format!("Band {}", band + 1),
            BAND_WIDTH,
            band_kind,
            open,
            theme,
            context,
        )
        .h(px(ROW_HEIGHT));
        let shape = self.shape_picker(
            "eq-shape",
            value.mode,
            shape_name,
            BAND_WIDTH - NUMBER_WIDTH - 4.,
            color,
            PopupKind::EqShape(track, plugin, band),
            theme,
            context,
        );
        let left = div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(4.))
            .child(pick)
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(self.band_number(input, &params[band], band, theme, context))
                    .child(shape),
            );
        div()
            .flex()
            .flex_none()
            .gap(px(KNOB_GAP))
            .h(px(KNOB_HEIGHT))
            .child(left)
            .child(self.band_knobs(input, band, value.mode, theme, context))
    }

    /// Freq, Gain (in its place, if its shape has one) and Q knobs.
    pub(super) fn band_knobs(
        &self,
        input: &EditorInput,
        band: usize,
        mode: FilterMode,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let effect = input.effect;
        let [freq, gain, q, ..] = EQ_BANDS[band].params;
        let gain = if mode.has_gain() {
            self.param_knob(effect, gain, "Gain", format_db, false, theme, context)
                .map(IntoElement::into_any_element)
        } else {
            None
        };
        let space = || div().flex_none().w(px(KNOB_WIDTH)).into_any_element();
        div()
            .flex()
            .flex_none()
            .gap(px(KNOB_GAP))
            .h(px(KNOB_HEIGHT))
            .children(self.param_knob(effect, freq, "Freq", format_hz, true, theme, context))
            .child(gain.unwrap_or_else(space))
            .children(self.param_knob(effect, q, "Q", |q| format!("{q:.2}"), true, theme, context))
    }

    /// The band's number, filled with its colour while it's on, outlined
    /// in it while off: click to switch it.
    pub(super) fn band_number(
        &self,
        input: &EditorInput,
        params: &BandParams,
        band: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let on = params[4].get() >= 0.5;
        let color = band_color(theme, band, input.enabled());
        let (fill, ink) = if on {
            (color, theme.bg_deep)
        } else {
            (TRANSPARENT, color)
        };
        let switch = params[4].clone();
        let editor = input.editor.clone();
        div()
            .id(("eq-number", band))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(px(NUMBER_WIDTH))
            .h(px(ROW_HEIGHT))
            .rounded(px(ROW_HEIGHT / 2.))
            .bg(hsla(fill))
            .border_1()
            .border_color(hsla(color))
            .text_color(hsla(ink))
            .text_size(px(size::LABEL))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child((band + 1).to_string())
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, _, context| {
                    context.stop_propagation();
                    commit(this, &[(&switch, if on { 0. } else { 1. })]);
                    if let Editor::Eq(eq) = &mut *editor.borrow_mut() {
                        eq.selected = band;
                    }
                    context.notify();
                }),
            )
    }

    /// In a full window: a card per band, side by side, with its number
    /// and shape's name, a button per shape, and its knobs. The band
    /// picked is outlined in its colour.
    pub(super) fn eq_cards(
        &self,
        input: &EditorInput,
        params: &[BandParams; BANDS],
        values: [BandValues; BANDS],
        selected: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let cards: Vec<_> = (0..BANDS)
            .map(|band| {
                self.eq_card(
                    input,
                    &params[band],
                    values[band],
                    band,
                    selected,
                    theme,
                    context,
                )
            })
            .collect();
        div()
            .id("eq-cards")
            .flex()
            .flex_none()
            .gap(px(CARD_GAP))
            .w_full()
            .h(px(CARD_HEIGHT))
            .overflow_x_scroll()
            .children(cards)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn eq_card(
        &self,
        input: &EditorInput,
        params: &BandParams,
        value: BandValues,
        band: usize,
        selected: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let color = band_color(theme, band, value.on && input.enabled());
        let (fill, outline, width) = if band == selected {
            (theme.bg_control_hover, color, 1.5)
        } else {
            (theme.bg_control, theme.border, 1.)
        };
        let ink = if value.on {
            theme.text
        } else {
            theme.text_muted
        };
        let (shape, on) = (params[3].clone(), params[4].clone());
        let editor = input.editor.clone();
        let shapes = shape_row(
            ("eq-card-shapes", band),
            &SHAPES,
            value.mode,
            color,
            theme,
            context,
            move |this, mode| {
                commit(this, &[(&shape, eq_mode_value(mode)), (&on, 1.)]);
                if let Editor::Eq(eq) = &mut *editor.borrow_mut() {
                    eq.selected = band;
                }
            },
        );
        let picking = input.editor.clone();
        div()
            .id(("eq-card", band))
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(4.))
            .w(px(CARD_WIDTH))
            .h(px(CARD_HEIGHT))
            .p(px(CARD_PADDING))
            .rounded(px(6.))
            .bg(hsla(fill))
            .border(px(width))
            .border_color(hsla(outline))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(self.band_number(input, params, band, theme, context))
                    .child(
                        div()
                            .text_size(px(size::LABEL))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(hsla(ink))
                            .child(shape_name(value.mode)),
                    ),
            )
            .child(shapes)
            .child(self.band_knobs(input, band, value.mode, theme, context))
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |_, _, _, context| {
                    if let Editor::Eq(eq) = &mut *picking.borrow_mut() {
                        eq.selected = band;
                    }
                    context.notify();
                }),
            )
    }

    /// The band of the EQ `plugin` of `track`, while it exists.
    pub(super) fn eq_band(
        &self,
        track: TrackId,
        plugin: PluginId,
        band: usize,
    ) -> Option<(BandParams, BandValues)> {
        let effect = self
            .state
            .effects(&track)
            .into_iter()
            .find(|effect| effect.plugin.id == plugin)?;
        let params = band_params(&effect)?.get(band)?.clone();
        let values = BandValues::of(&params);
        Some((params, values))
    }

    /// A band's shapes, picked from its picker or, with `point`, its
    /// point's menu: then headed by the band, and ending with Remove.
    pub fn eq_shape_menu(
        &self,
        track: TrackId,
        plugin: PluginId,
        band: usize,
        point: bool,
        context: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let (params, value) = self.eq_band(track, plugin, band)?;
        let theme = &self.theme;
        let menu = if point {
            Menu::new(theme).label(format!("Band {}", band + 1))
        } else {
            Menu::new(theme)
        };
        let [_, _, _, shape, on] = params;
        let switch = on.clone();
        let menu = shape_menu(
            menu,
            &SHAPES,
            value.mode,
            shape_name,
            band_color(theme, band, true),
            context,
            move |this, mode| commit(this, &[(&shape, eq_mode_value(mode)), (&switch, 1.)]),
        );
        let menu = if point {
            menu.separator().item(
                MenuItem::new("Remove")
                    .icon(icons::TRASH)
                    .color(theme.danger),
                context,
                move |this, _, _| commit(this, &[(&on, 0.)]),
            )
        } else {
            menu
        };
        Some(menu.into_element())
    }

    /// The bands to pick from, the picked one ticked.
    pub fn eq_band_menu(&self, plugin: PluginId, context: &mut Context<Self>) -> Option<gpui::Div> {
        let editor = self.bottom.rack.slots.get(&plugin)?.editor.clone();
        let selected = match &*editor.borrow() {
            Editor::Eq(eq) => eq.selected,
            _ => return None,
        };
        let menu = (0..BANDS).fold(Menu::new(&self.theme), |menu, band| {
            let editor = editor.clone();
            menu.item(
                MenuItem::new(format!("Band {}", band + 1)).checked(band == selected),
                context,
                move |_, _, _| {
                    if let Editor::Eq(eq) = &mut *editor.borrow_mut() {
                        eq.selected = band;
                    }
                },
            )
        });
        Some(menu.into_element())
    }
}
