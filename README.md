# MIDI Staff Trainer

A desktop app for learning to read sheet music with a MIDI keyboard. Notes are displayed on the treble clef; play the matching key to advance. Supports both a random note trainer with spaced repetition and playback of MIDI files.

## Features

### Staff display
- Treble clef rendered using the Noto Music font glyph (authentic engraved style)
- Multi-note staff display with horizontal paging — see upcoming notes at a glance
- Whole, half, quarter, eighth, and sixteenth note heads with stems and flags
- Ledger lines above and below the staff as needed
- Time signature display parsed from MIDI files (fallback 4/4)
- Beat-accurate bar lines across mixed note values and page flips
- Played notes dim to grey; active note is gold; upcoming notes are medium grey

### Piano keyboard visualization
- Full 88-key keyboard (A0–C8) at the bottom of the window, show/hide with **K**
- Gold key = currently expected note
- Green flash (~500 ms) = correct answer
- Red flash (~500 ms) = wrong answer
- Keys outside the active training range shown slightly dimmer
- Boundary triangles mark the active training range (random mode) or full keyboard range (MIDI file mode)

### Input and feedback
- Connects to any ALSA MIDI input device (USB or hardware MIDI)
- Correct answers flash green and auto-advance after ~1 second
- Playing the next note during the green flash immediately advances
- Wrong answers flash red and prompt you to try again
- Notes played outside the active training range are identified without counting as an attempt

### Random trainer
- Configurable note range to match non-full-sized keyboards
- Set range interactively: press **R**, then play any two keys
- Leitner spaced-repetition scheduler: 5 box levels; weaker notes appear more often
- Response latency tracking: fast answers (< 1500 ms) advance the note up a box; slow correct answers stay; wrong answers reset to box 0
- Training range remembered across sessions (`~/.cache/midi-staff-trainer/state.toml`)

### MIDI file mode
- Open any `.mid` file via the file picker
- Note durations (whole/half/quarter/eighth/sixteenth) preserved from the file
- Time signature read from the file

### General
- Score tracking (correct / total attempts, accuracy %)
- Restart current song/session: **Ctrl+R** or the Restart button
- All state persisted in `~/.cache/midi-staff-trainer/state.toml`

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| **R** | Set training range (random mode) |
| **Ctrl+R** | Restart current song / session |
| **K** | Toggle piano keyboard display |
| **Esc** | Cancel range-setting |

## Requirements

- Linux with ALSA
- A MIDI keyboard connected via USB or hardware MIDI
- [Nix](https://nixos.org/) (recommended) or a Rust toolchain with the system libraries below

System libraries required (all provided automatically by `nix develop`):

| Library | Purpose |
|---------|---------|
| `alsa-lib` | MIDI input via ALSA |
| `libxkbcommon` | Keyboard input handling |
| `libx11`, `libxcursor`, `libxrandr`, `libxi` | X11 window integration |
| `libGL` | OpenGL rendering (egui back-end) |

## Building

### With Nix (recommended)

```sh
nix develop
cargo run --release
```

### Without Nix

Install the system libraries (`libasound2-dev`, `libxkbcommon-dev`, `libx11-dev`, `libgl-dev` or your distro's equivalents), then:

```sh
cargo build --release
./target/release/midi-staff-trainer
```

## Testing

```sh
nix develop
cargo test
```

## Configuration

On first launch a config file is written to `~/.config/midi-staff-trainer/config.toml`:

```toml
midi_low = 41   # F2 — lowest note your keyboard can send
midi_high = 72  # C5 — highest note your keyboard can send
midi_port = "A-PRO 1"  # substring of MIDI port name to connect to
```

Set `midi_low` and `midi_high` to the actual physical range of your keyboard using [MIDI note numbers](https://en.wikipedia.org/wiki/Scientific_pitch_notation) (middle C = 60). If `midi_port` is not set, the first available port is used automatically.

Common keyboard ranges:

| Keys | Low | High | Example |
|------|-----|------|---------|
| 25   | 60 (C4) | 84 (C6) | mini controllers |
| 32   | 41 (F2) | 72 (C5) | Roland Cakewalk A300-PRO |
| 49   | 48 (C3) | 84 (C6) | Arturia MiniLab |
| 61   | 36 (C2) | 96 (C7) | Alesis Q61 |
| 88   | 21 (A0) | 108 (C8) | full piano |

## Architecture

| File | Responsibility |
|------|---------------|
| `src/app.rs` | egui application — staff drawing, piano keyboard, MIDI polling, feedback state |
| `src/song.rs` | `Song` enum unifying `RandomSong` and `MidiFileSong` — note delivery, restart, range |
| `src/staff.rs` | `Note` type — letter names, octaves, staff positions |
| `src/scheduler.rs` | Leitner spaced-repetition scheduler — box levels, weighted draws, latency tracking |
| `src/config.rs` | TOML config — load / save / defaults (`~/.config/…/config.toml`) |
| `src/state.rs` | Runtime state — training range persisted across sessions (`~/.cache/…/state.toml`) |
| `src/midi.rs` | ALSA MIDI input connection and port listing |
| `src/main.rs` | Entry point |

## License

BSD 3-Clause — see [LICENSE](LICENSE).
