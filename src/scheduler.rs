use crate::staff::{Note, natural_notes_in_range};
use std::time::{Duration, Instant};

// Box sampling weights — lower index = more frequent draws.
// Tune these constants to adjust the difficulty curve.
pub const BOX_WEIGHTS: [f32; 5] = [10.0, 6.0, 3.0, 1.5, 0.5];

// Response must arrive within this many milliseconds to count as "fast".
pub const FAST_THRESHOLD_MS: u64 = 1500;

pub struct NoteState {
    pub box_level: u8,          // 0–4; all notes start at 0
    pub last_seen: Option<Instant>,
    // Reserved for future ease / interval fields (e.g. SM-2 easiness factor).
}

pub struct Scheduler {
    states: std::collections::HashMap<u8, NoteState>,
    last_note: Option<u8>,
}

impl Scheduler {
    pub fn new(low: u8, high: u8) -> Self {
        let states = natural_notes_in_range(low, high)
            .into_iter()
            .map(|m| (m, NoteState { box_level: 0, last_seen: None }))
            .collect();
        Self { states, last_note: None }
    }

    /// Pick the next note via weighted random draw, never repeating the last
    /// note when more than one candidate exists.
    pub fn pick_next(&mut self) -> Note {
        if self.states.is_empty() {
            return Note::new(60); // fallback: C4
        }

        let mut candidates: Vec<u8> = self.states.keys().copied().collect();
        if candidates.len() > 1
            && let Some(last) = self.last_note {
            candidates.retain(|&m| m != last);
        }

        let weights: Vec<f32> = candidates
            .iter()
            .map(|m| BOX_WEIGHTS[self.states[m].box_level as usize])
            .collect();
        let total: f32 = weights.iter().sum();

        let chosen = if total <= 0.0 {
            candidates[0]
        } else {
            let mut pick = rand::random::<f32>() * total;
            let mut result = *candidates.last().unwrap();
            for (&midi, &w) in candidates.iter().zip(weights.iter()) {
                pick -= w;
                if pick <= 0.0 {
                    result = midi;
                    break;
                }
            }
            result
        };

        if let Some(state) = self.states.get_mut(&chosen) {
            state.last_seen = Some(Instant::now());
        }
        self.last_note = Some(chosen);
        Note::new(chosen)
    }

    /// Correct answer: advance one box if fast, stay put if slow.
    pub fn record_correct(&mut self, midi: u8, latency: Duration) {
        if let Some(state) = self.states.get_mut(&midi)
            && latency.as_millis() <= FAST_THRESHOLD_MS as u128 {
            state.box_level = (state.box_level + 1).min(4);
        }
    }

    /// Wrong answer: target note drops back to box 0.
    pub fn record_incorrect(&mut self, midi: u8) {
        if let Some(state) = self.states.get_mut(&midi) {
            state.box_level = 0;
        }
    }

    /// Number of notes currently in each box level.
    pub fn box_counts(&self) -> [usize; 5] {
        let mut counts = [0usize; 5];
        for state in self.states.values() {
            counts[state.box_level as usize] += 1;
        }
        counts
    }

    /// Box level for a specific MIDI note number (0 if not tracked).
    pub fn note_box(&self, midi: u8) -> u8 {
        self.states.get(&midi).map_or(0, |s| s.box_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast() -> Duration { Duration::from_millis(100) }
    fn slow() -> Duration { Duration::from_millis(FAST_THRESHOLD_MS + 1) }

    // ── Scheduler::new ────────────────────────────────────────────────────────

    #[test]
    fn new_tracks_only_natural_notes() {
        let sched = Scheduler::new(60, 72);
        for (&midi, _) in &sched.states {
            assert!(!Note::new(midi).is_accidental(), "MIDI {midi} should be natural");
        }
    }

    #[test]
    fn new_all_notes_start_in_box_zero() {
        let sched = Scheduler::new(60, 72);
        for state in sched.states.values() {
            assert_eq!(state.box_level, 0);
        }
    }

    #[test]
    fn new_all_notes_have_no_last_seen() {
        let sched = Scheduler::new(60, 72);
        for state in sched.states.values() {
            assert!(state.last_seen.is_none());
        }
    }

    #[test]
    fn new_counts_correct_number_of_notes() {
        // C4 to C5 contains 8 naturals: C D E F G A B C
        let sched = Scheduler::new(60, 72);
        assert_eq!(sched.states.len(), 8);
    }

    #[test]
    fn new_inverted_range_produces_no_states() {
        let sched = Scheduler::new(72, 60);
        assert!(sched.states.is_empty());
    }

    #[test]
    fn new_single_natural_note_range() {
        let sched = Scheduler::new(60, 60);
        assert_eq!(sched.states.len(), 1);
        assert!(sched.states.contains_key(&60));
    }

    #[test]
    fn new_accidental_only_range_is_empty() {
        let sched = Scheduler::new(61, 61); // C#4 only
        assert!(sched.states.is_empty());
    }

    // ── Scheduler::pick_next ─────────────────────────────────────────────────

    #[test]
    fn pick_next_empty_range_returns_c4_fallback() {
        let mut sched = Scheduler::new(61, 61); // no naturals
        assert_eq!(sched.pick_next().midi, 60);
    }

    #[test]
    fn pick_next_single_note_always_returns_that_note() {
        let mut sched = Scheduler::new(60, 60);
        for _ in 0..20 {
            assert_eq!(sched.pick_next().midi, 60);
        }
    }

    #[test]
    fn pick_next_always_returns_natural_note() {
        let naturals: std::collections::HashSet<u8> =
            crate::staff::natural_notes_in_range(60, 72).into_iter().collect();
        let mut sched = Scheduler::new(60, 72);
        for _ in 0..50 {
            let note = sched.pick_next();
            assert!(!Note::new(note.midi).is_accidental(), "MIDI {} is accidental", note.midi);
            assert!(naturals.contains(&note.midi), "MIDI {} not in expected set", note.midi);
        }
    }

    #[test]
    fn pick_next_never_repeats_last_note_when_multiple_available() {
        let mut sched = Scheduler::new(60, 72);
        let mut last = sched.pick_next().midi;
        for _ in 0..100 {
            let next = sched.pick_next().midi;
            assert_ne!(next, last, "pick_next repeated the same note twice in a row");
            last = next;
        }
    }

    #[test]
    fn pick_next_updates_last_seen_for_chosen_note() {
        let mut sched = Scheduler::new(60, 72);
        let note = sched.pick_next();
        assert!(sched.states[&note.midi].last_seen.is_some());
    }

    #[test]
    fn pick_next_visits_all_notes_in_range() {
        // Over enough draws every note in a small range should appear at least once.
        let mut sched = Scheduler::new(60, 67); // C4 D4 E4 F4 G4
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            seen.insert(sched.pick_next().midi);
        }
        for &midi in &[60u8, 62, 64, 65, 67] {
            assert!(seen.contains(&midi), "MIDI {midi} was never picked");
        }
    }

    // ── Scheduler::record_correct ────────────────────────────────────────────

    #[test]
    fn record_correct_fast_advances_box_by_one() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, fast());
        assert_eq!(sched.states[&60].box_level, 1);
    }

    #[test]
    fn record_correct_slow_does_not_advance_box() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, slow());
        assert_eq!(sched.states[&60].box_level, 0);
    }

    #[test]
    fn record_correct_fast_at_exactly_threshold_advances() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, Duration::from_millis(FAST_THRESHOLD_MS));
        assert_eq!(sched.states[&60].box_level, 1);
    }

    #[test]
    fn record_correct_one_over_threshold_does_not_advance() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, Duration::from_millis(FAST_THRESHOLD_MS + 1));
        assert_eq!(sched.states[&60].box_level, 0);
    }

    #[test]
    fn record_correct_caps_at_box_four() {
        let mut sched = Scheduler::new(60, 60);
        for _ in 0..10 {
            sched.record_correct(60, fast());
        }
        assert_eq!(sched.states[&60].box_level, 4);
    }

    #[test]
    fn record_correct_increments_one_at_a_time() {
        let mut sched = Scheduler::new(60, 60);
        for expected in 1..=4u8 {
            sched.record_correct(60, fast());
            assert_eq!(sched.states[&60].box_level, expected);
        }
    }

    #[test]
    fn record_correct_unknown_midi_is_noop() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(62, fast()); // 62 not in this scheduler's range
        assert_eq!(sched.states[&60].box_level, 0);
    }

    // ── Scheduler::record_incorrect ──────────────────────────────────────────

    #[test]
    fn record_incorrect_resets_to_box_zero() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, fast());
        sched.record_correct(60, fast());
        assert_eq!(sched.states[&60].box_level, 2);
        sched.record_incorrect(60);
        assert_eq!(sched.states[&60].box_level, 0);
    }

    #[test]
    fn record_incorrect_already_at_zero_stays_zero() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_incorrect(60);
        assert_eq!(sched.states[&60].box_level, 0);
    }

    #[test]
    fn record_incorrect_unknown_midi_is_noop() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_incorrect(62);
        assert_eq!(sched.states[&60].box_level, 0);
    }

    #[test]
    fn record_incorrect_after_reaching_max_box_resets() {
        let mut sched = Scheduler::new(60, 60);
        for _ in 0..10 {
            sched.record_correct(60, fast());
        }
        assert_eq!(sched.states[&60].box_level, 4);
        sched.record_incorrect(60);
        assert_eq!(sched.states[&60].box_level, 0);
    }

    // ── Scheduler::box_counts ────────────────────────────────────────────────

    #[test]
    fn box_counts_all_in_box_zero_initially() {
        let sched = Scheduler::new(60, 72); // 8 natural notes
        let counts = sched.box_counts();
        assert_eq!(counts[0], 8);
        assert_eq!(counts[1..].iter().sum::<usize>(), 0);
    }

    #[test]
    fn box_counts_reflects_advances() {
        let mut sched = Scheduler::new(60, 72);
        sched.record_correct(60, fast()); // C4 → box 1
        sched.record_correct(62, fast()); // D4 → box 1
        let counts = sched.box_counts();
        assert_eq!(counts[0], 6);
        assert_eq!(counts[1], 2);
    }

    #[test]
    fn box_counts_always_sums_to_total_notes() {
        let mut sched = Scheduler::new(60, 72);
        sched.record_correct(60, fast());
        sched.record_correct(60, fast());
        sched.record_incorrect(62);
        let total: usize = sched.box_counts().iter().sum();
        assert_eq!(total, sched.states.len());
    }

    #[test]
    fn box_counts_reflects_mix_of_levels() {
        let mut sched = Scheduler::new(60, 72);
        sched.record_correct(60, fast()); // C4 → box 1
        sched.record_correct(60, fast()); // C4 → box 2
        sched.record_correct(62, fast()); // D4 → box 1
        let counts = sched.box_counts();
        assert_eq!(counts[0], 6); // 8 notes total, 2 advanced
        assert_eq!(counts[1], 1); // D4
        assert_eq!(counts[2], 1); // C4
    }

    // ── Scheduler::note_box ──────────────────────────────────────────────────

    #[test]
    fn note_box_returns_zero_for_new_note() {
        let sched = Scheduler::new(60, 60);
        assert_eq!(sched.note_box(60), 0);
    }

    #[test]
    fn note_box_returns_updated_level_after_correct_answers() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, fast());
        sched.record_correct(60, fast());
        assert_eq!(sched.note_box(60), 2);
    }

    #[test]
    fn note_box_returns_zero_for_untracked_note() {
        let sched = Scheduler::new(60, 60);
        assert_eq!(sched.note_box(62), 0);
    }

    #[test]
    fn note_box_returns_zero_after_incorrect() {
        let mut sched = Scheduler::new(60, 60);
        sched.record_correct(60, fast());
        sched.record_incorrect(60);
        assert_eq!(sched.note_box(60), 0);
    }

    // ── Weighted selection bias ───────────────────────────────────────────────

    #[test]
    fn higher_box_notes_are_selected_less_often() {
        // 3 naturals: C4 (60), D4 (62), E4 (64).
        // Advance C4 to box 4 (weight 0.5); D4 and E4 stay at box 0 (weight 10).
        // When C4 is the last note, candidates are [D4, E4] — C4 gets 0 picks.
        // Otherwise C4 competes at ~4.8% vs one box-0 note's ~95.2%.
        let mut sched = Scheduler::new(60, 64);
        for _ in 0..4 {
            sched.record_correct(60, fast());
        }
        assert_eq!(sched.note_box(60), 4);

        let mut c4_picks = 0usize;
        let mut other_picks = 0usize;
        for _ in 0..500 {
            let note = sched.pick_next();
            if note.midi == 60 { c4_picks += 1; } else { other_picks += 1; }
        }
        assert!(
            other_picks > c4_picks * 5,
            "D4/E4 (box 0) should dominate C4 (box 4): others={other_picks} c4={c4_picks}"
        );
    }
}
