//! The settings dialog, as the egui app's settings window: the General tab
//! (the metronome's level) and the Appearance tab (the theme) apply at
//! once; the Audio tab's device and engine settings are gathered in a
//! draft and applied together, since they restart the audio. Settings are
//! Tonique's own, saved where the egui app reads them.
//!
//! The Shortcuts tab's changes apply at once too.
//!
//! Left out, as this app hasn't it: the interface's scale.

mod appearance;
mod audio;
mod general;
mod shortcuts;

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, SharedString, Window, div, prelude::*, px,
};

use crate::{
    app::{PopupKind, keys::shortcut},
    icons,
    model::{HoldsProject, Project},
    popup::{self, MenuBuilder},
    theme::{Theme, ThemeLibrary, hsla, size},
    widgets::{Flat, dialog_frame, dialog_title, follow_drag, glyph, tab_bar},
};
use appearance::AppearanceForm;
pub use audio::AudioChoice;
use audio::AudioForm;
use general::GeneralForm;
use shortcuts::ShortcutsForm;
use tonique_core::config::settings::Settings;

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
    Shortcuts,
}

/// The dialog, a view of its own.
pub struct SettingsDialog {
    project: Entity<Project>,
    /// Takes every key while a shortcut is recorded.
    recording_focus: FocusHandle,
    tab: SettingsTab,
    /// The settings being edited: the audio ones apply on demand.
    draft: Settings,
    general: GeneralForm,
    appearance: AppearanceForm,
    audio: AudioForm,
    shortcuts: ShortcutsForm,
}

/// What the dialog asks of the window.
pub enum SettingsEvent {
    Close,
}

impl EventEmitter<SettingsEvent> for SettingsDialog {}

impl HoldsProject for SettingsDialog {
    fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

impl SettingsDialog {
    /// The settings, choosing among `themes`; `theme_warnings` tell what
    /// went wrong loading the theme in use.
    pub fn new(
        project: Entity<Project>,
        themes: ThemeLibrary,
        theme_warnings: Vec<String>,
        context: &mut Context<Self>,
    ) -> Self {
        Self {
            project,
            recording_focus: context.focus_handle(),
            tab: SettingsTab::General,
            draft: Settings::default(),
            general: GeneralForm::default(),
            appearance: AppearanceForm::new(themes, theme_warnings),
            audio: AudioForm::new(),
            shortcuts: ShortcutsForm::default(),
        }
    }

    fn is_dragging(&self) -> bool {
        self.general.is_dragging() || self.audio.is_dragging()
    }
}

impl SettingsDialog {
    /// Shows `tab`, from the settings in use.
    pub fn open(&mut self, tab: SettingsTab, context: &mut App) {
        self.tab = tab;
        self.draft = self.state(context).settings().clone();
        self.appearance.open(&self.draft.theme);
        self.shortcuts.cancel();
        // Probing the hardware is slow: once, on opening.
        self.audio.refresh_devices();
    }

    /// Follows a drag in the settings.
    fn drag_move(&mut self, event: &MouseMoveEvent, context: &mut Context<Self>) {
        self.general_drag_move(event, context);
        self.audio_drag_move(event);
    }

    fn drag_end(&mut self, _: &MouseUpEvent, _: &mut Context<Self>) {
        self.general_drag_end();
        self.audio.end_drags();
    }

    /// Builds one of the dialog's menus each frame.
    fn menu_builder(&self, kind: PopupKind, context: &Context<Self>) -> MenuBuilder {
        popup::builder(context.weak_entity(), move |this, _, context| match kind {
            PopupKind::Setting(choice) => Some(this.audio_choice_menu(choice, context)),
            PopupKind::Theme => Some(this.theme_menu(context)),
            _ => None,
        })
    }

    /// The shortcut being recorded takes every key, Esc included.
    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, context: &mut Context<Self>) {
        if !self.shortcuts.is_recording() {
            return;
        }
        if !event.is_held
            && let Some((key, modifiers)) = shortcut(&event.keystroke)
        {
            self.record_shortcut(key, modifiers, context);
        }
        context.stop_propagation();
        context.notify();
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, context: &mut Context<Self>) -> impl IntoElement {
        let theme = crate::theme::current(context);
        let close = Flat::ghost("close-settings", CLOSE_SIZE)
            .element(glyph(icons::X, size::HEADING, false), &theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|_, _, _, context| context.emit(SettingsEvent::Close)),
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
                (SettingsTab::Shortcuts, "Shortcuts"),
            ],
            self.tab,
            &theme,
            context,
            |this, tab| {
                this.tab = tab;
                this.shortcuts.cancel();
            },
        );
        let content = match self.tab {
            SettingsTab::General => self.general_tab(&theme, context),
            SettingsTab::Appearance => self.appearance_tab(&theme, context),
            SettingsTab::Audio => self.audio_tab(&theme, window, context),
            SettingsTab::Shortcuts => self.shortcuts_tab(&theme, context),
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
        let dragging = self.is_dragging();
        let pressed = context.weak_entity();
        dialog_frame(WIDTH, &theme)
            .track_focus(&self.recording_focus)
            .on_key_down(context.listener(Self::on_key))
            .h(px(HEIGHT.min(room)))
            .flex()
            .flex_col()
            .gap(px(SECTION_GAP))
            .child(header.flex_none())
            .child(tabs.flex_none())
            .child(content)
            // A press anywhere stops recording a shortcut.
            .child(
                gpui::canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, context| {
                            if phase == gpui::DispatchPhase::Capture {
                                let _ = pressed.update(context, |this, context| {
                                    if this.shortcuts.is_recording() {
                                        this.shortcuts.stop_recording();
                                        context.notify();
                                    }
                                });
                            }
                        });
                    },
                )
                .absolute()
                .size_0(),
            )
            .when(dragging, |element| {
                element.child(follow_drag(
                    context.weak_entity(),
                    Self::drag_move,
                    Self::drag_end,
                ))
            })
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
