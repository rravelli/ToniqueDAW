#!/usr/bin/env python3
"""Builds the Tonique demo song: synthetic stems (WAV) and `Demo Song.tonique`
in the folder given (made if missing). Pure standard library.

The README's images are taken of it (see `readme_images` in
src/ui/app/tests.rs):

    scripts/make_demo.py "$HOME/Documents/Tonique Projects/Demo Song"
"""

import json
import math
import random
import struct
import sys
import wave
from pathlib import Path

OUT = Path(".")
RATE = 44100
BPM = 120.0
BEAT = 60.0 / BPM  # seconds
BAR = 4 * BEAT
random.seed(7)

# A minor: Am F C G, one chord a bar.
CHORDS = [
    [57, 60, 64],  # A C E
    [53, 57, 60],  # F A C
    [48, 55, 64],  # C G E
    [55, 59, 62],  # G B D
]


def hz(note):
    return 440.0 * 2 ** ((note - 69) / 12)


def silence(seconds, channels=1):
    n = int(seconds * RATE)
    return [[0.0] * n for _ in range(channels)]


def add(buf, at, samples, gain=1.0, pan=0.0):
    """Mixes mono `samples` into `buf` from `at` seconds, panned."""
    start = int(at * RATE)
    sides = [1.0] if len(buf) == 1 else [min(1.0, 1.0 - pan), min(1.0, 1.0 + pan)]
    for ch, side in enumerate(sides):
        out = buf[ch]
        for i, s in enumerate(samples):
            j = start + i
            if j >= len(out):
                break
            out[j] += s * gain * side


def normalize(buf, peak=0.89):
    top = max(max(abs(s) for s in ch) for ch in buf) or 1.0
    return [[s * peak / top for s in ch] for ch in buf]


def write(name, buf):
    buf = normalize(buf)
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(len(buf))
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for i in range(len(buf[0])):
            for ch in buf:
                frames += struct.pack("<h", int(max(-1.0, min(1.0, ch[i])) * 32767))
        w.writeframes(bytes(frames))


def kick():
    n = int(0.35 * RATE)
    out, phase = [], 0.0
    for i in range(n):
        t = i / RATE
        phase += 2 * math.pi * (45 + 110 * math.exp(-t * 30)) / RATE
        out.append(math.sin(phase) * math.exp(-t * 9))
    return out


def noise_hit(length, decay, tone=0.0):
    out, last = [], 0.0
    for i in range(int(length * RATE)):
        t = i / RATE
        x = random.uniform(-1, 1)
        # Crude high-pass: what's left after a one-pole low-pass.
        last += 0.35 * (x - last)
        hp = x - last
        body = math.sin(2 * math.pi * 190 * t) * tone
        out.append((hp + body) * math.exp(-t * decay))
    return out


def saw(freq, length, decay, harmonics=12, attack=0.005):
    out = []
    for i in range(int(length * RATE)):
        t = i / RATE
        s = sum(math.sin(2 * math.pi * freq * k * t) / k for k in range(1, harmonics))
        env = min(1.0, t / attack) * math.exp(-t * decay)
        out.append(s * env)
    return out


def pad(notes, length):
    out = []
    for i in range(int(length * RATE)):
        t = i / RATE
        s = 0.0
        for note in notes:
            f = hz(note)
            s += math.sin(2 * math.pi * f * t) + 0.4 * math.sin(2 * math.pi * f * 1.003 * t)
            s += 0.2 * math.sin(2 * math.pi * f * 2 * t)
        # Swells in and out, with a slow tremolo.
        env = math.sin(math.pi * t / length) ** 0.6 * (0.8 + 0.2 * math.sin(2 * math.pi * 4 * t))
        out.append(s * env)
    return out


def drums():
    k, c = kick(), noise_hit(0.25, 18, tone=0.6)
    kicks = silence(2 * BAR)
    for b in range(8):
        add(kicks, b * BEAT, k)
    write("Kick.wav", kicks)
    claps = silence(2 * BAR)
    for b in (1, 3, 5, 7):
        add(claps, b * BEAT, c)
    write("Clap.wav", claps)
    hats = silence(2 * BAR, 2)
    for step in range(32):
        open_hat = step % 4 == 2
        hit = noise_hit(0.18 if open_hat else 0.05, 14 if open_hat else 60)
        add(hats, step * BEAT / 4, hit, gain=1.0 if step % 2 == 0 else 0.55, pan=0.2)
    write("Hats.wav", hats)


def bass():
    out = silence(4 * BAR, 2)
    for bar, chord in enumerate(CHORDS):
        root = chord[0] - 24
        for eighth in range(8):
            if eighth in (3, 7):
                continue
            note = root + (12 if eighth == 6 else 0)
            add(out, bar * BAR + eighth * BEAT / 2, saw(hz(note), BEAT / 2, 6, harmonics=8))
    write("Bass.wav", out)


def chords():
    out = silence(4 * BAR, 2)
    for bar, chord in enumerate(CHORDS):
        notes = chord + [chord[0] + 12]
        add(out, bar * BAR, pad(notes, BAR), pan=-0.3)
        add(out, bar * BAR, pad([n + 12 for n in chord], BAR), gain=0.4, pan=0.4)
    write("Chords.wav", out)


def lead():
    out = silence(4 * BAR, 2)
    for bar, chord in enumerate(CHORDS):
        pattern = [chord[0], chord[1], chord[2], chord[0] + 12]
        for step in range(16):
            if step % 8 == 7:
                continue
            note = pattern[step % 4] + 12
            pan = -0.5 if step % 2 else 0.5
            add(out, bar * BAR + step * BEAT / 4, saw(hz(note), BEAT / 4, 14, 10), pan=pan)
    write("Lead.wav", out)


def riser():
    length = 2 * BAR
    out = silence(length, 2)
    samples, phase, last = [], 0.0, 0.0
    for i in range(int(length * RATE)):
        t = i / RATE
        phase += 2 * math.pi * (200 + 1800 * (t / length) ** 2) / RATE
        x = random.uniform(-1, 1)
        last += (0.05 + 0.6 * t / length) * (x - last)
        samples.append((0.6 * last + 0.4 * math.sin(phase)) * (t / length) ** 1.5)
    add(out, 0, samples)
    write("Riser.wav", out)


def clip(path, bar, bars, gain=1.0, looped=None, offset=0.0):
    c = {"path": path, "position": bar * 4.0, "offset": offset, "length": bars * 4.0}
    if looped:
        c["looped"] = {"start": looped[0], "end": looped[1]}
    if gain != 1.0:
        c["gain"] = gain
    return c


def track(name, color, clips, volume=0.8, group=None, height=60.0):
    return {
        "name": name,
        "color": color,
        "height": height,
        "collapsed": False,
        "soloed": False,
        "group": group,
        "volume": volume,
        "pan": 0.0,
        "muted": False,
        "effects": [],
        "clips": clips,
    }


def project():
    two = 2 * BAR  # a two-bar drum loop, in seconds
    tracks = [
        # Bars count from 0 here; the app shows them from 1.
        # Clips longer than their file loop it: without a loop, they'd be
        # cut at the file's end.
        track("Kick", "#e5604c", [clip("Kick.wav", 4, 8, looped=(0.0, two)),
                                   clip("Kick.wav", 12, 4, looped=(0.0, two)),
                                   clip("Kick.wav", 20, 8, looped=(0.0, two))], group=0),
        track("Clap", "#ec8a3f", [clip("Clap.wav", 8, 8, looped=(0.0, two)),
                                   clip("Clap.wav", 20, 8, looped=(0.0, two))], group=0),
        # One clip looping its second bar after the first: shows the loop lines.
        track("Hats", "#e8b33c", [clip("Hats.wav", 4, 12, looped=(BAR, two)),
                                   clip("Hats.wav", 20, 8, looped=(0.0, two))], group=0),
        track("Bass", "#4fbf7f", [clip("Bass.wav", 8, 4), clip("Bass.wav", 12, 4),
                                   clip("Bass.wav", 20, 8, looped=(0.0, 4 * BAR))], volume=0.7),
        track("Chords", "#4a9fe0", [clip("Chords.wav", 0, 4), clip("Chords.wav", 4, 4),
                                     clip("Chords.wav", 8, 8, looped=(0.0, 4 * BAR)),
                                     clip("Chords.wav", 20, 8, looped=(0.0, 4 * BAR))],
              volume=0.6),
        track("Lead", "#9a7be6", [clip("Lead.wav", 12, 4), clip("Lead.wav", 20, 4, gain=0.7),
                                   clip("Lead.wav", 24, 4)], volume=0.55),
        track("Riser", "#d46cc8", [clip("Riser.wav", 6, 2), clip("Riser.wav", 18, 2)],
              volume=0.5),
    ]
    return {
        "version": 5,
        "bpm": BPM,
        "loop_range": [32.0, 64.0],
        "looping": True,
        "master": {"volume": 0.8, "pan": 0.0, "muted": False, "effects": []},
        "groups": [{
            "name": "Drums",
            "color": "#e5604c",
            "height": 60.0,
            "collapsed": False,
            "soloed": False,
            "parent": None,
            "volume": 0.9,
            "pan": 0.0,
            "muted": False,
            "effects": [],
        }],
        "tracks": tracks,
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} OUTPUT_FOLDER")
    OUT = Path(sys.argv[1])
    OUT.mkdir(parents=True, exist_ok=True)
    drums()
    bass()
    chords()
    lead()
    riser()
    path = OUT / "Demo Song.tonique"
    path.write_text(json.dumps(project(), indent=2))
    print(f"wrote {path}")
