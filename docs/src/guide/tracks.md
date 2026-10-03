# Tracks and groups

A **track** holds audio clips. A **group** holds tracks and other groups.
Groups mix the tracks inside them together, so you can control several
tracks with a single volume, mute and solo, or put an effect on all of them.
At the top of the list, the **master** track controls the final output.

## Adding tracks

You can add a track in any of these ways:

- Press **Ctrl+T**.
- Choose **Edit → Add audio track**.
- Right-click a track header or the empty space below the tracks.
- Drop an audio file below the last track. This creates a new track that
  holds the file.

## Reordering tracks

Drag a track header up or down to move the track. A line shows where it
will go. Dragging a track into a group puts it inside the group. When you
move a group, everything inside it moves too.

## Selecting tracks

- **Click** a track header to select that track.
- **Ctrl+click** adds a track to the selection or removes it.
- **Shift+click** selects every track from the last selected one to the one
  you click.

## Track controls

| Control       | What it does                                                    |
| ------------- | --------------------------------------------------------------- |
| Arrow         | Collapses or expands the track. Collapsed tracks show only their clip headers. |
| Name          | To rename the track, right-click its header and choose **Rename**. |
| **M**         | Mutes the track.                                                |
| **S**         | Solos the track, so only soloed tracks are heard. **Shift+click** adds it to the tracks already soloed. |
| Volume bar    | Drag it sideways to set the level, from −40 dB to +5 dB. Double-click it to reset it to 0 dB. |
| Meter         | Shows the track's live output level.                            |
| Bottom edge   | Drag it to make the track taller or shorter.                    |

Muted tracks, and tracks silenced by another track's solo, are greyed out.

## The track menu

Right-click a track header to:

- **Rename** it.
- **Add Audio Track** below it.
- **Group** the selected tracks, or **Ungroup** a group.
- **Duplicate** the track and its clips.
- Change its **Color**.
- **Delete** it. For a group, this option is called
  **Delete group and contents**.

With a track selected and no clips selected, **Ctrl+D** duplicates the
track and **Delete** removes it.

## Groups

To create a group, select the tracks you want in it and press **Ctrl+G**,
or right-click and choose **Group**. Groups can contain other groups.

- Inside a group, tracks are indented and marked with the group's color.
- A collapsed group shows a faint overview of its clips. When you drag
  audio or clips over a collapsed group, it opens, and it closes again if
  you move away without dropping.
- The group's volume, mute, solo and effects apply to everything inside it.
- If you remove every track from a group, the group is removed too.

## Collapsing everything

The two buttons to the right of the ruler **expand all** or **collapse all**
tracks and groups.
