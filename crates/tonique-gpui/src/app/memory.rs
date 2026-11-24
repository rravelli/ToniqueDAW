//! In debug builds, what memory the app takes: the process's, then what
//! the audio files' data and the waveform tiles hold. Opened by clicking
//! the frame rate in the menu bar.

use crate::model::HoldsProject;
use gpui::{Context, SharedString, div, prelude::*, px};
use tonique_core::dev::memory::AudioMemory;

use crate::{
    app::Workspace,
    theme::{Theme, hsla},
    wave::WaveMemory,
    widgets::Menu,
};
use tonique_core::library::cache::AUDIO_ANALYSIS_CACHE;

/// Between a row's name and its value.
const VALUE_GAP: f32 = 24.;

/// The memory the process has in RAM (its resident set), in bytes. Only
/// known on Linux.
pub fn resident_bytes() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kilobytes: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024)
}

/// `bytes` in the largest unit it has one of, as "18.4 MB".
pub fn format_bytes(bytes: usize) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A count of things and what they take, as "48 · 18.4 MB".
fn count_and_bytes(count: usize, bytes: usize) -> String {
    format!("{count} · {}", format_bytes(bytes))
}

fn row(theme: &Theme, name: &str, value: String) -> gpui::Div {
    div()
        .flex()
        .justify_between()
        .gap(px(VALUE_GAP))
        .px(px(3.))
        .py(px(1.))
        .whitespace_nowrap()
        .child(SharedString::from(name.to_string()))
        .child(div().text_color(hsla(theme.text_muted)).child(value))
}

fn audio_rows(menu: Menu, theme: &Theme, memory: &AudioMemory) -> Menu {
    menu.child(row(theme, "Files", memory.files.to_string()))
        .child(row(
            theme,
            "Decoded samples",
            count_and_bytes(memory.decoded_files, memory.sample_bytes),
        ))
        .child(row(
            theme,
            "Waveform peaks",
            format_bytes(memory.peak_bytes),
        ))
}

impl Workspace {
    pub(super) fn memory_popup(&self, context: &mut Context<Self>) -> gpui::Div {
        let theme = &*crate::theme::current(context);
        let sources = self.state(context).source_memory();
        let cache = AUDIO_ANALYSIS_CACHE.memory();
        // A cache per view: each lets go of the tiles it no longer draws
        // as it renders, and a view not rendered keeps showing its own.
        let waves = [
            self.arrangement.read(context).wave_memory(),
            self.browser.read(context).wave_memory(),
            self.bottom.read(context).wave_memory(),
            self.export.read(context).wave_memory(),
        ]
        .into_iter()
        .fold(WaveMemory::default(), WaveMemory::add);
        let resident = self
            .menu_bar
            .resident_bytes()
            .map_or_else(|| "unknown".to_string(), format_bytes);

        let menu = Menu::new(theme)
            .child(row(theme, "Process (resident)", resident))
            .separator()
            .label("Files clips use");
        let menu = audio_rows(menu, theme, &sources.files)
            .child(row(
                theme,
                "Engine copies",
                count_and_bytes(sources.engine_copies, sources.engine_bytes),
            ))
            .separator()
            .label("Analysis cache (browsed and used)");
        audio_rows(menu, theme, &cache)
            .separator()
            .label("Waveform tiles")
            .child(row(
                theme,
                "Column values",
                count_and_bytes(waves.tiles, waves.tile_bytes),
            ))
            .child(row(
                theme,
                "GPU images",
                count_and_bytes(waves.images, waves.image_bytes),
            ))
            .into_element()
    }
}
