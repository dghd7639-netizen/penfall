# Sound sources

Retrieval date: 2026-09-28

The recording cues are byte-for-byte copies of the files bundled in the installed Typeless 2.8.0 app (`now.typeless.desktop`), extracted from `/Applications/Typeless.app/Contents/Resources/app.asar`. The copies match the SHA-256 integrity hashes recorded in the asar header. They are embedded in the binary with `include_bytes!`, and nothing is fetched at runtime. Typeless owns these sounds. This is a personal build: do not include them in a public repository, an upstream contribution, or a distributed binary without permission.

| File | Source | SHA-256 |
| --- | --- | --- |
| `src-tauri/sounds/record-start.wav` | `app.asar:/dist/renderer/static/wav/7ZwPubNT.wav` (Typeless `record-start`) | `93b0bd239391d699b2c5fb3c44a69eb72d8f97806e8586616aacfbdf7e6df044` |
| `src-tauri/sounds/record-end.wav` | `app.asar:/dist/renderer/static/wav/BGK9atS4.wav` (Typeless `record-end`) | `2f4aa608cd72869399c32cc76a3e5e7c78a653a491f39ea4a96fc973d0561357` |

Playback behavior taken from the same Typeless build (`src-tauri/src/audio/cue.rs`):

- Volume 0.4 (Howler `volume: 0.4`, linear gain).
- Playing a cue stops any cue that is still playing.
- The start cue plays once the microphone stream is open. It is delayed by 1200 ms when the input and default output are the same device, by 300 ms when the output is AirPods, and not delayed otherwise.
- The end cue plays as soon as a recording is stopped for transcription.

Local addition, not in Typeless: while a cue plays, captured microphone audio is replaced with silence for exactly the cue's length (0.44 s). Otherwise speech recognition picks up the chime from the speakers and transcribes it as "嗯". The start cue skips this when its start is delayed (headset or AirPods output), because little of the chime leaks into the mic and the user may already be speaking.
