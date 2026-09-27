use crate::{
    core::state::ToniqueProjectState,
    ui::{
        font::PHOSPHOR_REGULAR,
        theme::{COLORS, Theme, ThemeExt, ThemeLibrary, format_color},
        widget::{section::SectionHeader, select::Select, square_button::SquareButton},
    },
};
use egui::{
    Color32, FontFamily, FontId, Grid, RichText, ScrollArea, TextEdit, Ui,
    color_picker::{Alpha, color_edit_button_srgba},
    vec2,
};
use egui_phosphor::regular::{ARROW_COUNTER_CLOCKWISE, ARROWS_CLOCKWISE, COPY, PLUS, WARNING};

const LABEL_WIDTH: f32 = 110.;
const CONTROL_WIDTH: f32 = 240.;
const BUTTON_HEIGHT: f32 = 22.;
const EDITOR_HEIGHT: f32 = 340.;

/// The theme shown in the editor.
struct Draft {
    id: String,
    theme: Theme,
    /// What it builds on: resetting a colour goes back to this.
    base: Theme,
    /// Presets are shown read-only; duplicate one to edit it.
    editable: bool,
    /// Edited since last saved.
    dirty: bool,
}

/// Settings tab to choose a theme and edit custom ones. Edits show in the
/// whole app as they're made and are saved to the theme's file.
pub struct UIAppearanceTab {
    themes: ThemeLibrary,
    /// What went wrong loading the current theme.
    warnings: Vec<String>,
    draft: Option<Draft>,
    /// Last failed save or duplicate.
    error: Option<String>,
}

impl UIAppearanceTab {
    pub fn new(themes: ThemeLibrary, warnings: Vec<String>) -> Self {
        Self {
            themes,
            warnings,
            draft: None,
            error: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let current = state.settings().theme.clone();
        if self.draft.as_ref().is_none_or(|d| d.id != current) {
            self.draft = Some(self.load_draft(&current));
        }

        ui.add(SectionHeader::new("Theme"));
        self.picker(ui, state, &current);
        let folder = self
            .themes
            .dir()
            .map_or("the themes folder".into(), |d| d.display().to_string());
        let theme = ui.app_theme();
        let note = if self.draft.as_ref().is_some_and(|d| d.editable) {
            format!("Saved in {folder}. Changes apply as you make them.")
        } else {
            "Built-in themes can't be edited: duplicate one to make your own.".into()
        };
        ui.label(RichText::new(note).small().color(theme.text_muted));
        for problem in self.error.iter().chain(self.warnings.iter().take(5)) {
            ui.label(RichText::new(problem).small().color(theme.warning));
        }
        ui.add_space(8.);

        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let changed = ScrollArea::vertical()
            .max_height(EDITOR_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add_enabled_ui(draft.editable, |ui| editor(ui, draft))
                    .inner
            })
            .inner;
        if changed {
            draft.dirty = true;
            draft.theme.clone().install(ui.ctx());
        }
        // Save once the pointer is released, not on every step of a drag.
        if draft.dirty && !ui.input(|i| i.pointer.any_down()) {
            draft.dirty = false;
            self.error = self.themes.save(&draft.id, &draft.theme).err();
        }
    }

    fn picker(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState, current: &str) {
        let mut picked = current.to_string();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.;
            let themes = self.themes.list();
            let missing = (!themes.iter().any(|t| t.id == current))
                .then(|| (current.to_string(), format!("{current} (not found)")));
            ui.add(
                Select::new("settings-theme", &mut picked)
                    .width(CONTROL_WIDTH)
                    .options(themes.into_iter().map(|t| {
                        let label = if t.preset {
                            t.name
                        } else {
                            format!("{} (custom)", t.name)
                        };
                        (t.id, label)
                    }))
                    .options(missing),
            );
            if ui
                .add(icon_button(COPY).tooltip("Duplicate, to edit a copy"))
                .clicked()
            {
                match self.themes.duplicate(current) {
                    Ok(id) => picked = id,
                    Err(error) => self.error = Some(error),
                }
            }
            if ui
                .add(icon_button(ARROWS_CLOCKWISE).tooltip("Reload themes from disk"))
                .clicked()
            {
                self.themes = ThemeLibrary::load();
                // Pick up edits made to the current theme's file.
                self.select(ui, state, current);
            }
        });
        if picked != current {
            self.select(ui, state, &picked);
        }
    }

    /// Install theme `id` and save it as the chosen one.
    pub fn select(&mut self, ui: &Ui, state: &mut ToniqueProjectState, id: &str) {
        let (theme, warnings) = self.themes.resolve(id);
        theme.install(ui.ctx());
        self.warnings = warnings;
        self.error = None;
        self.draft = Some(self.load_draft(id));
        let mut settings = state.settings().clone();
        settings.theme = id.to_string();
        state.apply_settings(settings);
    }

    fn load_draft(&self, id: &str) -> Draft {
        Draft {
            id: id.to_string(),
            theme: self.themes.resolve(id).0,
            base: self.themes.resolve(&self.themes.base_of(id)).0,
            editable: !self.themes.is_preset(id),
            dirty: false,
        }
    }
}

/// Name, every colour by group, and the palette. Returns whether anything
/// changed.
fn editor(ui: &mut Ui, draft: &mut Draft) -> bool {
    let theme = ui.app_theme();
    let issues = draft.theme.contrast_issues();
    let mut changed = false;

    Grid::new("theme-name")
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([12., 6.])
        .show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme.text));
            changed |= ui
                .add(TextEdit::singleline(&mut draft.theme.name).desired_width(CONTROL_WIDTH))
                .changed();
            ui.end_row();
        });
    ui.add_space(6.);

    let mut groups: Vec<&str> = COLORS.iter().map(|role| role.group).collect();
    groups.dedup();
    for group in groups {
        ui.add(SectionHeader::new(group));
        Grid::new(("theme-colors", group))
            .num_columns(2)
            .min_col_width(LABEL_WIDTH)
            .spacing([12., 4.])
            .show(ui, |ui| {
                for role in COLORS.iter().filter(|role| role.group == group) {
                    ui.label(RichText::new(role.label).color(theme.text))
                        .on_hover_text(role.description);
                    let base = draft.base.color(role.key).unwrap_or_default();
                    let issue = issues.iter().find(|(key, _)| *key == role.key);
                    if let Some(color) = draft.theme.color_mut(role.key) {
                        changed |= color_row(ui, color, base, issue.map(|(_, text)| text));
                    }
                    ui.end_row();
                }
            });
        ui.add_space(6.);
    }

    ui.add(SectionHeader::new("Palette"));
    ui.label(
        RichText::new("Colours for new tracks and graph nodes. Right-click one to remove it.")
            .small()
            .color(theme.text_muted),
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4., 4.);
        let mut remove = None;
        for (i, color) in draft.theme.palette.iter_mut().enumerate() {
            let response = color_edit_button_srgba(ui, color, Alpha::Opaque);
            changed |= response.changed();
            if response.secondary_clicked() {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            draft.theme.palette.remove(i);
            changed = true;
        }
        if ui.add(icon_button(PLUS).tooltip("Add a colour")).clicked() {
            draft.theme.palette.push(draft.theme.accent);
            changed = true;
        }
        if draft.theme.palette != draft.base.palette
            && ui
                .add(icon_button(ARROW_COUNTER_CLOCKWISE).tooltip("Restore the palette"))
                .clicked()
        {
            draft.theme.palette.clone_from(&draft.base.palette);
            changed = true;
        }
    });
    changed
}

/// Picker, hex value, contrast warning and reset for one colour.
fn color_row(ui: &mut Ui, color: &mut Color32, base: Color32, issue: Option<&String>) -> bool {
    let theme = ui.app_theme();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        changed |= color_edit_button_srgba(ui, color, Alpha::OnlyBlend).changed();
        ui.label(
            RichText::new(format_color(*color))
                .monospace()
                .small()
                .color(theme.text_muted),
        );
        if let Some(issue) = issue {
            ui.label(
                RichText::new(WARNING)
                    .family(FontFamily::Name(PHOSPHOR_REGULAR.into()))
                    .color(theme.warning),
            )
            .on_hover_text(format!("Hard to read: {issue}"));
        }
        if *color != base
            && ui
                .add(icon_button(ARROW_COUNTER_CLOCKWISE).tooltip(format!(
                    "Restore {} from the base theme",
                    format_color(base)
                )))
                .clicked()
        {
            *color = base;
            changed = true;
        }
    });
    changed
}

fn icon_button(icon: &str) -> SquareButton {
    SquareButton::new(icon)
        .square(BUTTON_HEIGHT - 2.)
        .font(FontId::new(12., FontFamily::Name(PHOSPHOR_REGULAR.into())))
        .border_radius(2.)
}
