# tonique-gpui

Tonique's timeline and file browser rebuilt with
[gpui-fast](https://github.com/longbridge/gpui-fast), to compare with the egui
app (`crates/tonique-egui`).

Only the UI is new. The project state, engine, grid, clip maths, audio
analysis, config and the browser's file tree are the workspace's
`tonique-core` and `tonique-engine` crates, as the egui app uses them, so
both apps edit the same kind of project with the same audio and the same
waveform data. Nothing is written back to Tonique's config. The fonts are
the repository's (`fonts/`); icons are Phosphor's fonts (`assets/fonts`,
MIT) with their codepoints in `src/icons.rs`.

From the repository's root:

```sh
cargo run --release -p tonique-gpui
```

## What's there

- **Transport bar,** laid out as Tonique's: browser toggle and undo/redo
  on the left; metronome, play/stop, loop, follow playhead and the
  position (bars and time) centred; the grid step, tempo and time
  signature, and the audio load on the right (shown, not editable yet).
- **Timeline:** ruler with a loop lane (drag to draw a loop, click to toggle
  it), bar/beat labels and time; click or drag the ruler to seek. Lanes
  with Tonique's adaptive grid. Clips drawn as in Tonique (tinted body,
  solid header, waveform in black or white, selection), with their
  waveforms from Tonique's peaks. Drag a clip's header to move it (and the
  rest of the selection; snaps to the grid, **Alt** for none), its edges to
  trim it; click a body or a lane to place the edit cursor.
  **Ctrl+wheel** zooms around the pointer, **wheel** scrolls the tracks,
  **Shift+wheel** scrolls in time.
- **Track headers:** colour, collapse caret, name, mute, solo
  (**Ctrl** for exclusive), volume in dB.
- **Browser:** the folders from Tonique's config, open/close, background
  search, **arrows**/**Enter** to browse, auto-preview, drag files onto the
  timeline (on a track, or below the last one for a new track).
- **Preview:** name, format, waveform to seek in, play/stop, auto-preview,
  volume.
- **Drops from the file manager** land where the pointer is.
- **Keys:** Space, Del, Ctrl+Z / Ctrl+Shift+Z, Ctrl+D, Ctrl+A, L, Home, Esc,
  Ctrl+T (add track), Ctrl+B (browser), Ctrl+Shift+F (follow playhead).

Left out for now: groups, meters, pan, arm/monitor, selection zones, split,
clipboard, labels, folder menus, the effects panel, saving.

## Layout

| File | |
|---|---|
| `app.rs` | window, frame loop, keys, transport and status bars |
| `timeline.rs` | arrangement: scene, painting, pointer handling, drops |
| `headers.rs` | track headers |
| `browser.rs` | file list, search, preview |
| `wave.rs` | waveforms: Tonique's per-pixel columns and tile cache, drawn as quads |
| `widgets.rs` | buttons, slider, canvas text |
| `theme.rs` | Tonique's "Dark" theme, fonts, icons |
| `icons.rs` | Phosphor codepoints |

Note: `gpui-fast-platform` needs its `wayland`/`x11` features, or it runs
headless and no window appears.
