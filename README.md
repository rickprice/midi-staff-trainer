# MIDI Staff Trainer

A desktop app for learning the connection between keys on a MIDI keyboard and notes on the musical staff. A random note is displayed on the treble clef; play the matching key to advance to the next one.

## Features

- Treble clef rendered using the Noto Music font glyph (authentic engraved style)
- Ledger lines above and below the staff as needed
- Connects to any ALSA MIDI input device (USB or hardware MIDI)
- Note range is configurable to match non-full-sized keyboards
- Score tracking (correct / total attempts, accuracy %)
- Note name displayed at the bottom of the screen as a reference hint (not next to the note, to encourage reading staff position)
- Correct answers flash green and auto-advance after ~1 second
- Wrong answers flash red and prompt you to try again
- Notes played outside the active training range are identified by name without counting as an attempt
- Training range is remembered across sessions (stored in `~/.cache/midi-staff-trainer/state.toml`)
- Leitner spaced-repetition scheduler: 5 box levels with weighted random draws (box 0 weight 10×, box 4 weight 0.5×) so weaker notes appear more often
- Response latency tracking: answers within 1500 ms count as "fast" and advance the note to the next box; slower correct answers stay in the current box; wrong answers reset the note to box 0

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

The test suite covers `Note` (letter names, octaves, display, accidentals, staff positions), `natural_notes_in_range` (range filtering, edge cases), `random_natural_note` (range correctness, no accidentals, variety), `Config` (defaults, TOML round-trips, partial overrides), and `AppState` (defaults, TOML round-trips, missing-file fallback).

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
| `src/staff.rs` | `Note` type — letter names, octaves, staff positions, random picker |
| `src/scheduler.rs` | Leitner spaced-repetition scheduler — box levels, weighted draws, latency tracking |
| `src/config.rs` | TOML config — load / save / defaults (`~/.config/…/config.toml`) |
| `src/state.rs` | Runtime state — training range persisted across sessions (`~/.cache/…/state.toml`) |
| `src/midi.rs` | ALSA MIDI input connection and port listing |
| `src/app.rs` | egui application — staff drawing, MIDI polling, feedback state |
| `src/main.rs` | Entry point |

## License

BSD 3-Clause — see [LICENSE](LICENSE).
