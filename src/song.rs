use crate::staff::Note;
use std::path::Path;

pub struct SongPlayer {
    pub filename: String,
    notes: Vec<u8>,
    pub index: usize,
}

impl SongPlayer {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("Cannot read file: {e}"))?;
        let smf = midly::Smf::parse(&data).map_err(|e| format!("Invalid MIDI file: {e}"))?;

        // Collect (absolute_tick, midi_note) for every NoteOn with velocity > 0.
        let mut timed: Vec<(u64, u8)> = Vec::new();
        for track in &smf.tracks {
            let mut tick: u64 = 0;
            for event in track {
                tick += event.delta.as_int() as u64;
                if let midly::TrackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { key, vel },
                    ..
                } = event.kind
                {
                    if vel.as_int() > 0 {
                        timed.push((tick, key.as_int()));
                    }
                }
            }
        }

        if timed.is_empty() {
            return Err("No notes found in MIDI file".to_string());
        }

        // Sort by time; for simultaneous notes, order lowest pitch first.
        timed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let notes: Vec<u8> = timed.into_iter().map(|(_, n)| n).collect();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.mid")
            .to_string();

        Ok(Self { filename, notes, index: 0 })
    }

    pub fn current(&self) -> Option<Note> {
        self.notes.get(self.index).copied().map(Note::new)
    }

    pub fn advance(&mut self) {
        if self.index < self.notes.len() {
            self.index += 1;
        }
    }

    pub fn total(&self) -> usize {
        self.notes.len()
    }

    pub fn is_complete(&self) -> bool {
        self.index >= self.notes.len()
    }

    pub fn restart(&mut self) {
        self.index = 0;
    }
}
