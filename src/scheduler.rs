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
        if candidates.len() > 1 {
            if let Some(last) = self.last_note {
                candidates.retain(|&m| m != last);
            }
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
        if let Some(state) = self.states.get_mut(&midi) {
            if latency.as_millis() <= FAST_THRESHOLD_MS as u128 {
                state.box_level = (state.box_level + 1).min(4);
            }
            // slow correct → stay in same box (no change needed)
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
