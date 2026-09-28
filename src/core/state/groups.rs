//! Track groups: buses shown in the track list, holding tracks and other
//! groups at any depth. A group is addressed like a track, by
//! `TrackId(bus.0)` (engine IDs share one counter), so views, selection,
//! effects and meters treat it as one.
//!
//! The engine's track list is kept in tree order: each group's tracks are
//! contiguous, and its row sits right above its first track. Groups are
//! never empty (an emptied group is removed), so where a group sits always
//! follows from its tracks.

use std::collections::HashMap;

use tonique_engine::edit::{
    Bus, BusId, Output, TrackId,
    commands::{AddBus, MoveTrack, RemoveBus, RemoveTrack, SetBusOutput, SetOutput},
};

use super::{MASTER_TRACK_ID, ProjectState};
use crate::core::track::{TrackKind, TrackRow, TrackSoloState, TrackView};

/// Where a dragged row lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowTarget {
    /// Just above this row, in the same group.
    Before(TrackId),
    /// First inside this group.
    Into(TrackId),
    /// Last, outside any group.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Node {
    Track(TrackId),
    Group(BusId),
}

impl ProjectState {
    pub fn is_group(&self, id: TrackId) -> bool {
        id != MASTER_TRACK_ID && self.edit().bus(BusId(id.0)).is_ok()
    }

    fn exists(&self, id: TrackId) -> bool {
        self.edit().track(id).is_ok() || self.is_group(id)
    }

    fn output_of(&self, id: TrackId) -> Option<Output> {
        match self.edit().track(id) {
            Ok(track) => Some(track.output),
            Err(_) => self.edit().bus(BusId(id.0)).ok().map(|b| b.output),
        }
    }

    /// The group directly holding `id` (a track or a group).
    pub fn parent(&self, id: TrackId) -> Option<TrackId> {
        match self.output_of(id)? {
            Output::Bus(bus) => Some(TrackId(bus.0)),
            Output::Master => None,
        }
    }

    /// The groups holding `id`, nearest first.
    pub fn ancestors(&self, id: TrackId) -> Vec<TrackId> {
        self.output_of(id)
            .map(|output| {
                self.edit()
                    .buses_along(output)
                    .into_iter()
                    .map(|b| TrackId(b.0))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The tracks inside group `id` however deep, in order, as rows.
    pub fn group_tracks(&self, id: TrackId) -> Vec<TrackRow> {
        self.tracks_in(id)
            .into_iter()
            .filter_map(|t| self.track_index(t).and_then(|i| self.track_from_index(i)))
            .collect()
    }

    /// Whether `id` is inside a collapsed group.
    pub fn is_hidden(&self, id: TrackId) -> bool {
        self.ancestors(id)
            .iter()
            .any(|g| self.views.get(g).is_some_and(|v| v.collapsed))
    }

    /// The tracks inside `id` however deep, in order; a track is its own.
    pub fn tracks_in(&self, id: TrackId) -> Vec<TrackId> {
        if !self.is_group(id) {
            return self
                .edit()
                .track(id)
                .map(|t| vec![t.id])
                .unwrap_or_default();
        }
        let bus = BusId(id.0);
        let edit = self.edit();
        edit.tracks
            .iter()
            .filter(|t| edit.buses_along(t.output).contains(&bus))
            .map(|t| t.id)
            .collect()
    }

    /// The groups inside `id`, however deep.
    fn groups_in(&self, id: TrackId) -> Vec<TrackId> {
        let bus = BusId(id.0);
        let edit = self.edit();
        edit.buses
            .iter()
            .filter(|b| edit.buses_along(b.output).contains(&bus))
            .map(|b| TrackId(b.id.0))
            .collect()
    }

    /// The tracks and groups directly inside `id`.
    fn children(&self, id: TrackId) -> Vec<TrackId> {
        let target = Output::Bus(BusId(id.0));
        let edit = self.edit();
        edit.tracks
            .iter()
            .filter(|t| t.output == target)
            .map(|t| t.id)
            .chain(
                edit.buses
                    .iter()
                    .filter(|b| b.output == target)
                    .map(|b| TrackId(b.id.0)),
            )
            .collect()
    }

    /// Every row in display order with its depth, including those inside
    /// collapsed groups.
    fn layout(&self) -> Vec<(Node, usize)> {
        let edit = self.edit();
        let mut rows = Vec::new();
        let mut enclosing: Vec<BusId> = Vec::new();
        for track in &edit.tracks {
            let mut path = edit.buses_along(track.output);
            path.reverse(); // outermost first
            let shared = enclosing
                .iter()
                .zip(&path)
                .take_while(|(a, b)| a == b)
                .count();
            enclosing.truncate(shared);
            for bus in &path[shared..] {
                rows.push((Node::Group(*bus), enclosing.len()));
                enclosing.push(*bus);
            }
            rows.push((Node::Track(track.id), enclosing.len()));
        }
        rows
    }

    /// The rows of the track list in display order: tracks and groups,
    /// without what's inside collapsed groups.
    pub fn rows(&self) -> Vec<TrackRow> {
        let mut rows = Vec::new();
        // Depth of the collapsed group hiding the rows below it.
        let mut hiding: Option<usize> = None;
        for (node, depth) in self.layout() {
            if let Some(collapsed_depth) = hiding {
                if depth > collapsed_depth {
                    continue;
                }
                hiding = None;
            }
            let row = match node {
                Node::Track(id) => self.track_index(id).and_then(|i| self.track_from_index(i)),
                Node::Group(bus) => self.group_ref(bus, depth),
            };
            if let Some(row) = row {
                if row.kind == TrackKind::Group && row.collapsed {
                    hiding = Some(depth);
                }
                rows.push(row);
            }
        }
        rows
    }

    /// Every group, collapsed ones and their subgroups included.
    pub fn groups(&self) -> Vec<TrackRow> {
        self.edit()
            .buses
            .iter()
            .filter_map(|b| self.group_row(TrackId(b.id.0)))
            .collect()
    }

    /// The row of group `id`.
    pub(super) fn group_row(&self, id: TrackId) -> Option<TrackRow> {
        self.group_ref(BusId(id.0), self.ancestors(id).len())
    }

    fn group_ref(&self, bus: BusId, depth: usize) -> Option<TrackRow> {
        let group = self.edit().bus(bus).ok()?;
        let id = TrackId(bus.0);
        let view = self.views.get(&id).cloned().unwrap_or_else(TrackView::new);
        let tracks = self.tracks_in(id);
        let bpm = self.bpm();
        // Everything inside, for the lane's overview.
        let clips = tracks
            .iter()
            .filter_map(|t| self.edit().track(*t).ok())
            .flat_map(|t| t.clips.iter())
            .filter_map(|c| self.clip_view(c, bpm))
            .collect();
        Some(TrackRow {
            id,
            clips,
            muted: group.channel.muted,
            volume: group.channel.volume.get(),
            arm: false,
            name: group.name.clone(),
            height: view.height,
            collapsed: view.collapsed,
            color: view.color,
            selected: self.selected_tracks.contains(&id),
            solo: if group.soloed {
                TrackSoloState::Solo
            } else {
                TrackSoloState::NotSoloing
            },
            index: tracks
                .first()
                .and_then(|t| self.track_index(*t))
                .unwrap_or(0),
            kind: TrackKind::Group,
            depth,
        })
    }

    /// Tree order of the tracks: each group's together, siblings in the
    /// order of their first track.
    fn canonical_order(&self) -> Vec<TrackId> {
        let edit = self.edit();
        let mut children: HashMap<Option<BusId>, Vec<(usize, Node)>> = HashMap::new();
        for (i, track) in edit.tracks.iter().enumerate() {
            let path = edit.buses_along(track.output); // nearest first
            children
                .entry(path.first().copied())
                .or_default()
                .push((i, Node::Track(track.id)));
            // A group is first met at its first track.
            for (k, bus) in path.iter().enumerate() {
                let siblings = children.entry(path.get(k + 1).copied()).or_default();
                if !siblings.iter().any(|(_, n)| *n == Node::Group(*bus)) {
                    siblings.push((i, Node::Group(*bus)));
                }
            }
        }
        fn visit(
            parent: Option<BusId>,
            children: &HashMap<Option<BusId>, Vec<(usize, Node)>>,
            order: &mut Vec<TrackId>,
        ) {
            let Some(nodes) = children.get(&parent) else {
                return;
            };
            let mut nodes = nodes.clone();
            nodes.sort_by_key(|(first, _)| *first);
            for (_, node) in nodes {
                match node {
                    Node::Track(id) => order.push(id),
                    Node::Group(bus) => visit(Some(bus), children, order),
                }
            }
        }
        let mut order = Vec::with_capacity(edit.tracks.len());
        visit(None, &children, &mut order);
        order
    }

    /// Reorder the tracks into tree order (part of the current step).
    pub(super) fn normalize(&mut self) {
        for (i, id) in self.canonical_order().into_iter().enumerate() {
            if self.edit().tracks.get(i).map(|t| t.id) != Some(id) {
                self.perform(MoveTrack::new(id, i));
            }
        }
    }

    /// Remove groups left with nothing inside, innermost first.
    pub(super) fn remove_empty_groups(&mut self) {
        loop {
            let edit = self.edit();
            let empty = edit
                .buses
                .iter()
                .find(|b| {
                    let target = Output::Bus(b.id);
                    !edit.tracks.iter().any(|t| t.output == target)
                        && !edit.buses.iter().any(|o| o.output == target)
                })
                .map(|b| b.id);
            let Some(bus) = empty else {
                break;
            };
            self.perform(RemoveBus::new(bus));
            self.selected_tracks.retain(|t| t.0 != bus.0);
        }
    }

    /// Put `id` directly inside `parent` (or at the top level).
    fn set_parent(&mut self, id: TrackId, parent: Option<TrackId>) {
        let output = parent.map_or(Output::Master, |p| Output::Bus(BusId(p.0)));
        if self.output_of(id) == Some(output) {
            return;
        }
        if self.is_group(id) {
            self.perform(SetBusOutput::new(BusId(id.0), output));
        } else {
            self.perform(SetOutput::new(id, output));
        }
    }

    /// The deepest group holding all of `ids`.
    fn common_parent(&self, ids: &[TrackId]) -> Option<TrackId> {
        let chains: Vec<Vec<TrackId>> = ids
            .iter()
            .map(|id| self.ancestors(*id).into_iter().rev().collect())
            .collect();
        let mut deepest = None;
        for (depth, group) in chains.first()?.iter().enumerate() {
            if !chains.iter().all(|c| c.get(depth) == Some(group)) {
                break;
            }
            deepest = Some(*group);
        }
        deepest
    }

    /// The group a track inserted at `index` joins: the one its neighbours
    /// share, so it never splits a group.
    pub(super) fn insertion_parent(&self, index: usize) -> Option<TrackId> {
        let tracks = &self.edit().tracks;
        let before = tracks.get(index.checked_sub(1)?)?.id;
        let after = tracks.get(index)?.id;
        self.common_parent(&[before, after])
    }

    /// Put `ids` (tracks and groups, from anywhere) in a new group, as one
    /// undo step. The group goes where their branches meet; items inside
    /// another listed group move with it. Returns the new group.
    pub fn group(&mut self, ids: &[TrackId]) -> Option<TrackId> {
        let mut listed: Vec<TrackId> = Vec::new();
        for id in ids {
            if *id != MASTER_TRACK_ID && self.exists(*id) && !listed.contains(id) {
                listed.push(*id);
            }
        }
        let items: Vec<TrackId> = listed
            .iter()
            .copied()
            .filter(|id| !self.ancestors(*id).iter().any(|a| listed.contains(a)))
            .collect();
        if items.is_empty() {
            return None;
        }
        let parent = self.common_parent(&items);
        let number = self.edit().buses.len() + 1;
        let bus = self
            .session
            .create(|e| Bus::new(e, format!("Group {number}")));
        let id = TrackId(bus.id.0);
        let mut view = TrackView::new();
        view.name.clone_from(&bus.name);
        view.color = self.next_track_color();
        self.views.insert(id, view);

        self.transaction("Group", |s| {
            s.perform(AddBus::new(bus));
            s.set_parent(id, parent);
            for item in &items {
                s.set_parent(*item, Some(id));
            }
            s.normalize();
            s.remove_empty_groups();
        });
        self.select_track(&id);
        Some(id)
    }

    /// Remove group `id`, keeping what's inside one level up.
    pub fn ungroup(&mut self, id: TrackId) {
        if !self.is_group(id) {
            return;
        }
        let parent = self.parent(id);
        let children = self.children(id);
        self.transaction("Ungroup", |s| {
            for child in children {
                s.set_parent(child, parent);
            }
            s.perform(RemoveBus::new(BusId(id.0)));
            s.normalize();
        });
        self.selected_tracks.retain(|t| *t != id);
    }

    /// Delete group `id` with everything inside, as one undo step.
    pub fn delete_group(&mut self, id: TrackId) {
        if !self.is_group(id) {
            return;
        }
        let tracks = self.tracks_in(id);
        let mut groups = self.groups_in(id);
        // Innermost first: a bus goes once nothing routes into it.
        groups.sort_by_key(|g| std::cmp::Reverse(self.ancestors(*g).len()));
        groups.push(id);
        self.transaction("Delete group", |s| {
            for track in &tracks {
                s.perform(RemoveTrack::new(*track));
            }
            for group in &groups {
                s.perform(RemoveBus::new(BusId(group.0)));
            }
            s.remove_empty_groups();
        });
        self.selected_tracks
            .retain(|t| !tracks.contains(t) && !groups.contains(t));
    }

    /// Delete a track (its group goes too if left empty) as one undo step.
    pub(super) fn remove_track(&mut self, id: TrackId) {
        self.transaction("Delete track", |s| {
            s.perform(RemoveTrack::new(id));
            s.remove_empty_groups();
        });
    }

    /// Move a track or a group (with everything inside) to `target`, as one
    /// undo step. Refused (returns false) into the group itself or one of
    /// its subgroups.
    pub fn move_row(&mut self, id: TrackId, target: RowTarget) -> bool {
        if id == MASTER_TRACK_ID || !self.exists(id) {
            return false;
        }
        let (parent, anchor_row) = match target {
            RowTarget::Before(row) => (self.parent(row), Some(row)),
            RowTarget::Into(group) if self.is_group(group) => (Some(group), Some(group)),
            RowTarget::Into(_) => return false,
            RowTarget::End => (None, None),
        };
        if let Some(row) = anchor_row
            && (row == id || self.ancestors(row).contains(&id))
        {
            return false;
        }
        let block = self.tracks_in(id);
        // The first track the block goes in front of.
        let anchor =
            anchor_row.and_then(|row| self.tracks_in(row).into_iter().find(|t| !block.contains(t)));
        self.transaction("Move track", |s| {
            s.set_parent(id, parent);
            for track in &block {
                let Some(from) = s.track_index(*track) else {
                    continue;
                };
                let to = match anchor.and_then(|a| s.track_index(a)) {
                    Some(at) if from < at => at - 1,
                    Some(at) => at,
                    None => s.track_count() - 1,
                };
                if from != to {
                    s.perform(MoveTrack::new(*track, to));
                }
            }
            s.normalize();
            s.remove_empty_groups();
        });
        true
    }
}
