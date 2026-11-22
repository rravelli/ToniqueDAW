//! The Audio tab: the output device, its sample rate and buffer, and the
//! engine's block size and threads. Changes wait in the draft until
//! applied, which restarts the audio.

use crate::model::HoldsProject;
use gpui::{App, Context, MouseButton, MouseMoveEvent, Window, div, prelude::*, px};

use crate::{
    app::{
        PopupKind, Tonique,
        settings::{
            CONTROL_HEIGHT, CONTROL_WIDTH, button_row, section_header, setting_row, setting_rows,
            status,
        },
    },
    audio::host::{open_output, output_devices},
    config::settings::Settings,
    icons,
    theme::{Theme, hsla, size},
    widgets::{Flat, Menu, MenuItem, NumberField, NumberSpec, dialog_button, glyph, select},
};
use tonique_engine::device::{DeviceInfo, DeviceOptions};

const BUFFER_SIZES: [u32; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];
const BLOCK_SIZES: [usize; 5] = [64, 128, 256, 512, 1024];
const REFRESH_SIZE: f32 = 20.;
const REFRESH_GAP: f32 = 4.;
const FIELD_WIDTH: f32 = 90.;
/// The rate assumed for buffer lengths before a device runs.
const FALLBACK_RATE: u32 = 48000;

/// A setting picked in a dropdown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioChoice {
    Device,
    SampleRate,
    BufferSize,
    BlockSize,
}

/// What a device can do: the one selected in the draft.
struct DeviceCapabilities {
    device_id: Option<String>,
    sample_rates: Vec<u32>,
    buffer_range: Option<(u32, u32)>,
}

pub struct AudioForm {
    /// Probed when the settings open, or refreshed.
    devices: Vec<DeviceInfo>,
    capabilities: Option<DeviceCapabilities>,
    worker_threads: NumberField,
    parallel_threshold: NumberField,
}

/// `frames` long, and how long that is at `rate`.
fn buffer_label(frames: u32, rate: u32) -> String {
    format!(
        "{frames} frames ({:.1} ms)",
        frames as f32 * 1000. / rate as f32
    )
}

impl AudioForm {
    pub fn new() -> Self {
        let cores = std::thread::available_parallelism().map_or(4, |count| count.get());
        let most_workers = cores.saturating_sub(1).max(1) as f32;
        Self {
            devices: Vec::new(),
            capabilities: None,
            worker_threads: NumberField::new(NumberSpec::range(0., most_workers, 0)),
            parallel_threshold: NumberField::new(NumberSpec::range(1., 512., 0).suffix(" nodes")),
        }
    }

    pub fn refresh_devices(&mut self) {
        self.devices = output_devices();
        self.capabilities = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.worker_threads.is_dragging() || self.parallel_threshold.is_dragging()
    }

    pub fn end_drags(&mut self) {
        self.worker_threads.end_drag();
        self.parallel_threshold.end_drag();
    }

    /// The sample rates and buffer sizes of the device selected in
    /// `draft`, probed again only when the selection changes.
    fn capabilities(&mut self, draft: &Settings) -> &DeviceCapabilities {
        let stale = self
            .capabilities
            .as_ref()
            .is_none_or(|capabilities| capabilities.device_id != draft.device_id);
        if stale {
            let options = DeviceOptions {
                device_id: draft.device_id.clone(),
                ..Default::default()
            };
            let device = open_output(&options).ok();
            self.capabilities = Some(DeviceCapabilities {
                device_id: draft.device_id.clone(),
                sample_rates: device
                    .as_ref()
                    .map(|device| device.supported_sample_rates())
                    .unwrap_or_default(),
                buffer_range: device
                    .as_ref()
                    .and_then(|device| device.buffer_size_range()),
            });
        }
        self.capabilities.as_ref().expect("probed above")
    }
}

impl Tonique {
    pub(super) fn audio_drag_move(&mut self, event: &MouseMoveEvent) -> bool {
        let pointer_x = f32::from(event.position.x);
        let fine = event.modifiers.shift;
        let form = &mut self.settings.audio;
        let draft = &mut self.settings.draft;
        if let Some(workers) =
            form.worker_threads
                .drag_to(pointer_x, fine, draft.worker_threads as f32)
        {
            draft.worker_threads = workers as usize;
        } else if let Some(threshold) =
            form.parallel_threshold
                .drag_to(pointer_x, fine, draft.parallel_threshold as f32)
        {
            draft.parallel_threshold = threshold as usize;
        }
        form.is_dragging()
    }

    /// The sample rate buffer lengths are shown at.
    fn draft_rate(&self, context: &App) -> u32 {
        let running = self.state(context).audio().map(|audio| audio.sample_rate);
        self.settings
            .draft
            .sample_rate
            .or(running)
            .unwrap_or(FALLBACK_RATE)
    }

    /// The label of the device selected in the draft.
    fn device_label(&self) -> String {
        match &self.settings.draft.device_id {
            None => "System default".into(),
            Some(id) => self
                .settings
                .audio
                .devices
                .iter()
                .find(|device| device.id == *id)
                .map_or("Unavailable device".into(), |device| device.name.clone()),
        }
    }

    /// The options of `choice`'s dropdown, the chosen one ticked.
    pub fn audio_choice_menu(&self, choice: AudioChoice, context: &mut Context<Self>) -> gpui::Div {
        let draft = &self.settings.draft;
        let menu = Menu::new(&crate::theme::current(context));
        match choice {
            AudioChoice::Device => {
                let options = std::iter::once((None, "System default".to_string())).chain(
                    self.settings
                        .audio
                        .devices
                        .iter()
                        .map(|device| (Some(device.id.clone()), device.name.clone())),
                );
                options.fold(menu, |menu, (id, name)| {
                    let checked = draft.device_id == id;
                    menu.item(
                        MenuItem::new(name).checked(checked),
                        context,
                        move |this, _, _| {
                            this.settings.draft.device_id = id.clone();
                        },
                    )
                })
            }
            AudioChoice::SampleRate => {
                let rates = self
                    .settings
                    .audio
                    .capabilities
                    .as_ref()
                    .map(|capabilities| capabilities.sample_rates.clone())
                    .unwrap_or_default();
                let options = std::iter::once((None, "Device default".to_string())).chain(
                    rates
                        .into_iter()
                        .map(|rate| (Some(rate), format!("{rate} Hz"))),
                );
                options.fold(menu, |menu, (rate, name)| {
                    let checked = draft.sample_rate == rate;
                    menu.item(
                        MenuItem::new(name).checked(checked),
                        context,
                        move |this, _, _| {
                            this.settings.draft.sample_rate = rate;
                        },
                    )
                })
            }
            AudioChoice::BufferSize => {
                let range = self
                    .settings
                    .audio
                    .capabilities
                    .as_ref()
                    .and_then(|capabilities| capabilities.buffer_range);
                let fits =
                    |frames: &u32| range.is_none_or(|(min, max)| (min..=max).contains(frames));
                let rate = self.draft_rate(context);
                let options = std::iter::once((None, "Device default".to_string())).chain(
                    BUFFER_SIZES
                        .into_iter()
                        .filter(fits)
                        .map(|frames| (Some(frames), buffer_label(frames, rate))),
                );
                options.fold(menu, |menu, (frames, name)| {
                    let checked = draft.buffer_frames == frames;
                    menu.item(
                        MenuItem::new(name).checked(checked),
                        context,
                        move |this, _, _| {
                            this.settings.draft.buffer_frames = frames;
                        },
                    )
                })
            }
            AudioChoice::BlockSize => BLOCK_SIZES.into_iter().fold(menu, |menu, frames| {
                let checked = draft.block_size == frames;
                let item = MenuItem::new(format!("{frames} frames")).checked(checked);
                menu.item(item, context, move |this, _, _| {
                    this.settings.draft.block_size = frames;
                })
            }),
        }
        .into_element()
    }

    /// A dropdown for `choice`, showing `label`.
    fn choice_select(
        &self,
        id: &'static str,
        choice: AudioChoice,
        label: String,
        width: f32,
        theme: &Theme,
        context: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let kind = PopupKind::Setting(choice);
        let open = self.popup.as_ref().is_some_and(|popup| popup.kind == kind);
        select(id, label, width, kind, open, theme, context)
    }

    pub(super) fn audio_tab(
        &mut self,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let draft = self.settings.draft.clone();
        self.settings.audio.capabilities(&draft);
        div()
            .child(self.output_section(theme, context))
            .child(self.engine_section(theme, window, context))
            .child(self.audio_buttons(theme, context))
    }

    fn output_section(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let draft = &self.settings.draft;
        let refresh = Flat::new("refresh-devices", REFRESH_SIZE)
            .element(glyph(icons::ARROWS_CLOCKWISE, 12., false), theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.settings.audio.refresh_devices();
                    context.notify();
                }),
            );
        let device_width = CONTROL_WIDTH - REFRESH_SIZE - REFRESH_GAP;
        let device = div()
            .flex()
            .items_center()
            .gap(px(REFRESH_GAP))
            .child(self.choice_select(
                "device",
                AudioChoice::Device,
                self.device_label(),
                device_width,
                theme,
                context,
            ))
            .child(refresh);
        let rate = draft
            .sample_rate
            .map_or("Device default".into(), |rate| format!("{rate} Hz"));
        let buffer = draft
            .buffer_frames
            .map_or("Device default".into(), |frames| {
                buffer_label(frames, self.draft_rate(context))
            });
        let rows = setting_rows([
            setting_row("Device", device, theme),
            setting_row(
                "Sample rate",
                self.choice_select(
                    "rate",
                    AudioChoice::SampleRate,
                    rate,
                    CONTROL_WIDTH,
                    theme,
                    context,
                ),
                theme,
            ),
            setting_row(
                "Buffer size",
                self.choice_select(
                    "buffer",
                    AudioChoice::BufferSize,
                    buffer,
                    CONTROL_WIDTH,
                    theme,
                    context,
                ),
                theme,
            ),
        ]);
        let running = self.state(context).audio().map(|audio| {
            let buffer = audio
                .buffer_frames
                .map_or("device default".into(), |frames| {
                    buffer_label(frames, audio.sample_rate)
                });
            format!(
                "Running: {} · {} Hz · {} channels · buffer {buffer}",
                audio.device_name, audio.sample_rate, audio.channels
            )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(section_header("Audio output", theme))
            .child(rows)
            .children(running.map(|text| status(text, theme)))
    }

    fn engine_section(
        &self,
        theme: &Theme,
        window: &Window,
        context: &mut Context<Self>,
    ) -> gpui::Div {
        let draft = &self.settings.draft;
        let form = &self.settings.audio;
        let field = (FIELD_WIDTH, CONTROL_HEIGHT);
        let well = |element: gpui::AnyElement| {
            div().rounded(px(2.)).bg(hsla(theme.bg_deep)).child(element)
        };
        let workers = form.worker_threads.view(
            "workers",
            (
                draft.worker_threads as f32,
                crate::app::arrows::holds_number("workers", context),
            ),
            field,
            theme,
            window,
            context,
            |this| &mut this.settings.audio.worker_threads,
            |this, workers, _| this.settings.draft.worker_threads = workers as usize,
        );
        let threshold = form.parallel_threshold.view(
            "threshold",
            (
                draft.parallel_threshold as f32,
                crate::app::arrows::holds_number("threshold", context),
            ),
            field,
            theme,
            window,
            context,
            |this| &mut this.settings.audio.parallel_threshold,
            |this, threshold, _| this.settings.draft.parallel_threshold = threshold as usize,
        );
        let block = format!("{} frames", draft.block_size);
        let rows = setting_rows([
            setting_row(
                "Processing block",
                self.choice_select(
                    "block",
                    AudioChoice::BlockSize,
                    block,
                    CONTROL_WIDTH,
                    theme,
                    context,
                ),
                theme,
            ),
            setting_row("Worker threads", well(workers), theme),
            setting_row("Parallel threshold", well(threshold), theme),
        ]);
        let config = self.state(context).engine_config();
        let nodes = self
            .state(context)
            .graph
            .topology()
            .map_or(String::new(), |topology| {
                format!(" · {} graph nodes", topology.nodes.len())
            });
        let running = format!(
            "Running: {} Hz · block {} · {} workers · load {:.0}%{nodes}",
            config.sample_rate,
            config.max_block,
            config.worker_threads,
            self.state(context).cpu_load() * 100.
        );
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(section_header("Engine", theme))
            .child(rows)
            .child(status(running, theme))
    }

    /// Apply or revert the draft's audio changes, or fill it with the
    /// defaults; above, what went wrong last.
    fn audio_buttons(&self, theme: &Theme, context: &mut Context<Self>) -> gpui::Div {
        let pending = self
            .state(context)
            .settings()
            .audio_differs(&self.settings.draft);
        let error = self.state(context).audio_error.as_ref().map(|error| {
            let text = if self.state(context).audio_lost() {
                format!("No audio output ({error}), retrying…")
            } else {
                format!("Couldn't apply audio settings: {error}")
            };
            div()
                .text_size(px(size::SMALL))
                .text_color(hsla(theme.danger))
                .child(text)
        });
        let enabled = |button: gpui::Stateful<gpui::Div>| {
            if pending { button } else { button.opacity(0.5) }
        };
        let apply = enabled(dialog_button(
            "apply-audio",
            "Apply audio changes",
            true,
            theme,
        ));
        let apply = if pending {
            apply.on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    // Only the audio fields: the others are already applied.
                    let mut settings = this.state(context).settings().clone();
                    settings.set_audio(&this.settings.draft);
                    this.state_mut(context).apply_settings(settings);
                    this.settings.draft = this.state(context).settings().clone();
                    context.notify();
                }),
            )
        } else {
            apply
        };
        let revert = enabled(dialog_button("revert-audio", "Revert", false, theme));
        let revert = if pending {
            revert.on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.settings.draft = this.state(context).settings().clone();
                    context.notify();
                }),
            )
        } else {
            revert
        };
        // Only fills the draft: applying still restarts the audio.
        let restore = dialog_button("restore-audio", "Restore defaults", false, theme)
            .on_mouse_down(
                MouseButton::Left,
                context.listener(|this, _, _, context| {
                    this.settings.draft.set_audio(&Settings::default());
                    context.notify();
                }),
            );
        div()
            .mt_auto()
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(error)
            .child(
                button_row(theme)
                    .mt_0()
                    .child(apply)
                    .child(revert)
                    .child(div().flex_1())
                    .child(restore),
            )
    }
}
