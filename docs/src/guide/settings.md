# Settings and themes

To open the settings, click **Settings** in the menu bar or press
**Ctrl+,**. The settings window has four tabs.

## General

- **UI scale**: the size of the whole interface. You can also change it with
  **Ctrl++**, **Ctrl+-** and **Ctrl+0**.
- **Metronome level**: the volume of the metronome click.

## Appearance

### Theme

Choose one of the built-in themes: Dark, Light, Graphite, Slate, Studio,
Console, Session, Tape and High contrast.

You can't edit the built-in themes. To make your own theme:

1. Select the theme you want to start from.
2. Click **Duplicate**.
3. Change its name and colors. Changes apply as you make them.

If a color makes text hard to read, a warning appears next to it. Hover
over the warning to see the problem. You can also restore any color to the
value of the theme it was based on.

Your themes are saved as `.toml` files in the `themes` folder of the
[configuration folder](../reference/files.md). If you edit one of these
files in a text editor, click **Reload themes from disk** to see the
changes.

### Palette

The palette is the list of colors given to new tracks. Click **+** to add a
color, right-click a color to remove it, or restore the theme's default
palette.

## Audio

### Audio output

- **Device**: the output device used for playback. Leave it on **System
  default** unless you need a specific device. Click the refresh button
  after plugging in a new device.
- **Sample rate**: the rate the device runs at. **Device default** is
  usually the best choice.
- **Buffer size**: how many audio frames are sent to the device at a time.
  Smaller buffers make the app react faster but can cause clicks and
  dropouts. If you hear crackling, increase the buffer size.

The line below these settings shows the device that's actually running and
its settings.

### Engine

Most users can leave these settings at their defaults.

- **Processing block**: the largest block of audio the engine processes at
  once.
- **Worker threads**: extra threads that process independent parts of the
  project in parallel. Set it to `0` to run everything on the audio thread.
- **Parallel threshold**: projects smaller than this run on a single
  thread, because sharing the work between threads would cost more than it
  saves.

Audio changes take effect only when you click **Apply audio changes**. This
restarts the audio output, but your playhead position and undo history are
kept. Click **Revert** to discard the changes, or **Restore defaults** to go
back to the default settings.

## Shortcuts

Every command has a shortcut that you can change.

- **Click a shortcut** and press the new key combination. Press **Esc** to
  cancel.
- **Right-click a shortcut** to remove it.
- Click **+** to give a command an extra shortcut.
- Click the reset button next to a command to restore its default
  shortcut, or click **Reset all** to restore all of them.

If the new shortcut is already used by another command, it's removed from
that command.

You can't change Copy, Cut, Paste or the interface zoom shortcuts. They're
shown in the list for reference.

All default shortcuts are listed in
[Keyboard shortcuts](../reference/shortcuts.md).
