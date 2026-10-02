#!/usr/bin/env python3
"""Soft recording-cue candidates kept for later (not used by the app).

Run `python3 scripts/generate-cue-candidates.py` to regenerate the WAVs in
`scripts/cue-candidates/`. To switch the app to one of them, copy its `*_start.wav` and
`*_end.wav` over `src-tauri/sounds/record-start.wav` and `record-end.wav`. The files are
16-bit stereo PCM at 48 kHz, the format `src-tauri/src/audio/cue.rs` decodes. Everything is
synthesized here; there is no third-party audio.

  4_wood_soft    low, round wooden knock with a little ring (start higher, end lower)
  10_warm_low    one low, warm tone with a slow attack that glides up / down
  13_gentle_drop a round, low-pitched water drop with a slow attack
"""

import math
import struct
import wave
from pathlib import Path

RATE = 48_000


def write(path, mono, peak):
    top = max(abs(x) for x in mono) or 1
    with wave.open(str(path), "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        frames = bytearray()
        for value in mono:
            pcm = int(max(-1, min(1, value / top * peak)) * 32767)
            frames += struct.pack("<hh", pcm, pcm)
        out.writeframes(bytes(frames))


def note(freq, dur, partials, decay, attack):
    samples = []
    for i in range(int(RATE * dur)):
        t = i / RATE
        envelope = min(1, t / attack) * math.exp(-decay * t) * min(1, (dur - t) / 0.02)
        samples.append(envelope * sum(a * math.sin(2 * math.pi * freq * k * t) for k, a in partials))
    return samples


def glide(f0, f1, dur, decay, attack):
    count = int(RATE * dur)
    phase = 0.0
    samples = []
    for i in range(count):
        t = i / RATE
        settle = 1 - math.exp(-5 * i / count)
        phase += 2 * math.pi * (f0 + (f1 - f0) * settle) / RATE
        envelope = min(1, t / attack) * math.exp(-decay * t) * min(1, (dur - t) / 0.04)
        samples.append(envelope * (math.sin(phase) + 0.12 * math.sin(2 * phase)))
    return samples


def padded(samples, total):
    return samples + [0.0] * max(0, int(RATE * total) - len(samples))


WOOD = ((1, 1.0), (2.4, 0.18))

CANDIDATES = {
    "4_wood_soft": (
        padded(note(700, 0.22, WOOD, 24, 0.002), 0.3),
        padded(note(500, 0.22, WOOD, 24, 0.002), 0.3),
        0.55,
    ),
    "10_warm_low": (glide(330, 392, 0.34, 7, 0.03), glide(392, 330, 0.34, 7, 0.03), 0.42),
    "13_gentle_drop": (glide(380, 640, 0.22, 11, 0.012), glide(640, 380, 0.22, 11, 0.012), 0.42),
}

if __name__ == "__main__":
    target = Path(__file__).resolve().parent / "cue-candidates"
    target.mkdir(exist_ok=True)
    for name, (start, end, peak) in CANDIDATES.items():
        write(target / f"{name}_start.wav", start, peak)
        write(target / f"{name}_end.wav", end, peak)
    print(f"wrote {len(CANDIDATES) * 2} files to {target}")
