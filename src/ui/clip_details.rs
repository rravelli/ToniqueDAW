//! The bottom panel's view of one clip.

use egui::{CursorIcon, Grid, Rect, RichText, Sense, Stroke, Ui, pos2};
use tonique_engine::{edit::ClipId, time::BeatPos};

use crate::{
    core::{clip::AudioClip, grid::GridService, state::ProjectState, track::TrackRow},
    ui::{
        arrangement::grid::PaintGrid,
        theme::{Theme, ThemeExt, with_alpha},
        waveform::paint_waveform,
    },
};

const DETAILS_WIDTH: f32 = 260.;
const LABEL_WIDTH: f32 = 60.;
const PADDING: f32 = 8.;
const RULER_HEIGHT: f32 = 24.;
/// Room left around the clip when fitting it, as a share of its length.
const FIT_MARGIN: f64 = 0.02;
/// Half the width of the area grabbing a trim edge.
const HANDLE_REACH: f32 = 4.;
/// Shortest a clip can be trimmed to, in beats.
const MIN_LENGTH: f64 = 1. / 16.;

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
enum TrimEdge {
    Start,
    End,
}

/// Bottom panel view of one clip: its whole source file (the untrimmed
/// clip) on a grid counted from the file's start, zoomable and scrollable,
/// with the part the clip plays between two edges that can be dragged to
/// trim it; and the clip's position, length and format.
pub struct ClipDetails {
    /// The view's zoom and scroll, in beats from the file's start.
    grid: GridService,
    /// The clip the view was fitted to: another one refits it.
    fitted: Option<ClipId>,
}

impl ClipDetails {
    pub fn new() -> Self {
        Self {
            // Scrolls a little before the file's start when fitting.
            grid: GridService::new().with_negative_offset(),
            fitted: None,
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        clip: &AudioClip,
        track: &TrackRow,
        state: &mut ProjectState,
    ) {
        let rect = ui.available_rect_before_wrap().shrink(PADDING);
        let details_width = DETAILS_WIDTH.min(rect.width() / 2.);
        let view = Rect::from_min_max(
            rect.min,
            pos2(rect.right() - details_width - PADDING, rect.bottom()),
        );
        let ruler = Rect::from_min_max(view.min, pos2(view.right(), view.top() + RULER_HEIGHT));
        let lane = Rect::from_min_max(pos2(view.left(), ruler.bottom()), view.max);
        let details = Rect::from_min_max(pos2(view.right() + PADDING, rect.top()), rect.max);

        let bpm = state.bpm();
        let file_len = clip.source_beats(bpm);
        if self.fitted != Some(clip.id) {
            self.fit(played(clip, bpm), lane.width());
            self.fitted = Some(clip.id);
        }
        if let Some(beats) = self.navigate(ui, ruler, lane, file_len, played(clip, bpm)) {
            state.seek(file_start(clip, bpm) + beats);
        }
        self.trim_handles(ui, lane, clip, state);
        // Show the trim being dragged, here or in the timeline.
        let shown = preview(clip, state);

        // The grid and ruler cover the untrimmed clip: the whole file.
        let theme = ui.app_theme();
        let file_x = self.grid.beats_to_x(BeatPos::ZERO, lane)
            ..=self.grid.beats_to_x(BeatPos(file_len), lane);
        ui.painter_at(ruler).rect_filled(ruler, 0., theme.bg_raised);
        let labels = ruler.intersect(Rect::from_x_y_ranges(file_x, ruler.y_range()));
        self.grid
            .paint_labels(&ui.painter_at(labels), ruler, bpm, &theme);
        self.paint_lane(ui, lane, file_len, &shown, track, state, &theme);

        ui.scope_builder(egui::UiBuilder::new().max_rect(details), |ui| {
            details_grid(ui, &shown, state);
        });
        ui.allocate_rect(rect, Sense::hover());
    }

    fn fit(&mut self, (start, end): (f64, f64), width: f32) {
        let margin = (end - start) * FIT_MARGIN;
        self.grid
            .fit(BeatPos(start - margin), BeatPos(end + margin), width);
    }

    /// Ctrl+wheel or pinch zooms, like the timeline, and so does the wheel
    /// over the ruler. The wheel or a drag over the lane scrolls.
    /// Double-clicking the lane returns where to seek to (in beats from the
    /// file's start); double-clicking the ruler fits the clip again.
    fn navigate(
        &mut self,
        ui: &Ui,
        ruler: Rect,
        lane: Rect,
        file_len: f64,
        played: (f64, f64),
    ) -> Option<f64> {
        let response = ui
            .interact(lane, ui.id().with("clip-lane"), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::Grab);
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let over_ruler = pointer.is_some_and(|p| ruler.contains(p));
        let over_lane = pointer.is_some_and(|p| lane.contains(p));

        let zoom = ui.input(|i| i.zoom_delta());
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if let Some(pointer) = pointer.filter(|_| over_ruler || over_lane) {
            if zoom != 1. {
                self.grid.zoom_by(zoom, pointer.x, lane);
            } else if over_ruler && scroll.y != 0. {
                self.grid.zoom_around(scroll.y, pointer.x, lane);
            } else if over_lane {
                // Only horizontal scrolling here: use the vertical wheel too.
                self.grid.offset.x -= if scroll.x != 0. { scroll.x } else { scroll.y };
            }
        }
        if response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            self.grid.offset.x -= response.drag_delta().x;
        }
        let ruler_response = ui.interact(ruler, ui.id().with("clip-ruler"), Sense::click());
        if ruler_response.double_clicked() {
            self.fit(played, lane.width());
        }
        let seek = response
            .double_clicked()
            .then(|| response.interact_pointer_pos())
            .flatten()
            .map(|p| self.grid.x_to_beats(p.x, lane).0);

        // Keep part of the file in view.
        let min = -lane.width() / 2.;
        let max = (self.grid.beats_to_width(file_len) - lane.width() / 2.).max(min);
        self.grid.offset.x = self.grid.offset.x.clamp(min, max);
        seek
    }

    /// Drag the edges of the played part to trim the clip. Snaps to the grid
    /// unless Alt is held, like the timeline; commits on release.
    fn trim_handles(&self, ui: &Ui, lane: Rect, clip: &AudioClip, state: &mut ProjectState) {
        let bpm = state.bpm();
        let (start, end) = played(&preview(clip, state), bpm);
        for (edge, beats) in [(TrimEdge::Start, start), (TrimEdge::End, end)] {
            let x = self.grid.beats_to_x(BeatPos(beats), lane);
            let area = Rect::from_x_y_ranges(x - HANDLE_REACH..=x + HANDLE_REACH, lane.y_range());
            let response = ui
                .interact(
                    area.intersect(lane),
                    ui.id().with(("trim", edge)),
                    Sense::drag(),
                )
                .on_hover_and_drag_cursor(CursorIcon::ResizeHorizontal);
            if response.hovered() || response.dragged() {
                let theme = ui.app_theme();
                ui.painter_at(lane)
                    .rect_filled(area, 1., theme.hover_overlay);
            }
            if response.dragged()
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            {
                let beats = self.grid.x_to_beats(pointer.x, lane);
                let beats = if ui.input(|i| i.modifiers.alt) {
                    beats
                } else {
                    self.grid.snap_at_grid(beats)
                };
                let t = trimmed(clip, edge, beats.0, bpm);
                state.resize_clip(&clip.id, t.trim_start, t.trim_end, t.position);
            }
            if response.drag_stopped()
                && let Some((id, start, end, position)) = state.resized_clip
                && id == clip.id
            {
                state.commit_resize_clip(&id, start, end, position);
            }
        }
    }

    /// The file's waveform dimmed outside the part the clip plays, the grid,
    /// the trim edges and the playhead.
    #[allow(clippy::too_many_arguments)]
    fn paint_lane(
        &self,
        ui: &Ui,
        lane: Rect,
        file_len: f64,
        clip: &AudioClip,
        track: &TrackRow,
        state: &ProjectState,
        theme: &Theme,
    ) {
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 0., theme.bg_deep);
        let x = |beats: f64| self.grid.beats_to_x(BeatPos(beats), lane);
        let file = Rect::from_x_y_ranges(x(0.)..=x(file_len), lane.y_range());
        self.grid
            .paint_grid(&ui.painter_at(lane.intersect(file)), lane, theme);

        if let Some(frames) = clip.audio.total_frames() {
            paint_waveform(
                &painter,
                file,
                file.intersect(lane),
                &clip.audio.data,
                0.0..frames,
                clip.audio.channels >= 2,
                track.color,
            );
        }

        let bpm = state.bpm();
        let (start, end) = played(clip, bpm);
        let (start, end) = (x(start), x(end));
        let dim = with_alpha(theme.bg_deep, 190);
        for part in [file.left()..=start, end..=file.right()] {
            painter.rect_filled(Rect::from_x_y_ranges(part, lane.y_range()), 0., dim);
        }
        for edge in [start, end] {
            painter.vline(edge, lane.y_range(), Stroke::new(2., track.color));
        }
        // The playhead, where the file is at that moment.
        painter.vline(
            x(state.playhead() - file_start(clip, bpm)),
            lane.y_range(),
            Stroke::new(1., theme.playhead),
        );
    }
}

/// Where the file's start lies in the arrangement: before the clip's
/// position by what its start trims away.
fn file_start(clip: &AudioClip, bpm: f32) -> BeatPos {
    clip.position - clip.trim_start as f64 * clip.source_beats(bpm)
}

/// The part of the file the clip plays, in beats from the file's start.
fn played(clip: &AudioClip, bpm: f32) -> (f64, f64) {
    let length = clip.source_beats(bpm);
    (
        clip.trim_start as f64 * length,
        clip.trim_end as f64 * length,
    )
}

/// `clip` with the trim being dragged, if it's this one.
fn preview(clip: &AudioClip, state: &ProjectState) -> AudioClip {
    let mut shown = clip.clone();
    if let Some((id, start, end, position)) = state.resized_clip
        && id == clip.id
    {
        shown.trim_start = start;
        shown.trim_end = end;
        shown.position = position;
    }
    shown
}

/// `clip` with `edge` moved to `beats` from the file's start. The audio
/// stays where it is in the arrangement, so trimming the start moves the
/// clip's position; the clip keeps at least [`MIN_LENGTH`] and never starts
/// before the arrangement.
fn trimmed(clip: &AudioClip, edge: TrimEdge, beats: f64, bpm: f32) -> AudioClip {
    let mut clip = clip.clone();
    let length = clip.source_beats(bpm);
    if length <= 0. {
        return clip;
    }
    let ratio = beats / length;
    let min_ratio = MIN_LENGTH / length;
    let (trim_start, trim_end) = (clip.trim_start as f64, clip.trim_end as f64);
    match edge {
        TrimEdge::Start => {
            let first = (trim_start - clip.position.0 / length).max(0.);
            let last = (trim_end - min_ratio).max(first);
            let start = ratio.clamp(first, last);
            clip.position += (start - trim_start) * length;
            clip.trim_start = start as f32;
        }
        TrimEdge::End => {
            let first = (trim_start + min_ratio).min(1.);
            clip.trim_end = ratio.clamp(first, 1.) as f32;
        }
    }
    clip
}

fn details_grid(ui: &mut Ui, clip: &AudioClip, state: &ProjectState) {
    let theme = ui.app_theme();
    let bpm = state.bpm();
    let beats_per_bar = state.grid.beats_per_bar();
    let source = clip.source_seconds();
    let length_beats = clip.end(bpm) - clip.position;
    let position = clip.position.0;
    let audio = &clip.audio;

    ui.label(RichText::new(&audio.name).strong())
        .on_hover_text(audio.path.display().to_string());
    ui.add_space(4.);
    Grid::new(("clip-details", clip.id))
        .num_columns(2)
        .min_col_width(LABEL_WIDTH)
        .spacing([8., 3.])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.label(RichText::new(label).color(theme.text_muted));
                ui.label(RichText::new(value).color(theme.text));
                ui.end_row();
            };
            row(
                "Position",
                format!(
                    "{}  ·  {}",
                    format_bars(position, beats_per_bar),
                    format_time(position * 60. / bpm as f64)
                ),
            );
            row(
                "Length",
                format!(
                    "{}  ·  {}",
                    format_length(length_beats, beats_per_bar),
                    format_time(length_beats * 60. / bpm as f64)
                ),
            );
            row(
                "Plays",
                format!(
                    "{} – {} of {}",
                    format_time((clip.trim_start * source) as f64),
                    format_time((clip.trim_end * source) as f64),
                    format_time(source as f64)
                ),
            );
            row(
                "Format",
                format_audio(audio.sample_rate, audio.bit_depth, audio.channels),
            );
        });
}

/// Position as `bar.beat.sixteenth`, counting from 1: beat 0 is `1.1.1`.
fn format_bars(beats: f64, beats_per_bar: usize) -> String {
    let sixteenths = (beats * 4.).floor().max(0.) as usize;
    let per_bar = beats_per_bar.max(1) * 4;
    format!(
        "{}.{}.{}",
        sixteenths / per_bar + 1,
        sixteenths % per_bar / 4 + 1,
        sixteenths % 4 + 1
    )
}

/// Length as bars, beats and sixteenths: `1 bar 2 beats`, `3 beats`.
fn format_length(beats: f64, beats_per_bar: usize) -> String {
    let sixteenths = (beats * 4.).round().max(0.) as usize;
    let per_bar = beats_per_bar.max(1) * 4;
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let parts: Vec<String> = [
        (sixteenths / per_bar, "bar"),
        (sixteenths % per_bar / 4, "beat"),
        (sixteenths % 4, "sixteenth"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, word)| plural(n, word))
    .collect();
    if parts.is_empty() {
        "0 beats".into()
    } else {
        parts.join(" ")
    }
}

/// `m:ss.mmm`.
fn format_time(seconds: f64) -> String {
    let millis = (seconds.max(0.) * 1000.).round() as u64;
    format!(
        "{}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

/// `48 kHz · 24-bit · stereo`.
fn format_audio(sample_rate: u32, bit_depth: Option<u32>, channels: u16) -> String {
    let rate = sample_rate as f32 / 1000.;
    let rate = if rate.fract() == 0. {
        format!("{rate:.0} kHz")
    } else {
        format!("{rate:.1} kHz")
    };
    let channels = match channels {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        n => format!("{n} channels"),
    };
    match bit_depth {
        Some(bits) => format!("{rate} · {bits}-bit · {channels}"),
        None => format!("{rate} · {channels}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clip at beat 10 playing the middle half of an 8-beat file (at 120
    /// bpm): beats 2 to 6 of the file.
    fn clip() -> AudioClip {
        use crate::analysis::AudioInfo;
        let audio = AudioInfo {
            name: "a.wav".into(),
            duration: Some(std::time::Duration::from_secs(4)),
            data: Default::default(),
            sample_rate: 48000,
            channels: 2,
            bit_depth: None,
            num_samples: None,
            path: Default::default(),
        };
        let mut clip = AudioClip::new(ClipId(1), audio, BeatPos(10.));
        clip.trim_start = 0.25;
        clip.trim_end = 0.75;
        clip
    }

    #[test]
    fn the_view_counts_from_the_file_start() {
        assert_eq!(clip().source_beats(120.), 8.);
        assert_eq!(played(&clip(), 120.), (2., 6.));
        // Beat 3 of the file plays at beat 11 of the arrangement.
        assert_eq!(file_start(&clip(), 120.) + 3., BeatPos(11.));
    }

    #[test]
    fn trimming_the_start_keeps_the_audio_in_place() {
        let t = trimmed(&clip(), TrimEdge::Start, 1., 120.);
        assert_eq!((t.trim_start, t.position), (0.125, BeatPos(9.)));
        assert_eq!(t.trim_end, 0.75);
        // No further than the file's start...
        let t = trimmed(&clip(), TrimEdge::Start, -3., 120.);
        assert_eq!((t.trim_start, t.position), (0., BeatPos(8.)));
        // ...nor past the end, minus the minimum length.
        let t = trimmed(&clip(), TrimEdge::Start, 7., 120.);
        assert_eq!(t.trim_start, (0.75 - MIN_LENGTH / 8.) as f32);
    }

    #[test]
    fn trimming_the_start_stops_at_the_arrangement_start() {
        let mut near_zero = clip();
        near_zero.position = BeatPos(1.); // the file would start at beat -1
        let t = trimmed(&near_zero, TrimEdge::Start, 0., 120.);
        assert_eq!((t.trim_start, t.position), (0.125, BeatPos::ZERO));
    }

    #[test]
    fn trimming_the_end() {
        let t = trimmed(&clip(), TrimEdge::End, 7., 120.);
        assert_eq!(
            (t.trim_end, t.position, t.trim_start),
            (0.875, BeatPos(10.), 0.25)
        );
        assert_eq!(trimmed(&clip(), TrimEdge::End, 20., 120.).trim_end, 1.);
        assert_eq!(
            trimmed(&clip(), TrimEdge::End, 0., 120.).trim_end,
            (0.25 + MIN_LENGTH / 8.) as f32
        );
    }

    #[test]
    fn positions_count_bars_beats_and_sixteenths_from_one() {
        assert_eq!(format_bars(0., 4), "1.1.1");
        assert_eq!(format_bars(9.5, 4), "3.2.3");
        assert_eq!(format_bars(3.99, 4), "1.4.4");
        assert_eq!(format_bars(3., 3), "2.1.1");
    }

    #[test]
    fn lengths_read_naturally() {
        assert_eq!(format_length(4., 4), "1 bar");
        assert_eq!(format_length(6.25, 4), "1 bar 2 beats 1 sixteenth");
        assert_eq!(format_length(3., 4), "3 beats");
        assert_eq!(format_length(0., 4), "0 beats");
    }

    #[test]
    fn times_and_formats() {
        assert_eq!(format_time(0.), "0:00.000");
        assert_eq!(format_time(83.25), "1:23.250");
        assert_eq!(
            format_audio(44100, Some(16), 2),
            "44.1 kHz · 16-bit · stereo"
        );
        assert_eq!(format_audio(48000, None, 1), "48 kHz · mono");
    }
}
