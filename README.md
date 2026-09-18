# MIDI Staff Trainer

A desktop app for learning the connection between keys on a MIDI keyboard and notes on the musical staff. A random note is displayed on the treble clef; play the matching key to advance to the next one.

## Features

- Treble clef staff rendered with ledger lines as needed
- Connects to any ALSA MIDI input device
- Configurable note range to match non-full-sized keyboards
- Score tracking (correct / total attempts)
- Correct answers auto-advance after a short delay

## Requirements

- Linux with ALSA
- A MIDI keyboard connected via USB or hardware MIDI
- [Nix](https://nixos.org/) (recommended) or a Rust toolchain with `alsa-lib`, `libxkbcommon`, `wayland`, and `libGL` available

## Building

### With Nix (recommended)

```sh
nix develop
cargo run --release
```

### Without Nix

Install the system libraries (`alsa-lib-dev`, `libxkbcommon-dev`, `libwayland-dev`, `libgl-dev` or distro equivalents), then:

```sh
cargo build --release
./target/release/midi-staff-trainer
```

## Configuration

On first launch a config file is written to `~/.config/midi-staff-trainer/config.toml`:

```toml
midi_low = 48   # C3 — lowest note your keyboard can send
midi_high = 84  # C6 — highest note your keyboard can send
midi_port = ""  # optional: substring of the MIDI port name to connect to
```

Adjust `midi_low` and `midi_high` to the actual range of your keyboard using [MIDI note numbers](https://en.wikipedia.org/wiki/Scientific_pitch_notation#Table_of_note_frequencies) (middle C = 60). If `midi_port` is empty the first available port is used.

Common keyboard ranges:

| Keys | Low | High | Example |
|------|-----|------|---------|
| 25   | 60 (C4) | 84 (C6) | mini controllers |
| 49   | 48 (C3) | 84 (C6) | Arturia MiniLab |
| 61   | 36 (C2) | 96 (C7) | Alesis Q61 |
| 88   | 21 (A0) | 108 (C8) | full piano |

## License

BSD 3-Clause — see [LICENSE](LICENSE).
