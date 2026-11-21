//! Audio dropped on the timeline, from the browser or the system: where
//! it would land while dragged over, and the clips it adds. An effect
//! dropped on a lane or a header goes on its track, last; the track it
//! would go on is lit while it's dragged over.

use std::path::PathBuf;

use gpui::{Context, DragMoveEvent, ExternalPaths, Pixels, Point, Stateful, prelude::*};
use tonique_engine::{edit::TrackId, time::BeatPos};

use crate::{
    analysis::AudioInfo,
    app::Tonique,
    bottom::{BottomView, DraggedEffect},
    browser::DraggedAudio,
    cache::AUDIO_ANALYSIS_CACHE,
    core::{clip::AudioClip, state::MASTER_TRACK_ID},
    timeline::{RULER_HEIGHT, rows::rows},
};

/// Where audio dragged over the timeline would land.
#[derive(Clone)]
pub struct DropTarget {
    pub audio: AudioInfo,
    /// The index of the track it lands on; `None`: on a new track below
    /// the last.
    pub track_index: Option<usize>,
    pub position: BeatPos,
}

impl Tonique {
    fn drop_target_at(
        &self,
        position: Point<Pixels>,
        audio: AudioInfo,
        alt: bool,
    ) -> Option<DropTarget> {
        if !self.timeline.bounds.get().contains(&position) {
            return None;
        }
        audio.duration?;
        audio.load_peaks();
        let (local_x, local_y) = self.local(position);
        if self.in_footer(local_y) {
            return None;
        }
        let rows = rows(&self.state);
        Some(DropTarget {
            audio,
            track_index: self.track_at(&rows, local_y.max(RULER_HEIGHT)),
            position: self.snapped(local_x, alt),
        })
    }

    fn follow_drag(&mut self, path: Option<PathBuf>, position: Point<Pixels>, alt: bool) {
        let over_lanes = self.timeline.bounds.get().contains(&position);
        let (_, local_y) = self.local(position);
        self.expand_on_hover(over_lanes.then_some(local_y), true);
        let audio = path.and_then(|path| AUDIO_ANALYSIS_CACHE.get_or_analyze(path));
        self.timeline.drop_target =
            audio.and_then(|audio| self.drop_target_at(position, audio, alt));
    }

    /// Places `paths` from `target` on: the first on its track (or a new
    /// one), the others on the tracks below, adding tracks as needed.
    fn drop_audio(&mut self, paths: &[PathBuf], target: Option<DropTarget>) {
        let Some(target) = target else {
            self.expand_on_hover(None, false);
            return;
        };
        let infos: Vec<AudioInfo> = paths
            .iter()
            .filter_map(|path| AUDIO_ANALYSIS_CACHE.get_or_analyze(path.clone()))
            .filter(|audio| audio.duration.is_some())
            .collect();
        if infos.is_empty() {
            return;
        }
        let tracks: Vec<TrackId> = self.state.tracks().map(|track| track.id).collect();
        self.state.begin_batch();
        for (index, audio) in infos.into_iter().enumerate() {
            let track = match target
                .track_index
                .map(|first| first + index)
                .and_then(|track_index| tracks.get(track_index))
            {
                Some(track) => *track,
                None => self.state.add_track(),
            };
            let clip = AudioClip::new(
                self.state.new_clip_id(),
                audio,
                target.position,
                self.state.bpm(),
            );
            self.state.add_clips(&track, vec![clip]);
        }
        self.state.commit_batch();
        self.timeline.drop_target = None;
        // The groups it was dragged into stay open, to show where it landed.
        self.timeline.hover_expanded.clear();
    }

    /// The track an effect dropped at `position` goes on: the one whose
    /// lane or header is there, the master in the footer.
    fn effect_target_at(&self, position: Point<Pixels>) -> Option<TrackId> {
        let over_lanes = self.timeline.bounds.get().contains(&position);
        if !over_lanes && !self.headers_bounds().contains(&position) {
            return None;
        }
        let (_, local_y) = self.local(position);
        if local_y < RULER_HEIGHT {
            return None;
        }
        if self.in_footer(local_y) {
            return Some(MASTER_TRACK_ID);
        }
        let rows = rows(&self.state);
        self.row_at(&rows, local_y).map(|row| rows[row].track.id)
    }

    /// Lights the track an effect dragged to `position` would go on.
    pub fn follow_effect_drag(&mut self, position: Point<Pixels>) {
        self.timeline.effect_target = self.effect_target_at(position);
    }

    /// Puts `dragged` last on the track at `position`, selects it and shows
    /// its effects.
    pub fn drop_effect(&mut self, dragged: &DraggedEffect, position: Point<Pixels>) {
        self.timeline.effect_target = None;
        let Some(track) = self.effect_target_at(position) else {
            return;
        };
        self.state.add_effect(&track, dragged.kind, usize::MAX);
        self.state.select_track(&track);
        self.bottom.open = true;
        self.bottom.view = BottomView::Effects;
    }

    /// `view` taking files dragged from the browser and the system, and
    /// effects.
    pub(super) fn take_drops(
        &self,
        view: Stateful<gpui::Div>,
        context: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        view.on_drag_move(context.listener(
            |this, event: &DragMoveEvent<DraggedAudio>, _, context| {
                let path = event.drag(context).path.clone();
                this.follow_drag(Some(path), event.event.position, event.event.modifiers.alt);
                context.notify();
            },
        ))
        .on_drag_move(
            context.listener(|this, event: &DragMoveEvent<ExternalPaths>, _, context| {
                let path = event.drag(context).paths().first().cloned();
                this.follow_drag(path, event.event.position, event.event.modifiers.alt);
                context.notify();
            }),
        )
        .on_drop(
            context.listener(|this, dragged: &DraggedAudio, _, context| {
                let target = this.timeline.drop_target.take();
                this.drop_audio(std::slice::from_ref(&dragged.path), target);
                context.notify();
            }),
        )
        .on_drag_move(
            context.listener(|this, event: &DragMoveEvent<DraggedEffect>, _, context| {
                this.follow_effect_drag(event.event.position);
                context.notify();
            }),
        )
        .on_drop(
            context.listener(|this, dragged: &DraggedEffect, window, context| {
                this.drop_effect(dragged, window.mouse_position());
                context.notify();
            }),
        )
        .on_drop(
            context.listener(|this, paths: &ExternalPaths, window, context| {
                // Without a move during the drag (some systems), from where
                // the pointer is now.
                let target = this.timeline.drop_target.take().or_else(|| {
                    let audio =
                        AUDIO_ANALYSIS_CACHE.get_or_analyze(paths.paths().first()?.clone())?;
                    this.drop_target_at(window.mouse_position(), audio, false)
                });
                this.drop_audio(paths.paths(), target);
                context.notify();
            }),
        )
    }
}
