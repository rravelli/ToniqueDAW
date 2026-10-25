# Fonts

Bundled into the binary by `src/ui/font.rs`. All are under the SIL Open
Font License 1.1 (see the `*-LICENSE.txt` files).

| File | From | Prepared with |
|---|---|---|
| `Inter-Regular.ttf` | [Inter 4.1](https://github.com/rsms/inter/releases/tag/v4.1), `extras/ttf/Inter-Regular.ttf` | `prepare_font.py --tabular` |
| `Inter-SemiBold.ttf` | Inter 4.1, `extras/ttf/Inter-SemiBold.ttf` | `prepare_font.py --tabular` |
| `JetBrainsMonoNL-Regular.ttf` | [JetBrains Mono 2.304](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304), `fonts/ttf/JetBrainsMonoNL-Regular.ttf` | `prepare_font.py` |

`prepare_font.py` (needs `fonttools`) removes the private-use code points,
which would hide the icon font's glyphs, keeping everything else. With
`--tabular`, the digits are the font's tabular ones (its `tnum` feature),
all the same width: egui can't turn font features on, and values that
change mustn't shift.

```sh
python3 fonts/prepare_font.py Inter-Regular.ttf fonts/Inter-Regular.ttf --tabular
```
