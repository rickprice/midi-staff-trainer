use std::fmt;

/// Letter names for each of the 12 chromatic pitch classes, expressed as sharps.
const LETTER_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// Diatonic step (0–6) for each chromatic pitch class.
/// Accidentals share the diatonic step of the natural below them.
const DIATONIC_STEPS: [i32; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];

/// A note identified by its MIDI note number (0–127).
/// Middle C is MIDI 60 (C4 in scientific pitch notation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Note {
    pub midi: u8,
}

impl Note {
    /// Construct a note from a raw MIDI note number.
    #[inline]
    pub const fn new(midi: u8) -> Self {
        Self { midi }
    }

    /// Letter name of the pitch class, e.g. `"C"`, `"F#"`.
    /// Accidentals are represented as sharps.
    #[must_use]
    #[inline]
    pub fn letter(self) -> &'static str {
        LETTER_NAMES[(self.midi % 12) as usize]
    }

    /// Scientific octave number; C4 = MIDI 60 → octave 4.
    #[must_use]
    #[inline]
    pub fn octave(self) -> i32 {
        (self.midi as i32 / 12) - 1
    }

    /// Returns `true` for black-key (sharp/flat) notes.
    #[must_use]
    #[inline]
    pub fn is_accidental(self) -> bool {
        matches!(self.midi % 12, 1 | 3 | 6 | 8 | 10)
    }

    /// Diatonic staff position relative to C4.
    ///
    /// C4 = 0, D4 = 1, E4 = 2 (treble bottom line), F4 = 3, G4 = 4,
    /// A4 = 5, B4 = 6, C5 = 7, … One octave = 7 diatonic steps.
    /// Accidentals share the position of the natural below them.
    #[must_use]
    #[inline]
    pub fn staff_position(self) -> i32 {
        let diatonic = DIATONIC_STEPS[(self.midi % 12) as usize];
        let octave_offset = (self.octave() - 4) * 7;
        diatonic + octave_offset
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.letter(), self.octave())
    }
}

/// Returns every natural (non-accidental) MIDI note number in `low..=high`,
/// in ascending order.
#[must_use]
pub fn natural_notes_in_range(low: u8, high: u8) -> Vec<u8> {
    (low..=high)
        .filter(|&m| !Note::new(m).is_accidental())
        .collect()
}

/// Picks a uniformly random natural note within `low..=high`.
/// Falls back to C4 (MIDI 60) when the range contains no natural notes.
#[must_use]
pub fn random_natural_note(low: u8, high: u8) -> Note {
    use rand::seq::SliceRandom as _;
    natural_notes_in_range(low, high)
        .choose(&mut rand::thread_rng())
        .copied()
        .map_or(Note::new(60), Note::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Note::letter ─────────────────────────────────────────────────────────

    #[test]
    fn letter_natural_notes() {
        let cases = [
            (60, "C"), (62, "D"), (64, "E"), (65, "F"),
            (67, "G"), (69, "A"), (71, "B"),
        ];
        for (midi, expected) in cases {
            assert_eq!(Note::new(midi).letter(), expected, "MIDI {midi}");
        }
    }

    #[test]
    fn letter_accidentals_are_sharps() {
        let cases = [
            (61, "C#"), (63, "D#"), (66, "F#"), (68, "G#"), (70, "A#"),
        ];
        for (midi, expected) in cases {
            assert_eq!(Note::new(midi).letter(), expected, "MIDI {midi}");
        }
    }

    #[test]
    fn letter_repeats_every_octave() {
        assert_eq!(Note::new(48).letter(), "C"); // C3
        assert_eq!(Note::new(60).letter(), "C"); // C4
        assert_eq!(Note::new(72).letter(), "C"); // C5
    }

    // ── Note::octave ──────────────────────────────────────────────────────────

    #[test]
    fn octave_middle_c() {
        assert_eq!(Note::new(60).octave(), 4);
    }

    #[test]
    fn octave_c_notes_across_range() {
        assert_eq!(Note::new(0).octave(), -1);  // C-1
        assert_eq!(Note::new(12).octave(), 0);  // C0
        assert_eq!(Note::new(24).octave(), 1);  // C1
        assert_eq!(Note::new(36).octave(), 2);  // C2
        assert_eq!(Note::new(48).octave(), 3);  // C3
        assert_eq!(Note::new(72).octave(), 5);  // C5
        assert_eq!(Note::new(84).octave(), 6);  // C6
        assert_eq!(Note::new(96).octave(), 7);  // C7
    }

    #[test]
    fn octave_non_c_notes() {
        assert_eq!(Note::new(69).octave(), 4);  // A4  (69/12=5, 5-1=4)
        assert_eq!(Note::new(71).octave(), 4);  // B4
        assert_eq!(Note::new(127).octave(), 9); // G9
    }

    // ── Note::name / Display ─────────────────────────────────────────────────

    #[test]
    fn name_middle_c() {
        assert_eq!(Note::new(60).to_string(), "C4");
        assert_eq!(Note::new(60).to_string(), "C4");
    }

    #[test]
    fn name_accidentals() {
        assert_eq!(Note::new(61).to_string(), "C#4");
        assert_eq!(Note::new(66).to_string(), "F#4");
        assert_eq!(Note::new(70).to_string(), "A#4");
    }

    #[test]
    fn name_across_octaves() {
        assert_eq!(Note::new(48).to_string(), "C3");
        assert_eq!(Note::new(84).to_string(), "C6");
        assert_eq!(Note::new(69).to_string(), "A4");
    }

    // ── Note::is_accidental ───────────────────────────────────────────────────

    #[test]
    fn natural_notes_are_not_accidental() {
        for midi in [48u8, 50, 52, 53, 55, 57, 59, 60, 62, 64, 65, 67, 69, 71, 72] {
            assert!(!Note::new(midi).is_accidental(), "MIDI {midi} should be natural");
        }
    }

    #[test]
    fn sharp_notes_are_accidental() {
        for midi in [49u8, 51, 54, 56, 58, 61, 63, 66, 68, 70, 73] {
            assert!(Note::new(midi).is_accidental(), "MIDI {midi} should be accidental");
        }
    }

    #[test]
    fn is_accidental_repeats_every_octave() {
        for midi in 0u8..12 {
            assert_eq!(
                Note::new(midi).is_accidental(),
                Note::new(midi + 12).is_accidental(),
                "Accidental pattern should repeat at MIDI {midi}",
            );
        }
    }

    // ── Note::staff_position ─────────────────────────────────────────────────

    #[test]
    fn staff_position_c4_is_zero() {
        assert_eq!(Note::new(60).staff_position(), 0);
    }

    #[test]
    fn staff_position_treble_staff_lines() {
        // From bottom: E4 (line 1) = pos 2, G4 = 4, B4 = 6, D5 = 8, F5 = 10
        assert_eq!(Note::new(64).staff_position(), 2);  // E4
        assert_eq!(Note::new(67).staff_position(), 4);  // G4
        assert_eq!(Note::new(71).staff_position(), 6);  // B4
        assert_eq!(Note::new(74).staff_position(), 8);  // D5
        assert_eq!(Note::new(77).staff_position(), 10); // F5
    }

    #[test]
    fn staff_position_treble_spaces() {
        // Spaces between the five staff lines
        assert_eq!(Note::new(65).staff_position(), 3); // F4  (1st space)
        assert_eq!(Note::new(69).staff_position(), 5); // A4  (2nd space)
        assert_eq!(Note::new(72).staff_position(), 7); // C5  (3rd space)
        assert_eq!(Note::new(76).staff_position(), 9); // E5  (4th space)
    }

    #[test]
    fn staff_position_below_staff() {
        assert_eq!(Note::new(62).staff_position(), 1);  // D4 — space below bottom line
        assert_eq!(Note::new(59).staff_position(), -1); // B3
        assert_eq!(Note::new(57).staff_position(), -2); // A3
        assert_eq!(Note::new(55).staff_position(), -3); // G3
        assert_eq!(Note::new(53).staff_position(), -4); // F3
        assert_eq!(Note::new(52).staff_position(), -5); // E3
        assert_eq!(Note::new(50).staff_position(), -6); // D3
        assert_eq!(Note::new(48).staff_position(), -7); // C3 — 5 ledger lines below
    }

    #[test]
    fn staff_position_above_staff() {
        assert_eq!(Note::new(79).staff_position(), 11); // G5
        assert_eq!(Note::new(81).staff_position(), 12); // A5
        assert_eq!(Note::new(84).staff_position(), 14); // C6
    }

    #[test]
    fn staff_position_accidentals_share_natural_below() {
        // Each accidental must occupy the same position as the natural below it.
        let sharps_and_naturals = [
            (61, 60), // C#4 == C4
            (63, 62), // D#4 == D4
            (66, 65), // F#4 == F4
            (68, 67), // G#4 == G4
            (70, 69), // A#4 == A4
        ];
        for (sharp, natural) in sharps_and_naturals {
            assert_eq!(
                Note::new(sharp).staff_position(),
                Note::new(natural).staff_position(),
                "C#/D# etc. should share position with natural below",
            );
        }
    }

    #[test]
    fn staff_position_one_octave_is_seven_diatonic_steps() {
        for root in [48u8, 50, 52, 53, 55, 57, 59] {
            let low = Note::new(root).staff_position();
            let high = Note::new(root + 12).staff_position();
            assert_eq!(high - low, 7, "Octave should span 7 diatonic steps (root MIDI {root})");
        }
    }

    // ── natural_notes_in_range ────────────────────────────────────────────────

    #[test]
    fn natural_notes_one_octave_c4_to_c5() {
        assert_eq!(
            natural_notes_in_range(60, 72),
            [60, 62, 64, 65, 67, 69, 71, 72],
        );
    }

    #[test]
    fn natural_notes_excludes_all_accidentals() {
        for &m in &natural_notes_in_range(36, 96) {
            assert!(!Note::new(m).is_accidental(), "MIDI {m} should be natural");
        }
    }

    #[test]
    fn natural_notes_includes_both_endpoints_when_natural() {
        let notes = natural_notes_in_range(64, 71); // E4 ..= B4
        assert_eq!(notes.first().copied(), Some(64));
        assert_eq!(notes.last().copied(), Some(71));
    }

    #[test]
    fn natural_notes_single_natural_endpoint() {
        assert_eq!(natural_notes_in_range(60, 60), [60]);
    }

    #[test]
    fn natural_notes_single_accidental_is_empty() {
        assert_eq!(natural_notes_in_range(61, 61), []);
    }

    #[test]
    fn natural_notes_inverted_range_is_empty() {
        assert_eq!(natural_notes_in_range(72, 60), []);
    }

    #[test]
    fn natural_notes_two_naturals_with_accidental_between() {
        // C4, C#4, D4
        assert_eq!(natural_notes_in_range(60, 62), [60, 62]);
    }

    #[test]
    fn natural_notes_count_per_octave() {
        // Seven naturals per octave (C D E F G A B).
        assert_eq!(natural_notes_in_range(60, 71).len(), 7);
    }

    // ── random_natural_note ───────────────────────────────────────────────────

    #[test]
    fn random_note_always_in_range() {
        let (low, high) = (48u8, 84u8);
        for _ in 0..200 {
            let note = random_natural_note(low, high);
            assert!(note.midi >= low && note.midi <= high, "note {note} outside {low}–{high}");
        }
    }

    #[test]
    fn random_note_never_accidental() {
        for _ in 0..200 {
            assert!(!random_natural_note(48, 84).is_accidental());
        }
    }

    #[test]
    fn random_note_fallback_when_no_naturals() {
        assert_eq!(random_natural_note(61, 61).midi, 60); // only C#4 in range → C4 fallback
    }

    #[test]
    fn random_note_single_choice() {
        for _ in 0..20 {
            assert_eq!(random_natural_note(60, 60).midi, 60); // only C4 available
        }
    }

    #[test]
    fn random_note_produces_variety() {
        // With a 3-octave range (C3–C6, 22 natural notes) we should see at least
        // 5 distinct notes in 50 draws — probability of failure is negligibly small.
        let notes: std::collections::HashSet<u8> =
            (0..50).map(|_| random_natural_note(48, 84).midi).collect();
        assert!(notes.len() >= 5, "random_natural_note appears non-random: only {}", notes.len());
    }
}
