#!/usr/bin/env python3
"""Generates the recording cues from scratch (no third-party audio).

Both cues are a short "water drop": a sine whose pitch glides while its level decays.
The start cue glides up, the end cue glides down. Output is 16-bit stereo PCM at 48 kHz,
the format `src-tauri/src/audio/cue.rs` expects.

    python3 scripts/generate-cues.py
"""

import math
import struct
import wave
from pathlib import Path

RATE = 48_000
PEAK = 0.55  # leaves headroom; playback volume is applied on top in cue.rs


def drop(start_hz, end_hz, seconds, decay, second_harmonic=0.18):
    frames = int(RATE * seconds)
    samples = []
    phase = 0.0
    for i in range(frames):
        t = i / RATE
        progress = i / frames
        # Fast glide at the start that settles, like a drop hitting water.
        glide = 1 - math.exp(-6 * progress)
        hz = start_hz + (end_hz - start_hz) * glide
        phase += 2 * math.pi * hz / RATE
        attack = min(1.0, t / 0.004)
        release = min(1.0, (seconds - t) / 0.02)
        envelope = attack * release * math.exp(-decay * t)
        tone = math.sin(phase) + second_harmonic * math.sin(2 * phase)
        samples.append(PEAK * envelope * tone / (1 + second_harmonic))
    return samples


def write(path, mono):
    with wave.open(str(path), "wb") as out:
        out.setnchannels(2)
        out.setsampwidth(2)
        out.setframerate(RATE)
        frames = bytearray()
        for value in mono:
            pcm = int(max(-1.0, min(1.0, value)) * 32767)
            frames += struct.pack("<hh", pcm, pcm)
        out.writeframes(bytes(frames))


if __name__ == "__main__":
    target = Path(__file__).resolve().parent.parent / "src-tauri" / "sounds"
    write(target / "record-start.wav", drop(520, 1040, 0.26, 9))
    write(target / "record-end.wav", drop(1040, 520, 0.26, 9))
    print(f"wrote cues to {target}")
