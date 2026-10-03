# Playback, loop and tempo

## Playhead and edit cursor

The timeline has two positions:

- The **playhead** moves during playback.
- The **edit cursor** is where playback starts, where clips are split, and
  where pasted clips go.

To move them:

- **Click an empty part of the timeline** to place the edit cursor. If
  playback is stopped, the playhead moves there too. If it's running, it
  keeps playing.
- **Click or drag the ruler** to move both right away, even during
  playback.
- With no clips selected, **←** and **→** move the edit cursor one grid
  step.

**Space** starts and stops playback. When playback stops, the playhead goes
back to the edit cursor, so you can listen to the same passage several
times.

With **Follow playhead** on (**Ctrl+Shift+F**), the timeline scrolls to keep
the playhead in view.

## Loop region

The loop region is shown in the thin lane at the top of the ruler. It's
highlighted when looping is on and greyed out when it's off.

- **Draw a new region**: drag across an empty part of the lane. Looping
  turns on automatically.
- **Move it**: drag the middle of the region.
- **Resize it**: drag either edge.
- **Switch looping on or off**: click the region, click the **Loop** button
  in the transport bar, or press **Ctrl+L**.

**Loop the selection.** If clips are selected when you press **Ctrl+L**, the
loop region is set to cover them and looping turns on. This is a quick way
to loop a passage you're editing.

The loop edges snap to the grid.

## Tempo

The **BPM** field in the transport bar sets the project's tempo, from 10
to 1000 BPM. Double-click it to type a value, or use its arrows to change it
one step at a time. The ruler, grid and metronome follow the tempo.

## Metronome

**Ctrl+M** or the metronome button turns the click on or off. While
playback is running, the button blinks on each beat. To change the click
volume, go to **Settings → General → Metronome level**.
