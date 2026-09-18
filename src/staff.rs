/// A note on the musical staff, identified by its MIDI note number.
///
/// Middle C is MIDI 60 (C4 in scientific pitch notation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub midi: u8,
}

impl Note {
    pub fn new(midi: u8) -> Self {
        Self { midi }
    }

    /// Letter name: C D E F G A B
    pub fn letter(&self) -> &'static str {
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
            [(self.midi % 12) as usize]
    }

    /// Scientific octave number (C4 = MIDI 60).
    pub fn octave(&self) -> i32 {
        (self.midi as i32 / 12) - 1
    }

    /// Human-readable name, e.g. "C4", "F#3".
    pub fn name(&self) -> String {
        format!("{}{}", self.letter(), self.octave())
    }

    /// True for black keys.
    pub fn is_accidental(&self) -> bool {
        matches!(self.midi % 12, 1 | 3 | 6 | 8 | 10)
    }

    /// Staff position relative to C4 (MIDI 60), in half-steps of staff lines.
    /// Natural notes: C=0, D=1, E=2, F=3, G=4, A=5, B=6 per octave.
    /// Accidentals share the position of the note below them.
    pub fn staff_position(&self) -> i32 {
        // diatonic step within octave (accidentals round down)
        let diatonic = [0i32, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6][(self.midi % 12) as usize];
        let octave_offset = (self.octave() - 4) * 7;
        diatonic + octave_offset
    }
}

/// Generates a random note within the given MIDI range, restricted to natural
/// notes so the staff stays uncluttered for beginners.
pub fn random_natural_note(low: u8, high: u8) -> Note {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Collect all natural (non-accidental) notes in range.
    let naturals: Vec<u8> = (low..=high)
        .filter(|&m| !Note::new(m).is_accidental())
        .collect();
    if naturals.is_empty() {
        return Note::new(60); // fallback to C4
    }
    // Simple LCG for no-dependency randomness.
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    Note::new(naturals[seed % naturals.len()])
}
