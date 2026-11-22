//! The Appearance tab, as the egui app's: the theme to use, picked from the
//! presets and the user's theme files, and an editor for the user's own.
//! Edits show in the whole app as they're made and are saved to the
//! theme's file. Presets are shown read-only: duplicate one to edit it.
//!
//! Unlike egui, gpui has no colour picker: a colour is edited by typing
//! its hex value.

use super::SettingsDialog;
use crate::model::HoldsProject;
use gpui::{App, Context, MouseButton, Window, div, prelude::*, px};

use crate::{
    app::{
        PopupKind, focus_main,
        settings::{CONTROL_HEIGHT, CONTROL_WIDTH, ROW_GAP, section_header, setting_row, status},
    },
    icons,
    theme::{COLORS, Color, FONT_MONO, Theme, ThemeLibrary, hsla, size},
    utils::color::{format_color, parse_color},
    widgets::{Flat, Menu, MenuItem, TextField, glyph, select, typing_view},
};

const ICON_BUTTON: f32 = CONTROL_HEIGHT;
const ICON_SIZE: f32 = 12.;
const SWATCH_SIZE: f32 = 18.;
/// A hex value, `#rrggbbaa` at most.
const HEX_WIDTH: f32 = 72.;
/// Warnings shown at most, as the egui app.
const MAX_WARNINGS: usize = 5;

/// The theme shown in the editor.
struct Draft {
    id: String,
    theme: Theme,
    /// What it builds on: resetting a colour goes back to this.
    base: Theme,
    /// Presets are shown read-only; duplicate one to edit it.
    editable: bool,
}

/// What of the theme is being typed.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Typed {
    Name,
    /// The colour of this key of theme files.
    Color(&'static str),
    /// This colour of the palette.
    Palette(usize),
}

pub struct AppearanceForm {
    themes: ThemeLibrary,
    /// What went wrong loading the current theme.
    warnings: Vec<String>,
    draft: Option<Draft>,
    /// The last failed save, duplicate or typed colour.
    error: Option<String>,
    typing: Option<(Typed, TextField)>,
}

impl AppearanceForm {
    pub fn new(themes: ThemeLibrary, warnings: Vec<String>) -> Self {
        Self {
            themes,
            warnings,
            draft: None,
            error: None,
            typing: None,
        }
    }

    /// The theme `id`, as edited.
    fn load_draft(&self, id: &str) -> Draft {
        Draft {
            id: id.to_string(),
            theme: self.themes.resolve(id).0,
            base: self.themes.resolve(&self.themes.base_of(id)).0,
            editable: !self.themes.is_preset(id),
        }
    }

    /// Shows the theme `id` in the editor, nothing being typed.
    pub fn open(&mut self, id: &str) {
        self.draft = Some(self.load_draft(id));
        self.typing = None;
        self.error = None;
    }
}

impl SettingsDialog {
    /// Uses `theme` everywhere, new tracks taking their colours from its
    /// palette.
    pub fn install_theme(&mut self, theme: Theme, context: &mut App) {
        self.state_mut(context).set_track_palette(&theme.palette);
        crate::theme::install(theme, context);
    }

    /// Uses theme `id` and saves it as the chosen one.
    fn select_theme(&mut self, id: &str, context: &mut App) {
        let form = &mut self.appearance;
        let (theme, warnings) = form.themes.resolve(id);
        form.warnings = warnings;
        form.open(id);
        self.install_theme(theme, context);
        let mut settings = self.state(context).settings().clone();
        settings.theme = id.to_string();
        self.draft.theme = settings.theme.clone();
        self.state_mut(context).apply_settings(settings);
    }

    fn duplicate_theme(&mut self, context: &mut App) {
        let current = self.state(context).settings().theme.clone();
        match self.appearance.themes.duplicate(&current) {
            Ok(id) => self.select_theme(&id, context),
            Err(error) => self.appearance.error = Some(error),
        }
    }

    /// Reads the theme files again, picking up edits made to the current
    /// one's.
    fn reload_themes(&mut self, context: &mut App) {
        self.appearance.themes = ThemeLibrary::load();
        let current = self.state(context).settings().theme.clone();
        self.select_theme(&current, context);
    }

    /// Changes the theme being edited, shows it and saves it.
    fn edit_theme(&mut self, change: impl FnOnce(&mut Draft), context: &mut App) {
        let form = &mut self.appearance;
        let Some(draft) = form.draft.as_mut().filter(|draft| draft.editable) else {
            return;
        };
        change(draft);
        let theme = draft.theme.clone();
        form.error = form.themes.save(&draft.id, &theme).err();
        self.install_theme(theme, context);
    }

    /// Starts typing `typed`, from `text`.
    fn type_theme(
        &mut self,
        typed: Typed,
        text: String,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let mut field = TextField::new(context);
        field.start(text, window, context);
        self.appearance.typing = Some((typed, field));
    }

    fn finish_theme_typing(
        &mut self,
        keep: bool,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let Some((typed, field)) = self.appearance.typing.take() else {
            return;
        };
        focus_main(window, context);
        if !keep {
            return;
        }
        let text = field.text.trim();
        if typed == Typed::Name {
            if !text.is_empty() {
                let name = text.to_string();
                self.edit_theme(|draft| draft.theme.name = name, context);
            }
            return;
        }
        let hex = if text.starts_with('#') {
            text.to_string()
        } else {
            format!("#{text}")
        };
        let Some(color) = parse_color(&hex) else {
            self.appearance.error = Some(format!("“{text}” isn't a #rrggbb colour"));
            return;
        };
        self.edit_theme(
            |draft| match typed {
                Typed::Color(key) => {
                    if let Some(slot) = draft.theme.color_mut(key) {
                        *slot = color;
                    }
                }
                Typed::Palette(index) => {
                    // The palette has no transparent colours.
                    let opaque = Color { a: 255, ..color };
                    if let Some(slot) = draft.theme.palette.get_mut(index) {
                        *slot = opaque;
                    }
                }
                Typed::Name => {}
            },
            context,
        );
    }

    /// The themes to choose from, the one in use ticked.
    pub fn theme_menu(&self, context: &mut Context<Self>) -> gpui::Div {
        let current = &self.state(context).settings().theme.clone();
        let themes = self.appearance.themes.list();
        let missing = (!themes.iter().any(|theme| theme.id == *current))
            .then(|| (current.clone(), format!("{current} (not found)")));
        let options = themes
            .into_iter()
            .map(|theme| {
                let label = if theme.preset {
                    theme.name
                } else {
                    format!("{} (custom)", theme.name)
                };
                (theme.id, label)
            })
            .chain(missing);
        options
            .fold(
                Menu::new(&crate::theme::current(context)),
                |menu, (id, label)| {
                    let checked = id == *current;
                    menu.item(
                        MenuItem::new(label).checked(checked),
                        context,
                        move |this, _, context| this.select_theme(&id, context),
                    )
                },
            )
            .into_element()
    }

    pub(super) fn appearance_tab(
        &mut self,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let current = self.state(context).settings().theme.clone();
        let form = &mut self.appearance;
        if form.draft.as_ref().is_none_or(|draft| draft.id != current) {
            form.open(&current);
        }
        let form = &self.appearance;
        let Some(draft) = form.draft.as_ref() else {
            return div();
        };

        let open = crate::popup::shown(context) == Some(PopupKind::Theme);
        let label = if form.themes.is_preset(&draft.id) {
            draft.theme.name.clone()
        } else {
            format!("{} (custom)", draft.theme.name)
        };
        let picker = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(select(
                "theme-select",
                label,
                CONTROL_WIDTH,
                PopupKind::Theme,
                self.menu_builder(PopupKind::Theme, context),
                open,
                theme,
                context,
            ))
            .child(
                Flat::new("duplicate-theme", ICON_BUTTON)
                    .width(ICON_BUTTON)
                    .show(
                        glyph(icons::COPY, ICON_SIZE, false),
                        theme,
                        context,
                        |this, context| this.duplicate_theme(context),
                    ),
            )
            .child(
                Flat::new("reload-themes", ICON_BUTTON)
                    .width(ICON_BUTTON)
                    .show(
                        glyph(icons::ARROWS_CLOCKWISE, ICON_SIZE, false),
                        theme,
                        context,
                        |this, context| this.reload_themes(context),
                    ),
            );

        let folder = form
            .themes
            .dir()
            .map_or("the themes folder".into(), |folder| {
                folder.display().to_string()
            });
        let note = if draft.editable {
            format!("Saved in {folder}. Changes apply as you make them.")
        } else {
            "Built-in themes can't be edited: duplicate one to make your own.".into()
        };
        let problems = form
            .error
            .iter()
            .chain(form.warnings.iter().take(MAX_WARNINGS))
            .map(|problem| status(problem.clone(), theme).text_color(hsla(theme.warning)));

        div()
            .child(section_header("Theme", theme))
            .child(
                div()
                    .mt(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(ROW_GAP))
                    .child(picker)
                    .child(status(note, theme))
                    .children(problems),
            )
            .child(self.theme_editor(draft, theme, context))
    }

    /// The name, every colour by group, and the palette.
    fn theme_editor(&self, draft: &Draft, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let issues = draft.theme.contrast_issues();
        let typing = self.appearance.typing.as_ref();
        let name = match typing {
            Some((Typed::Name, field)) => {
                self.theme_typing(field, theme, context).into_any_element()
            }
            _ => {
                let text = draft.theme.name.clone();
                self.theme_value(
                    "theme-name",
                    draft.editable,
                    theme,
                    context,
                    move |this, window, context| {
                        this.type_theme(Typed::Name, text.clone(), window, context)
                    },
                )
                .w(px(CONTROL_WIDTH))
                .child(draft.theme.name.clone())
                .into_any_element()
            }
        };
        let mut editor = div()
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .child(setting_row("Name", name, theme));

        let mut groups: Vec<&str> = COLORS.iter().map(|role| role.group).collect();
        groups.dedup();
        for group in groups {
            editor = editor.child(section_header(group, theme).mt(px(4.)));
            for role in COLORS.iter().filter(|role| role.group == group) {
                let color = draft.theme.color(role.key).unwrap_or_default();
                let base = draft.base.color(role.key).unwrap_or_default();
                let issue = issues
                    .iter()
                    .find(|(key, _)| *key == role.key)
                    .map(|(_, text)| text.clone());
                let row =
                    self.color_row(role.key, color, base, issue, draft.editable, theme, context);
                editor = editor.child(setting_row(role.label, row, theme));
            }
        }

        editor
            .child(section_header("Palette", theme).mt(px(4.)))
            .child(status(
                if draft.editable {
                    "Colours for new tracks. Right-click one to remove it."
                } else {
                    "Colours for new tracks."
                },
                theme,
            ))
            .child(self.palette_row(draft, theme, context))
    }

    /// A colour's swatch, hex value, contrast warning and reset.
    #[allow(clippy::too_many_arguments)]
    fn color_row(
        &self,
        key: &'static str,
        color: Color,
        base: Color,
        issue: Option<String>,
        editable: bool,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let typing = self
            .appearance
            .typing
            .as_ref()
            .filter(|(typed, _)| *typed == Typed::Color(key));
        let hex = format_color(color);
        let value = match typing {
            Some((_, field)) => self
                .theme_typing(field, theme, context)
                .w(px(HEX_WIDTH))
                .into_any_element(),
            None => {
                let text = hex.clone();
                self.theme_value(
                    key,
                    editable,
                    theme,
                    context,
                    move |this, window, context| {
                        this.type_theme(Typed::Color(key), text.clone(), window, context)
                    },
                )
                .w(px(HEX_WIDTH))
                .font_family(FONT_MONO)
                .text_size(px(size::LABEL))
                .child(hex)
                .into_any_element()
            }
        };
        let mut row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .child(swatch(color, theme))
            .child(value);
        if editable && color != base {
            row = row.child(
                Flat::ghost(("restore-color", key_index(key)), ICON_BUTTON)
                    .width(ICON_BUTTON)
                    .show(
                        glyph(icons::ARROW_COUNTER_CLOCKWISE, ICON_SIZE, false),
                        theme,
                        context,
                        move |this, context| {
                            this.edit_theme(
                                |draft| {
                                    if let Some(slot) = draft.theme.color_mut(key) {
                                        *slot = base;
                                    }
                                },
                                context,
                            )
                        },
                    ),
            );
        }
        if let Some(issue) = issue {
            row = row.child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap(px(3.))
                    .text_size(px(size::SMALL))
                    .text_color(hsla(theme.warning))
                    .child(glyph(icons::WARNING, ICON_SIZE, false))
                    .child(div().min_w_0().child(issue)),
            );
        }
        row
    }

    /// The palette's colours, each typed when clicked and removed when
    /// right-clicked; then adding one and restoring them.
    fn palette_row(&self, draft: &Draft, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let typing = self.appearance.typing.as_ref();
        let mut row = div().flex().flex_wrap().items_center().gap(px(4.));
        for (index, &color) in draft.theme.palette.iter().enumerate() {
            if let Some((Typed::Palette(typed), field)) = typing
                && *typed == index
            {
                row = row.child(self.theme_typing(field, theme, context).w(px(HEX_WIDTH)));
                continue;
            }
            let mut chip = div().id(("palette", index)).child(swatch(color, theme));
            if draft.editable {
                chip = chip
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        context.listener(move |this, _, window, context| {
                            context.stop_propagation();
                            let text = format_color(color);
                            this.type_theme(Typed::Palette(index), text, window, context);
                            context.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        context.listener(move |this, _, _, context| {
                            context.stop_propagation();
                            this.edit_theme(
                                |draft| {
                                    if index < draft.theme.palette.len() {
                                        draft.theme.palette.remove(index);
                                    }
                                },
                                context,
                            );
                            context.notify();
                        }),
                    );
            }
            row = row.child(chip);
        }
        if !draft.editable {
            return row;
        }
        row = row.child(
            Flat::ghost("add-palette-color", ICON_BUTTON)
                .width(ICON_BUTTON)
                .show(
                    glyph(icons::PLUS, ICON_SIZE, false),
                    theme,
                    context,
                    |this, context| {
                        this.edit_theme(
                            |draft| {
                                let accent = draft.theme.accent;
                                draft.theme.palette.push(accent)
                            },
                            context,
                        )
                    },
                ),
        );
        if draft.theme.palette != draft.base.palette {
            row = row.child(
                Flat::ghost("restore-palette", ICON_BUTTON)
                    .width(ICON_BUTTON)
                    .show(
                        glyph(icons::ARROW_COUNTER_CLOCKWISE, ICON_SIZE, false),
                        theme,
                        context,
                        |this, context| {
                            this.edit_theme(
                                |draft| draft.theme.palette.clone_from(&draft.base.palette),
                                context,
                            )
                        },
                    ),
            );
        }
        row
    }

    /// A value of the theme, as an input box: pressed, it's typed in, when
    /// the theme can be edited.
    fn theme_value(
        &self,
        id: &'static str,
        editable: bool,
        theme: &Theme,
        context: &mut Context<Self>,
        start: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::Stateful<gpui::Div> {
        let element = div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .h(px(CONTROL_HEIGHT))
            .px(px(4.))
            .rounded(px(2.))
            .overflow_hidden()
            .whitespace_nowrap()
            .bg(hsla(theme.bg_deep))
            .text_color(hsla(if editable {
                theme.text
            } else {
                theme.text_muted
            }));
        if !editable {
            return element;
        }
        element.cursor_text().on_mouse_down(
            MouseButton::Left,
            context.listener(move |this, _, window, context| {
                context.stop_propagation();
                start(this, window, context);
                context.notify();
            }),
        )
    }

    /// The value being typed.
    fn theme_typing(
        &self,
        field: &TextField,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        typing_view(
            "theme-typing",
            field,
            theme,
            context,
            |this| this.appearance.typing.as_mut().map(|(_, field)| field),
            Self::finish_theme_typing,
        )
        .flex_none()
        .w(px(CONTROL_WIDTH))
        .h(px(CONTROL_HEIGHT))
    }
}

/// Where `key` is among the theme's colours, to tell their buttons apart.
fn key_index(key: &str) -> usize {
    COLORS
        .iter()
        .position(|role| role.key == key)
        .unwrap_or_default()
}

/// A colour, in a box outlined so it shows on any background.
fn swatch(color: Color, theme: &Theme) -> gpui::Div {
    div()
        .flex_none()
        .size(px(SWATCH_SIZE))
        .rounded(px(2.))
        .border_1()
        .border_color(hsla(theme.border))
        .bg(hsla(color))
}
