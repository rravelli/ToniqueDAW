//! New, open and save as the core's `ProjectFiles` runs them; here, their
//! dialogs: the file pickers, the prompt about unsaved changes (also on
//! closing the window), notices about what went wrong, and the window's
//! title.

use crate::model::HoldsProject;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{
    App, Context, MouseButton, PathPromptOptions, PromptLevel, Stateful, Window, div, prelude::*,
    px,
};

use crate::{
    app::{Workspace, dialog::Dialog},
    widgets::{dialog_body, dialog_button, dialog_frame, dialog_title},
};
pub use tonique_core::project::files::ProjectAction;
use tonique_core::project::files::{Ask, FileOutcome};

/// The window's title, as last set.
#[derive(Default)]
pub struct WindowTitle {
    shown: String,
    /// When it last looked for unsaved changes: comparing the project
    /// with the saved one takes too long to do every frame.
    checked: Option<Instant>,
}

/// How long the title's star may lag an edit.
const TITLE_CHECK_INTERVAL: Duration = Duration::from_millis(250);

/// The egui dialog's 340 wide content, with the frame's margins and border.
const UNSAVED_WIDTH: f32 = 340. + 2. * 18. + 2.;

impl Workspace {
    pub fn project_action(
        &mut self,
        action: ProjectAction,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let outcome = self
            .files
            .request(action, &mut self.project.state_mut(context));
        self.follow_files(outcome, window, context);
    }

    /// Does what a step of the project's files left to the window.
    fn follow_files(
        &mut self,
        outcome: FileOutcome,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        // Saved or opened: the title shows it now.
        self.title.checked = None;
        if outcome.replaced {
            self.forget_old_project(context);
        }
        let mut notice = outcome.notice;
        let missing = outcome.missing_files;
        if missing > 0 {
            self.show_missing_files(None, context);
            let notice = notice.get_or_insert_with(|| tonique_core::project::files::Notice {
                title: format!("“{}” opened with problems", self.files.name()),
                lines: Vec::new(),
            });
            notice.lines.push(format!(
                "{missing} audio file{} not found: see the browser's Missing tab.",
                if missing == 1 { " was" } else { "s were" }
            ));
        }
        if let Some(notice) = notice {
            self.notice(&notice.title, &notice.lines, window, context);
        }
        if outcome.close {
            window.remove_window();
            return;
        }
        match outcome.ask {
            None => {}
            Some(Ask::Unsaved) => {
                self.dialog = Some(Dialog::Unsaved);
                // Enter answers it, not a focused list or field.
                window.focus(&self.focus, context);
            }
            Some(Ask::ProjectToOpen { .. }) => self.pick_project(window, context),
            Some(Ask::SavePath {
                start_dir,
                file_name,
            }) => self.pick_save_path(start_dir, file_name, window, context),
        }
    }

    /// The answer about unsaved changes: save them first (`Some(true)`),
    /// let them go (`Some(false)`), or cancel.
    pub(super) fn answer_unsaved(
        &mut self,
        save: Option<bool>,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        self.dialog = None;
        window.focus(&self.focus, context);
        let outcome = self
            .files
            .answer_unsaved(save, &mut self.project.state_mut(context));
        self.follow_files(outcome, window, context);
    }

    /// The question about unsaved changes, as the egui app's: saving on
    /// the right, where the eye ends; losing the changes apart on the
    /// left, so it isn't clicked by mistake.
    pub(super) fn unsaved_dialog(&self, context: &mut Context<Self>) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let answer = |button: Stateful<gpui::Div>, save: Option<bool>| {
            button.on_mouse_down(
                MouseButton::Left,
                context.listener(move |this, _, window, context| {
                    this.answer_unsaved(save, window, context);
                    context.notify();
                }),
            )
        };
        let dont_save = answer(
            dialog_button("dont-save", "Don't save", false, theme),
            Some(false),
        );
        let cancel = answer(dialog_button("cancel", "Cancel", false, theme), None);
        let save = answer(dialog_button("save", "Save", true, theme), Some(true));
        dialog_frame(UNSAVED_WIDTH, theme)
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(dialog_title(format!(
                "Save changes to “{}”?",
                self.files.name()
            )))
            .child(dialog_body(
                "Your changes will be lost if you don't save them.",
                theme,
            ))
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(dont_save)
                    .child(div().flex_1())
                    .child(cancel)
                    .child(save),
            )
    }

    /// Lets the user pick a project, then opens it.
    fn pick_project(&mut self, window: &mut Window, context: &mut Context<Self>) {
        let picked = context.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        context
            .spawn_in(window, async move |this, context| {
                let path = match picked.await {
                    Ok(Ok(Some(paths))) => paths.into_iter().next(),
                    _ => None,
                };
                this.update_in(context, |this, window, context| {
                    let outcome = this
                        .files
                        .opened(path, &mut this.project.state_mut(context));
                    this.follow_files(outcome, window, context);
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    /// Lets the user choose where to save, then saves there.
    fn pick_save_path(
        &mut self,
        start_dir: PathBuf,
        file_name: String,
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let picked = context.prompt_for_new_path(&start_dir, Some(&file_name));
        context
            .spawn_in(window, async move |this, context| {
                let path = match picked.await {
                    Ok(Ok(path)) => path,
                    _ => None,
                };
                this.update_in(context, |this, window, context| {
                    let outcome = this
                        .files
                        .saved_to(path, &mut this.project.state_mut(context));
                    this.follow_files(outcome, window, context);
                    context.notify();
                })
                .ok();
            })
            .detach();
    }

    /// Tells the user what went wrong, until dismissed.
    fn notice(
        &self,
        title: &str,
        lines: &[String],
        window: &mut Window,
        context: &mut Context<Self>,
    ) {
        let detail = lines.join("\n");
        // Only dismissed: the answer doesn't matter.
        drop(window.prompt(
            PromptLevel::Critical,
            title,
            Some(&detail),
            &["OK"],
            context,
        ));
    }

    /// What referred to the replaced project's tracks and clips no longer
    /// does.
    fn forget_old_project(&mut self, context: &mut App) {
        crate::popup::close(context);
        self.arrangement
            .update(context, |arrangement, _| arrangement.forget_project());
        self.bottom
            .update(context, |bottom, _| bottom.forget_project());
    }

    /// The window's title: the project's name, starred while unsaved;
    /// looked at a few times a second.
    pub(super) fn update_title(&mut self, window: &mut Window, context: &mut App) {
        let now = Instant::now();
        if self
            .title
            .checked
            .is_some_and(|checked| now - checked < TITLE_CHECK_INTERVAL)
        {
            return;
        }
        self.title.checked = Some(now);
        let title = self.files.title(self.state(context));
        if title != self.title.shown {
            window.set_window_title(&title);
            self.title.shown = title;
        }
    }

    /// Asks about unsaved changes before the window closes. Returns
    /// whether it can close now.
    pub(super) fn should_close(
        &mut self,
        window: &mut Window,
        context: &mut Context<Self>,
    ) -> bool {
        if self.files.should_close(self.state(context)) {
            return true;
        }
        self.dialog = Some(Dialog::Unsaved);
        window.focus(&self.focus, context);
        false
    }
}
