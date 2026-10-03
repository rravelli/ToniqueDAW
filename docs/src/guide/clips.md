# Editing clips

A **clip** is a piece of an audio file placed on a track. You can edit clips
freely: Tonique never changes your audio files.

## Adding clips

Drag an audio file from the [browser](browser.md) onto the timeline. While
you drag, a preview shows where the clip will go.

- Drop it on a track to add it there.
- Drop it below the last track to create a new track.

Clips on the same track never overlap. A clip you drop or move on top of
another one trims away the part it covers.

## Selecting clips

- **Click** a clip to select it.
- **Shift+click** adds a clip to the selection or removes it.
- **Drag on an empty part** of the timeline to draw a selection zone. It
  selects the parts of the clips inside it, across several tracks.
- **Ctrl+A** selects every clip.
- **Click an empty part** of the timeline to clear the selection. This also
  places the edit cursor there.

## Moving and copying

- **Drag a clip by its header** to move it in time or to another track. If
  several clips are selected, they all move together.
- **Hold Ctrl while you start dragging** to drag a copy and leave the
  original in place.
- **←** and **→** nudge the selected clips by one grid step. With nothing
  selected, they move the edit cursor instead.
- **↑** and **↓** move the selected clips to the track above or below.
- **Ctrl+D** duplicates the selected clips.

If you made a selection zone, only the part of the clips inside it moves or
is copied.

## Trimming

Drag the **left or right edge** of a clip to shorten or lengthen it. The
waveform stays where it is, so you choose which part of the file plays.

## Splitting

Place the edit cursor where you want to cut, select the track or tracks, and
press **Ctrl+K**. Every clip under the cursor on those tracks is split in
two.

## Copy, cut and paste

**Ctrl+C**, **Ctrl+X** and **Ctrl+V** copy, cut and paste the selected
clips. Pasted clips start at the edit cursor, on the selected track (and
the tracks below it, if you copied from several tracks). Tracks are added
if needed. The edit cursor then moves past the pasted clips, so pasting
again adds the clips after them.

## Deleting

Press **Delete** or **Backspace**, or right-click a clip and choose
**Delete**.

## Snapping

Moving, trimming, dropping, drawing a selection zone and placing the cursor
all snap to the grid. Trimming also snaps to the edges of other clips, the
loop region and the edit cursor.

**Hold Alt** while dragging to turn snapping off.

The grid follows the zoom: zoom in to get finer grid steps and zoom out to
get coarser ones. To zoom the timeline, use **Ctrl+mouse wheel** (or pinch
on a trackpad) over the timeline, or the mouse wheel over the ruler.

## Undo

Every edit can be undone with **Ctrl+Z** and redone with **Ctrl+Y** or
**Ctrl+Shift+Z**. This includes changes to clips, tracks, groups, volume,
mute, solo and effects.
