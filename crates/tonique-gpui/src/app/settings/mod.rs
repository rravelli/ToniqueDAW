//! The settings dialog, as the egui app's settings window: the General tab
//! (the metronome's level) and the Appearance tab (the theme) apply at
//! once; the Audio tab's device and engine settings are gathered in a
//! draft and applied together, since they restart the audio. Settings are
//! Tonique's own, saved where the egui app reads them.
//!
//! Left out, as this app hasn't them: the interface's scale and editing
//! shortcuts.

mod appearance;
mod audio;
mod general;

use gpui::{Context, MouseButton, MouseMoveEvent, SharedString, Window, div, prelude::*, px};

use crate::{
    app::{Tonique, dialog::Dialog},
    config::settings::Settings,
    icons,
    theme::{Theme, ThemeLibrary, hsla, size},
    widgets::{Flat, dialog_frame, dialog_title, glyph, tab_bar},
};
use appearance::AppearanceForm;
pub use audio::AudioChoice;
use audio::AudioForm;
use general::GeneralForm;

const WIDTH: f32 = 440.;
/// Fits the tallest tab, so switching tabs doesn't resize the dialog.
const HEIGHT: f32 = 440.;
const LABEL_WIDTH: f32 = 120.;
const CONTROL_WIDTH: f32 = 240.;
const CONTROL_HEIGHT: f32 = 22.;
const ROW_GAP: f32 = 6.;
const SECTION_GAP: f32 = 10.;
const CLOSE_SIZE: f32 = 20.;
/// Kept free around the dialog in a window too short for it.
const WINDOW_MARGIN: f32 = 12.;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SettingsTab {
    General,
    Appearance,
    Audio,
}

pub struct SettingsUi {
    tab: SettingsTab,
    /// The settings being edited: the audio ones apply on demand.
    draft: Settings,
    general: GeneralForm,
    appearance: AppearanceForm,
    audio: AudioForm,
}

impl SettingsUi {
    /// The settings, choosing among `themes`; `theme_warnings` tell what
    /// went wrong loading the theme in use.
    pub fn new(themes: ThemeLibrary, theme_warnings: Vec<String>) -> Self {
        Self {
            tab: SettingsTab::General,
            draft: Settings::default(),
            general: GeneralForm::default(),
            appearance: AppearanceForm::new(themes, theme_warnings),
            audio: AudioForm::new(),
        }
    }

    pub fn is_dragging(&self) -> bool {
        self.general.is_dragging() || self.audio.is_dragging()
    }

    pub fn end_drags(&mut self) {
        self.audio.end_drags();
    }
}

impl Tonique {
    /// Opens the settings on `tab`, from the settings in use.
    pub fn open_settings(&mut self, tab: SettingsTab) {
        let settings = &mut self.settings;
        settings.tab = tab;
        settings.draft = self.state.settings().clone();
        settings.appearance.open(&settings.draft.theme);
        // Probing the hardware is slow: once, on opening.
        settings.audio.refresh_devices();
        self.dialog = Some(Dialog::Settings);
    }

    /// Opens or closes the settings, as their shortcut does.
    pub fn toggle_settings(&mut self) {
        if self.dialog == Some(Dialog::Settings) {
            self.dialog = None;
        } else {
            self.open_settings(SettingsTab::General);
        }
    }

    /// Follows a drag in the settings. Returns whether it's one.
    pub(super) fn settings_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        self.general_drag_move(event) | self.audio_drag_move(event)
    }

    pub(super) fn settings_drag_end(&mut self) {
        self.general_drag_end();
        self.settings.end_drags();
    }

    pub(super) fn settings_dialog(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = self.theme.clone();
        let close = Flat::ghost("close-settings", CLOSE_SIZE)
            .element(glyph(icons::X, size::HEADING, false), &theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.dialog = None;
                    context.notify();
                }),
            );
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .child(dialog_title("Settings"))
            .child(close);
        let tabs = tab_bar(
            &[
                (SettingsTab::General, "General"),
                (SettingsTab::Appearance, "Appearance"),
                (SettingsTab::Audio, "Audio"),
            ],
            self.settings.tab,
            &theme,
            context,
            |this, tab| this.settings.tab = tab,
        );
        let content = match self.settings.tab {
            SettingsTab::General => self.general_tab(&theme, context),
            SettingsTab::Appearance => self.appearance_tab(&theme, context),
            SettingsTab::Audio => self.audio_tab(&theme, window, context),
        };
        // In a window too short for it, the dialog fits in and its tab
        // scrolls.
        let room = f32::from(window.viewport_size().height) - 2. * WINDOW_MARGIN;
        let content = div()
            .id("settings-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(content.min_h_full().flex().flex_col().gap(px(SECTION_GAP)));
        dialog_frame(WIDTH, &theme)
            .h(px(HEIGHT.min(room)))
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(header.flex_none())
            .child(tabs.flex_none())
            .child(content)
    }
}

/// A section's heading, over a line.
pub(super) fn section_header(text: &'static str, theme: &Theme) -> gpui::Div {
    div()
        .pb(px(2.))
        .border_b_1()
        .border_color(hsla(theme.separator))
        .text_size(px(size::SMALL))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(hsla(theme.text_muted))
        .child(text.to_uppercase())
}

/// A setting: its name, then its control.
fn setting_row(label: &'static str, control: impl IntoElement, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .min_h(px(CONTROL_HEIGHT))
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(hsla(theme.text))
                .child(label),
        )
        .child(control)
}

/// Settings one under the other.
fn setting_rows(rows: impl IntoIterator<Item = gpui::Div>) -> gpui::Div {
    div().flex().flex_col().gap(px(ROW_GAP)).children(rows)
}

/// What's running, under a section: small and muted.
fn status(text: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .text_size(px(size::SMALL))
        .text_color(hsla(theme.text_muted))
        .child(text.into())
}

/// The buttons at the bottom of a tab, on a line.
fn button_row(theme: &Theme) -> gpui::Div {
    div()
        .mt_auto()
        .pt(px(SECTION_GAP))
        .border_t_1()
        .border_color(hsla(theme.separator))
        .flex()
        .items_center()
        .gap(px(ROW_GAP))
}
