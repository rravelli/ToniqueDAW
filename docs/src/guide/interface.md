# The interface

```text
┌──────────────────────────────────────────────────────────────┐
│ Menu bar: File  Edit  View  Settings                         │
├──────────────────────────────────────────────────────────────┤
│ Transport bar                                                │
├────────────┬──────────────┬──────────────────────────────────┤
│            │ Ruler + loop lane                               │
│  Browser   ├──────────────┬──────────────────────────────────┤
│  (Files /  │ Track        │                                  │
│  Effects)  │ headers      │ Timeline (clips)                 │
│            │              │                                  │
├────────────┴──────────────┴──────────────────────────────────┤
│ Effects panel (selected track)                               │
└──────────────────────────────────────────────────────────────┘
```

## Menu bar

- **File**: new, open, open recent, save and save as. See
  [Saving projects](projects.md).
- **Edit**: undo, redo, and add an audio track.
- **View**: switch between the timeline and the audio graph, show or hide
  the browser and the effects panel, and zoom the interface.
- **Settings**: opens the [settings window](settings.md).

## Transport bar

From left to right:

| Control             | What it does                                          |
| ------------------- | ----------------------------------------------------- |
| Sidebar             | Shows or hides the browser (**Ctrl+B**).              |
| Graph               | Switches between the timeline and the audio graph.    |
| Metronome           | Turns the click on or off (**Ctrl+M**). It blinks on the beat. |
| Play / Stop         | Starts or stops playback (**Space**).                 |
| Record              | Not available yet.                                    |
| Loop                | Turns looping on or off.                              |
| Follow playhead     | Scrolls the timeline to keep the playhead in view.    |
| BPM                 | The project tempo.                                    |

On the right side are the master output waveform, frames per second, CPU
usage, and the **Undo** and **Redo** buttons.

Hover over any button to see its name and its shortcut.

## Browser

The left panel has two tabs. **Files** lists your sample folders and
**Effects** lists the effects you can add. See
[Browsing and previewing audio](browser.md).

## Track headers and timeline

Each row is a track or a group. The header on the left has the track's
controls and the timeline on the right holds its clips. See
[Tracks and groups](tracks.md) and [Editing clips](clips.md).

Above the timeline, the **ruler** shows bars and beats:

- Click or drag the ruler to move the playhead.
- Scroll the mouse wheel over the ruler to zoom in and out of the timeline.
- The thin lane at the top of the ruler holds the loop region. See
  [Playback, loop and tempo](playback.md).

The buttons to the right of the ruler expand or collapse all tracks.

## Effects panel

The bottom panel shows the effects of the selected track. Show or hide it
with **Ctrl+J**. See [Effects](effects.md).

## Audio graph

**Ctrl+Shift+G** replaces the timeline with a view of the audio graph. This
diagram shows how tracks, groups and effects are connected to the master
output. It's useful for understanding the routing, but you don't need it for
normal work.

## Zooming the interface

**Ctrl++** and **Ctrl+-** make the whole interface larger or smaller, and
**Ctrl+0** resets it. Tonique remembers the zoom level.
