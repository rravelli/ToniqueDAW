# Effects

Effects process a track's or a group's sound. Each track and group has its
own chain of effects, which are applied in order from left to right.

> For now, the only available effect is a **low-pass filter**. More are
> planned.

## Adding an effect

1. Open the **Effects** tab of the browser.
2. Drag **Filter** onto one of these:
   - a track header, or the track's lane in the timeline. The effect is
     added at the start of that track's chain.
   - the effects panel. The effect is added at the end of the selected
     track's chain.

When you drop on the timeline, the **effects panel** opens at the bottom of
the window.

## The effects panel

The effects panel shows the effects of the **selected track**. To see
another track's effects, select that track. To show or hide the panel,
press **Ctrl+J** or double-click a track header.

In the panel, you can:

- **Bypass** an effect with its power button. A bypassed effect stays in
  the chain but does nothing.
- **Select** an effect by clicking its header. Click it again to deselect
  it.
- **Remove** the selected effect with **Delete**.

## Filter

The filter cuts frequencies above the **cutoff** and lets lower ones
through. Its editor shows the filter's curve over the live spectrum of the
sound.

| Knob     | What it does                                                   |
| -------- | -------------------------------------------------------------- |
| **Freq** | The cutoff frequency. Frequencies above it are reduced.        |
| **Q**    | The resonance. Higher values add a peak just below the cutoff. |
