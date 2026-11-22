//! Dialogs, as the egui app's modals: the window dimmed behind, the dialog
//! centred, under only the menus its dropdowns open. Escape closes it, and
//! a click beside it closes those that hold nothing to lose.

use std::sync::{Arc, LazyLock};

use gpui::{
    AnyElement, Context, Image, ImageFormat, MouseButton, Window, anchored, deferred, div, img,
    point, prelude::*, px,
};

use crate::{
    app::Tonique,
    theme::{Theme, hsla, size},
    widgets::{dialog_body, dialog_button, dialog_frame, dialog_title},
};

/// Which dialog.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dialog {
    /// The app's name, version and credits, from the Help menu.
    About,
    Settings,
    /// Exporting audio, from the File menu.
    Export,
    /// Save, let go or keep the unsaved changes, before what waits for them.
    Unsaved,
}

impl Dialog {
    /// Whether a click beside it closes it: not while changes are being
    /// made in it.
    fn closes_on_backdrop(self) -> bool {
        match self {
            Self::About => true,
            Self::Settings => false,
            Self::Export => true,
            Self::Unsaved => true,
        }
    }
}

/// Over the window, under the menus.
const PRIORITY: usize = 1;
const ABOUT_WIDTH: f32 = 300.;
const LOGO_SIZE: f32 = 72.;

/// Third-party work shipped with the app, credited as their licences ask.
const CREDITS: &[(&str, &str)] = &[
    (
        "Inter typeface",
        "The Inter Project Authors, SIL Open Font License 1.1",
    ),
    ("Phosphor Icons", "Phosphor Icons, MIT License"),
];

static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    let svg = include_bytes!("../../../../images/logo.svg");
    Arc::new(Image::from_bytes(ImageFormat::Svg, svg.to_vec()))
});

impl Tonique {
    /// The open dialog over the dimmed window.
    pub(super) fn dialog_view(
        &mut self,
        window: &Window,
        context: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // A running export stays in sight until it ends.
        if self.export.is_running() {
            self.dialog = Some(Dialog::Export);
        }
        let dialog = self.dialog?;
        let theme = crate::theme::current(context);
        let content = match dialog {
            Dialog::About => about(&theme, context),
            Dialog::Settings => div().child(self.settings.clone()),
            Dialog::Export => self.export_dialog(window, context),
            Dialog::Unsaved => self.unsaved_dialog(context),
        };
        let size = window.viewport_size();
        let backdrop = div()
            .id("dialog-backdrop")
            .w(size.width)
            .h(size.height)
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(theme.shadow))
            .occlude()
            .on_any_mouse_down(context.listener(move |this, _, window, context| {
                context.stop_propagation();
                if dialog.closes_on_backdrop() {
                    this.close_dialog(window, context);
                    context.notify();
                }
            }))
            .child(
                content
                    .id("dialog")
                    // Its own clicks stay in it.
                    .on_any_mouse_down(|_, _, context| context.stop_propagation()),
            );
        let layer = anchored().position(point(px(0.), px(0.))).child(backdrop);
        Some(deferred(layer).priority(PRIORITY).into_any_element())
    }
}

impl Tonique {
    /// Closes the open dialog, the keyboard going back to the app; not an
    /// export while it runs.
    pub(super) fn close_dialog(&mut self, window: &mut Window, context: &mut Context<Self>) {
        if self.dialog == Some(Dialog::Export) && !self.export_closes() {
            return;
        }
        self.dialog = None;
        window.focus(&self.focus, context);
    }
}

/// The logo, the app's name and version, and the credits.
fn about(theme: &Theme, context: &mut Context<Tonique>) -> gpui::Div {
    let version = format!("gpui-fast prototype, version {}", env!("CARGO_PKG_VERSION"));
    let credits = CREDITS.iter().map(|(work, credit)| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .child(div().text_size(px(size::LABEL)).child(*work))
            .child(
                div()
                    .text_size(px(size::SMALL))
                    .text_color(hsla(theme.text_muted))
                    .child(*credit),
            )
    });
    let close = dialog_button("close", "Close", true, theme).on_mouse_down(
        MouseButton::Left,
        context.listener(|this, _, _, context| {
            this.dialog = None;
            context.notify();
        }),
    );
    dialog_frame(ABOUT_WIDTH, theme)
        .flex()
        .flex_col()
        .items_center()
        .child(img(LOGO.clone()).size(px(LOGO_SIZE)))
        .child(dialog_title("Tonique").mt(px(8.)))
        .child(dialog_body(version, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .mt(px(12.))
                .children(credits),
        )
        .child(div().mt(px(12.)).child(close))
}
