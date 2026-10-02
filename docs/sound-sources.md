# Sound sources

Four selectable cue pairs ship with the app (Settings → General → Cue sound, or off); all are
original, synthesized by the scripts below: `water-drop` (the default, `record-start/end.wav`,
from `scripts/generate-cues.py`) and `wood-soft`, `warm-low`, `gentle-drop`
(`wood-soft-*`, `warm-low-*`, `gentle-drop-*`, from `scripts/generate-cue-candidates.py`, which
also writes copies to `scripts/cue-candidates/`). The config value is `cue_sound`.

The default cue is original: `src-tauri/sounds/record-start.wav` and `record-end.wav` are
generated from scratch by `scripts/generate-cues.py` (plain Python, no third-party audio or
samples). Re-run the script to regenerate them or to change the sound.

Design: a short "water drop" of about 0.26 s. A sine tone with a faint second harmonic glides in
pitch while its level decays. The start cue glides up (520 → 1040 Hz) and the end cue glides
down (1040 → 520 Hz). The files are 16-bit stereo PCM at 48 kHz, the format
`src-tauri/src/audio/cue.rs` decodes, and are embedded in the binary with `include_bytes!`;
nothing is fetched at runtime.

Playback behavior (`src-tauri/src/audio/cue.rs`):

- Volume 0.4 (linear gain).
- Playing a cue stops any cue that is still playing.
- The start cue plays once the microphone stream is open. It waits 1200 ms when the input and
  default output are the same device, and 300 ms when the output is AirPods: a Bluetooth headset
  that is also the microphone switches profile and swallows the first moments of audio.
- The end cue plays as soon as a recording is stopped for transcription.
- While a cue plays, captured microphone audio is replaced with silence for exactly the cue's
  length. Otherwise speech recognition picks up the chime from the speakers and transcribes it
  as "嗯". The start cue skips this when its start is delayed (headset or AirPods output),
  because little of the chime leaks into the mic and the user may already be speaking.

History: earlier builds embedded two third-party cue files. They were replaced by these original
sounds, so the project no longer ships any third-party audio.
