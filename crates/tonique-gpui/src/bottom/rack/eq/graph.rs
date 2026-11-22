//! The EQ's graph: its curve, spectrum and points, and what pressing it does.

use super::*;

/// Switches on the first band that's off at `hz`, `db` up: a cut at the
/// graph's ends, a bell between. The band, if one was free.
pub(super) fn add_point(
    app: &mut Tonique,
    params: &[BandParams; BANDS],
    now: &[BandValues; BANDS],
    hz: f32,
    db: f32,
) -> Option<usize> {
    let band = (0..BANDS).find(|band| !now[*band].on)?;
    let mode = if hz < LOW_CUT_BELOW {
        FilterMode::HighPass
    } else if hz > HIGH_CUT_ABOVE {
        FilterMode::LowPass
    } else {
        FilterMode::Peak
    };
    let [freq, gain, q, shape, on] = &params[band];
    commit(
        app,
        &[
            (freq, hz),
            (shape, eq_mode_value(mode)),
            (q, q.default),
            (gain, if mode.has_gain() { db } else { 0. }),
            (on, 1.),
        ],
    );
    Some(band)
}

/// How a point is drawn: on, picked, under the pointer.
#[derive(Clone, Copy)]
struct PointLook {
    on: bool,
    picked: bool,
    hot: bool,
}

/// A band's point: filled with its colour when on, an outline when off;
/// larger and haloed under the pointer, ringed when picked.
fn paint_point(
    window: &mut gpui::Window,
    context: &mut gpui::App,
    theme: &Theme,
    at: (f32, f32),
    band: usize,
    look: PointLook,
) {
    let color = if look.on {
        band_color(theme, band, true)
    } else {
        theme.text_disabled
    };
    let radius = if look.hot || look.picked {
        POINT_RADIUS + 1.5
    } else {
        POINT_RADIUS
    };
    if look.hot {
        circle(window, at, radius + 5., faded(color, 0.2), None);
    }
    if look.on {
        circle(window, at, radius, color, None);
    } else {
        circle(window, at, radius, theme.bg_deep, Some((1.5, color)));
    }
    if look.picked {
        circle(
            window,
            at,
            radius + 2.,
            TRANSPARENT,
            Some((1.5, theme.text)),
        );
    }
    let font = gpui::Font {
        weight: gpui::FontWeight::SEMIBOLD,
        ..gpui::font(FONT_UI)
    };
    text(
        window,
        context,
        (band + 1).to_string(),
        at,
        Align::CENTER,
        size::SMALL,
        if look.on { theme.bg_deep } else { color },
        Some(font),
    );
}

/// `points` dashed: on 4 px, off 3, along the line.
fn dashed(window: &mut gpui::Window, points: &[(f32, f32)], width: f32, color: Color) {
    let (mut along, mut dash) = (0., Vec::new());
    for pair in points.windows(2) {
        if along % 7. < 4. {
            if dash.is_empty() {
                dash.push(pair[0]);
            }
            dash.push(pair[1]);
        } else if dash.len() > 1 {
            polyline(window, &std::mem::take(&mut dash), width, color);
        } else {
            dash.clear();
        }
        along += (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
    }
    if dash.len() > 1 {
        polyline(window, &dash, width, color);
    }
}

impl Tonique {
    /// The graph: drag, add, remove and pick points, and draw it all.
    pub(super) fn eq_graph(
        &self,
        input: &EditorInput,
        params: &[BandParams; BANDS],
        values: [BandValues; BANDS],
        selected: usize,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let enabled = input.enabled();
        let sample_rate = input.sample_rate;
        let graph_bounds = match &*input.editor.borrow() {
            Editor::Eq(eq) => eq.graph.clone(),
            _ => Rc::default(),
        };
        let dragged = match &self.bottom.rack.drag {
            Some(RackDrag::Graph(drag)) => {
                (0..BANDS).find(|band| params[*band][0].id == drag.cutoff.id)
            }
            _ => None,
        };
        let editor = input.editor.clone();
        let paint_theme = theme.clone();
        let slot = graph_bounds.clone();
        let painting = canvas(
            move |bounds, _, _| slot.set(bounds),
            move |bounds, _, window, context| {
                let theme = &paint_theme;
                let rect = Rect::of(bounds);
                let scale = graph_scale(rect);
                fill_rounded(window, rect, 3., theme.bg_deep);
                paint_freq_grid(window, context, rect, theme);
                let mut db = -(RANGE_DB / 6.).floor() * 6.;
                while db < RANGE_DB {
                    let color = if db == 0. {
                        theme.grid_bar
                    } else {
                        theme.grid_beat
                    };
                    hline(window, rect, scale.y(db), color);
                    db += 6.;
                }
                // Labelled when there's room: small, they'd sit under the
                // points.
                if rect.height > LABELLED_HEIGHT {
                    for db in [-12., -6., 6., 12.] {
                        text(
                            window,
                            context,
                            format!("{db:+.0}"),
                            (rect.left + 4., scale.y(db) - 1.),
                            Align::LEFT_BOTTOM,
                            size::SMALL,
                            theme.text_muted,
                            None,
                        );
                    }
                }
                let mut editor = editor.borrow_mut();
                let Editor::Eq(eq) = &mut *editor else {
                    return;
                };
                if enabled {
                    eq.spectrum.paint(window, rect, theme);
                }
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                    // The band picked on its own, dashed, in its colour,
                    // where it changes anything.
                    let picked = values[selected];
                    if picked.on {
                        let filter = picked.biquad(sample_rate);
                        let tint = faded(band_color(theme, selected, enabled), 0.7);
                        let mut run: Vec<(f32, f32)> = Vec::new();
                        for column in 0..=rect.width.ceil() as usize + 1 {
                            let x = rect.left + column as f32;
                            let db = filter.response_db(scale.hz(x), sample_rate);
                            if db.abs() > 0.1 && x <= rect.right() + 1. {
                                run.push((x, scale.y(db)));
                            } else {
                                dashed(window, &std::mem::take(&mut run), 1., tint);
                            }
                        }
                        dashed(window, &run, 1., tint);
                    }
                    let zero = scale.y(0.);
                    let curve = eq.curve(values, sample_rate, &scale, theme, enabled);
                    // Between the curve and 0 dB, in its colours.
                    for (column, (db, color)) in curve.db.iter().zip(&curve.colors).enumerate() {
                        let (x, y) = (rect.left + column as f32, scale.y(*db));
                        let (top, bottom) = (y.min(zero), y.max(zero));
                        fill(
                            window,
                            Rect::new(x, top, 1., bottom - top),
                            faded(*color, 0.15),
                        );
                    }
                    let points: Vec<(f32, f32)> = curve
                        .db
                        .iter()
                        .enumerate()
                        .map(|(column, db)| (rect.left + column as f32, scale.y(*db)))
                        .collect();
                    let mut start = 0;
                    while start + 1 < points.len() {
                        let end = (start + COLOR_RUN).min(points.len() - 1);
                        let color = curve.colors[(start + end) / 2];
                        polyline(window, &points[start..=end], 2., color);
                        start = end;
                    }
                });

                let pointer = window.mouse_position();
                let hovered = if bounds.contains(&pointer) {
                    let at = (f32::from(pointer.x), f32::from(pointer.y));
                    nearest(&scale, &values, selected, at)
                } else {
                    None
                };
                for (band, value) in values.iter().enumerate() {
                    if !(value.on || band == selected) {
                        continue;
                    }
                    let look = PointLook {
                        on: value.on && enabled,
                        picked: band == selected,
                        hot: hovered == Some(band) || dragged == Some(band),
                    };
                    let at = (scale.x(value.hz), scale.y(value.height()));
                    paint_point(window, context, theme, at, band, look);
                }
                if let Some(band) = hovered.or(dragged) {
                    let value = values[band];
                    let detail = if value.mode.has_gain() {
                        format_db(value.gain)
                    } else {
                        format!("Q {:.2}", value.q)
                    };
                    text(
                        window,
                        context,
                        format!(
                            "{} · {} · {detail}",
                            shape_name(value.mode),
                            format_hz(value.hz)
                        ),
                        (rect.right() - 6., rect.top + 4.),
                        Align::RIGHT_TOP,
                        size::LABEL,
                        band_color(theme, band, enabled),
                        None,
                    );
                } else if !values.iter().any(|value| value.on) {
                    text(
                        window,
                        context,
                        "Double-click to add a point",
                        (rect.left + rect.width / 2., rect.top + rect.height / 2.),
                        Align::CENTER,
                        size::LABEL,
                        theme.text_muted,
                        None,
                    );
                }
            },
        )
        .absolute()
        .size_full();

        let (track, plugin) = (input.track, input.effect.plugin.id);
        let editor = input.editor.clone();
        let press_params = params.clone();
        let menu_params = params.clone();
        let menu_editor = input.editor.clone();
        let menu_bounds = graph_bounds.clone();
        div()
            .id("eq-graph")
            .absolute()
            .size_full()
            .cursor(CursorStyle::Crosshair)
            .child(painting)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let graph = graph_bounds.get();
                    let scale = graph_scale(Rect::of(graph));
                    if scale.rect.is_empty() {
                        return;
                    }
                    let (x, y) = (f32::from(event.position.x), f32::from(event.position.y));
                    let now = press_params.each_ref().map(BandValues::of);
                    let Editor::Eq(eq) = &mut *editor.borrow_mut() else {
                        return;
                    };
                    let pressed = nearest(&scale, &now, eq.selected, (x, y));
                    this.bottom.rack.drag = None;
                    if event.click_count >= 2 {
                        match pressed {
                            // Removed: switched off.
                            Some(band) => commit(this, &[(&press_params[band][4], 0.)]),
                            None => {
                                if let Some(band) =
                                    add_point(this, &press_params, &now, scale.hz(x), scale.db(y))
                                {
                                    eq.selected = band;
                                }
                            }
                        }
                    } else if let Some(band) = pressed {
                        // Dragged across for its frequency, up for its
                        // height; a band that's off switches on once moved.
                        eq.selected = band;
                        let band_params = &press_params[band];
                        let height = height_param(band_params).clone();
                        let on = band_params[4].clone();
                        let before = on.get();
                        this.bottom.rack.drag = Some(RackDrag::Graph(GraphDrag {
                            cutoff: band_params[0].clone(),
                            before: (band_params[0].get(), height.get()),
                            height,
                            mode: now[band].mode,
                            graph: graph_bounds.clone(),
                            range: (RANGE_DB, -RANGE_DB),
                            switched: Some((on, before)),
                        }));
                    }
                    context.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                context.listener(move |this, event: &MouseDownEvent, _, context| {
                    context.stop_propagation();
                    let scale = graph_scale(Rect::of(menu_bounds.get()));
                    let at = (f32::from(event.position.x), f32::from(event.position.y));
                    let now = menu_params.each_ref().map(BandValues::of);
                    let Editor::Eq(eq) = &mut *menu_editor.borrow_mut() else {
                        return;
                    };
                    let Some(band) = nearest(&scale, &now, eq.selected, at) else {
                        return;
                    };
                    eq.selected = band;
                    this.toggle_popup(
                        PopupKind::EqPoint(track, plugin, band),
                        event.position,
                        Anchor::TopLeft,
                    );
                    context.notify();
                }),
            )
    }
}
