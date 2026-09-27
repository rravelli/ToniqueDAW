use crate::{
    config::settings::{Settings, UI_SCALE_RANGE},
    core::state::ToniqueProjectState,
    ui::{
        font::PHOSPHOR_REGULAR,
        panels::menu_bar::set_ui_scale,
        theme::PRIMARY_COLOR,
        widget::{
            input::NumberInput, section::SectionHeader, select::Select, slider::ValueSlider,
            square_button::SquareButton, tab_bar::TabBar,
        },
        windows::shortcuts::UIShortcutsTab,
    },
};
use egui::{
    Color32, FontFamily, FontId, Frame, Grid, Layout, Margin, Rangef, RichText, Ui, Vec2, vec2,
};
use egui_phosphor::regular::ARROWS_CLOCKWISE;
use tonique_engine::device::{DeviceInfo, DeviceOptions, OutputDevice, output_devices};

const BUFFER_SIZES: [u32; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];
const BLOCK_SIZES: [usize; 5] = [64, 128, 256, 512, 1024];
const LABEL_WIDTH: f32 = 120.;
const CONTROL_WIDTH: f32 = 240.;
const BUTTON_HEIGHT: f32 = 22.;
const ERROR_COLOR: Color32 = Color32::from_rgb(255, 110, 110);
const WINDOW_WIDTH: f32 = 440.;

#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    General,
    Audio,
    Shortcuts,
}

/// What a device can do, for the device selected in the form.
struct DeviceCapabilities {
    device_id: Option<String>,
    sample_rates: Vec<u32>,
    buffer_range: Option<(u32, u32)>,
}

/// Preferences window. Interface, metronome and shortcut changes apply
/// immediately; audio device and engine changes are collected in a draft
/// and applied together, since they restart the audio.
pub struct UISettingsWindow {
    pub open: bool,
    tab: SettingsTab,
    shortcuts: UIShortcutsTab,
    draft: Settings,
    /// Probed when the window opens (probing hardware is slow).
    devices: Vec<DeviceInfo>,
    capabilities: Option<DeviceCapabilities>,
    parallel_threshold: NumberInput,
}

impl UISettingsWindow {
    pub fn new() -> Self {
        Self {
            open: false,
            tab: SettingsTab::General,
            shortcuts: UIShortcutsTab::new(),
            draft: Settings::default(),
            devices: Vec::new(),
            capabilities: None,
            parallel_threshold: NumberInput::new(vec2(90., 20.))
                .fill(Color32::from_gray(58))
                .with_range(Rangef::new(1., 512.))
                .decimals(0)
                .suffix(" nodes"),
        }
    }

    pub fn open(&mut self, state: &ToniqueProjectState) {
        self.open = true;
        self.draft = state.settings().clone();
        self.shortcuts.cancel();
        self.refresh_devices();
    }

    pub fn toggle(&mut self, state: &ToniqueProjectState) {
        if self.open {
            self.open = false;
            self.shortcuts.cancel();
        } else {
            self.open(state);
        }
    }

    fn refresh_devices(&mut self) {
        self.devices = output_devices();
        self.capabilities = None;
    }

    pub fn show(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .frame(Frame::window(ui.style()).inner_margin(Margin::same(5)))
            .show(ui.ctx(), |ui| {
                ui.set_width(WINDOW_WIDTH);
                ui.spacing_mut().item_spacing = vec2(6., 4.);
                let tabs = ui.add(TabBar::new(
                    &mut self.tab,
                    [
                        (SettingsTab::General, "General"),
                        (SettingsTab::Audio, "Audio"),
                        (SettingsTab::Shortcuts, "Shortcuts"),
                    ],
                ));
                if tabs.changed() {
                    self.shortcuts.cancel();
                }
                ui.add_space(10.);
                match self.tab {
                    SettingsTab::General => {
                        self.interface_section(ui, state);
                        ui.add_space(10.);
                        self.general_buttons(ui, state);
                    }
                    SettingsTab::Audio => {
                        self.audio_section(ui, state);
                        ui.add_space(10.);
                        self.engine_section(ui, state);
                        ui.add_space(10.);
                        self.audio_buttons(ui, state);
                    }
                    SettingsTab::Shortcuts => self.shortcuts.show(ui, state),
                }
            });
        if !open {
            self.shortcuts.cancel();
        }
        self.open = open;
    }

    fn interface_section(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.add(SectionHeader::new("Interface"));
        settings_grid(ui, "settings-interface", |ui| {
            row_label(ui, "UI scale", "Also Ctrl + / Ctrl - / Ctrl 0.");
            // Rescaling while dragging would move the slider under the
            // pointer: apply when the drag ends.
            let response = ui.add(
                ValueSlider::new(&mut self.draft.ui_scale, UI_SCALE_RANGE)
                    .step(0.05)
                    .default_value(1.)
                    .width(CONTROL_WIDTH)
                    .percent(),
            );
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                set_ui_scale(ui.ctx(), state, self.draft.ui_scale);
            }
            ui.end_row();

            row_label(ui, "Metronome level", "Volume of the metronome click.");
            let response = ui.add(
                ValueSlider::new(&mut self.draft.metronome_level, 0.0..=1.0)
                    .step(0.01)
                    .default_value(Settings::default().metronome_level)
                    .width(CONTROL_WIDTH)
                    .percent(),
            );
            if response.changed() {
                let mut settings = state.settings().clone();
                settings.metronome_level = self.draft.metronome_level;
                state.apply_settings(settings);
            }
            ui.end_row();
        });
    }

    fn audio_section(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.add(SectionHeader::new("Audio output"));
        let caps = self.capabilities();
        settings_grid(ui, "settings-audio", |ui| {
            row_label(ui, "Device", "Output device used for playback.");
            ui.horizontal(|ui| {
                let unavailable = self
                    .draft
                    .device_id
                    .as_ref()
                    .filter(|id| !self.devices.iter().any(|d| d.id == **id))
                    .map(|id| (Some(id.clone()), "Unavailable device".to_string()));
                ui.add(
                    Select::new("settings-device", &mut self.draft.device_id)
                        .width(CONTROL_WIDTH - BUTTON_HEIGHT - 4.)
                        .option(None, "System default")
                        .options(unavailable)
                        .options(
                            self.devices
                                .iter()
                                .map(|d| (Some(d.id.clone()), d.name.clone())),
                        ),
                );
                ui.add_space(4.);
                if ui
                    .add(
                        SquareButton::new(ARROWS_CLOCKWISE)
                            .square(BUTTON_HEIGHT - 2.)
                            .fill(Color32::from_gray(58))
                            .font(FontId::new(12., FontFamily::Name(PHOSPHOR_REGULAR.into())))
                            .border_radius(2.)
                            .tooltip("Refresh devices"),
                    )
                    .clicked()
                {
                    self.refresh_devices();
                }
            });
            ui.end_row();

            row_label(ui, "Sample rate", "Rate the device runs at.");
            ui.add(
                Select::new("settings-rate", &mut self.draft.sample_rate)
                    .width(CONTROL_WIDTH)
                    .option(None, "Device default")
                    .options(
                        caps.sample_rates
                            .iter()
                            .map(|r| (Some(*r), format!("{r} Hz"))),
                    ),
            );
            ui.end_row();

            row_label(
                ui,
                "Buffer size",
                "Frames per device callback. Smaller buffers lower latency but risk dropouts.",
            );
            let fits = |n: &u32| {
                caps.buffer_range
                    .is_none_or(|(min, max)| (min..=max).contains(n))
            };
            let rate = self
                .draft
                .sample_rate
                .or(state.audio().map(|a| a.sample_rate))
                .unwrap_or(48000) as f32;
            let unsupported = self
                .draft
                .buffer_frames
                .map_or(String::new(), |n| format!("{n} frames (unsupported)"));
            ui.add(
                Select::new("settings-buffer", &mut self.draft.buffer_frames)
                    .width(CONTROL_WIDTH)
                    .option(None, "Device default")
                    .options(BUFFER_SIZES.iter().filter(|n| fits(n)).map(|n| {
                        (
                            Some(*n),
                            format!("{n} frames ({:.1} ms)", *n as f32 * 1000. / rate),
                        )
                    }))
                    .placeholder(unsupported),
            );
            ui.end_row();
        });
        self.capabilities = Some(caps);

        if let Some(audio) = state.audio() {
            let buffer = match audio.buffer_frames {
                Some(n) => format!(
                    "{n} frames ({:.1} ms)",
                    n as f32 * 1000. / audio.sample_rate as f32
                ),
                None => "device default".into(),
            };
            status(
                ui,
                format!(
                    "Running: {} · {} Hz · {} channels · buffer {buffer}",
                    audio.device_name, audio.sample_rate, audio.channels
                ),
            );
        }
    }

    fn engine_section(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.add(SectionHeader::new("Engine"));
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        settings_grid(ui, "settings-engine", |ui| {
            row_label(
                ui,
                "Processing block",
                "Largest block the engine renders at once. Device buffers are split into blocks this size.",
            );
            ui.add(
                Select::new("settings-block", &mut self.draft.block_size)
                    .width(CONTROL_WIDTH)
                    .options(BLOCK_SIZES.map(|n| (n, format!("{n} frames")))),
            );
            ui.end_row();

            row_label(
                ui,
                "Worker threads",
                "Helper threads that process independent parts of the graph in parallel. 0 runs everything on the audio thread.",
            );
            ui.add(
                ValueSlider::new(
                    &mut self.draft.worker_threads,
                    0..=cores.saturating_sub(1).max(1),
                )
                .default_value(Settings::default().worker_threads)
                .width(CONTROL_WIDTH),
            );
            ui.end_row();

            row_label(
                ui,
                "Parallel threshold",
                "Graphs with fewer nodes run on the audio thread alone: below this, coordinating threads costs more than it saves.",
            );
            self.parallel_threshold.value = self.draft.parallel_threshold as f32;
            self.parallel_threshold.ui(ui);
            self.draft.parallel_threshold = self.parallel_threshold.value.round() as usize;
            ui.end_row();
        });

        let config = state.engine_config();
        let nodes = state.graph.topology().map_or(String::new(), |t| {
            format!(" · {} graph nodes", t.nodes.len())
        });
        status(
            ui,
            format!(
                "Running: {} Hz · block {} · {} workers · load {:.0}%{nodes}",
                config.sample_rate,
                config.max_block,
                config.worker_threads,
                state.cpu_load() * 100.
            ),
        );
    }

    fn general_buttons(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        ui.separator();
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(secondary_button("Restore defaults")).clicked() {
                let defaults = Settings::default();
                self.draft.ui_scale = defaults.ui_scale;
                self.draft.metronome_level = defaults.metronome_level;
                set_ui_scale(ui.ctx(), state, self.draft.ui_scale);
                let mut settings = state.settings().clone();
                settings.metronome_level = self.draft.metronome_level;
                state.apply_settings(settings);
            }
        });
    }

    fn audio_buttons(&mut self, ui: &mut Ui, state: &mut ToniqueProjectState) {
        let pending = state.settings().audio_differs(&self.draft);
        if let Some(error) = &state.audio_error {
            ui.label(
                RichText::new(format!("Couldn't apply audio settings: {error}"))
                    .color(ERROR_COLOR)
                    .small(),
            );
            ui.add_space(4.);
        }
        ui.separator();
        ui.horizontal(|ui| {
            let apply = SquareButton::new("Apply audio changes")
                .size(vec2(0., BUTTON_HEIGHT))
                .padding(10.)
                .font(FontId::proportional(12.))
                .fill(PRIMARY_COLOR)
                .color(Color32::BLACK)
                .border_radius(2.);
            if ui
                .add_enabled(pending, apply)
                .on_hover_text("Restarts the audio output. The playhead and undo history are kept.")
                .clicked()
            {
                // Only the audio fields: the others are already applied.
                let mut settings = state.settings().clone();
                settings.set_audio(&self.draft);
                state.apply_settings(settings);
                self.draft = state.settings().clone();
            }
            if ui
                .add_enabled(pending, secondary_button("Revert"))
                .clicked()
            {
                self.draft = state.settings().clone();
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // Only fills the draft: applying still restarts the audio.
                if ui.add(secondary_button("Restore defaults")).clicked() {
                    self.draft.set_audio(&Settings::default());
                }
            });
        });
    }

    /// Sample rates and buffer sizes of the device selected in the draft,
    /// probed again only when the selection changes.
    fn capabilities(&mut self) -> DeviceCapabilities {
        match self.capabilities.take() {
            Some(caps) if caps.device_id == self.draft.device_id => caps,
            _ => {
                let options = DeviceOptions {
                    device_id: self.draft.device_id.clone(),
                    ..Default::default()
                };
                let device = OutputDevice::open(&options).ok();
                DeviceCapabilities {
                    device_id: self.draft.device_id.clone(),
                    sample_rates: device
                        .as_ref()
                        .map(|d| d.supported_sample_rates())
                        .unwrap_or_default(),
                    buffer_range: device.as_ref().and_then(|d| d.buffer_size_range()),
                }
            }
        }
    }
}

fn settings_grid(ui: &mut Ui, id: &str, content: impl FnOnce(&mut Ui)) {
    Grid::new(id)
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([12., 6.])
        .show(ui, content);
}

fn row_label(ui: &mut Ui, text: &str, hint: &str) {
    ui.label(RichText::new(text).color(Color32::from_gray(200)))
        .on_hover_text(hint);
}

fn status(ui: &mut Ui, text: String) {
    ui.add_space(2.);
    ui.label(RichText::new(text).small().color(Color32::GRAY));
}

fn secondary_button(text: &str) -> SquareButton {
    SquareButton::new(text)
        .size(Vec2::new(0., BUTTON_HEIGHT))
        .padding(10.)
        .font(FontId::proportional(12.))
        .fill(Color32::from_gray(58))
        .border_radius(2.)
}
