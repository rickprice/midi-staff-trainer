use crate::scheduler::Scheduler;
use crate::staff::Note;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::time::Duration;

/// Snap a raw beat duration to the nearest standard note value.
fn quantize_beats(raw: f32) -> f32 {
    const VALUES: [f32; 5] = [4.0, 2.0, 1.0, 0.5, 0.25];
    VALUES
        .iter()
        .copied()
        .min_by(|&a, &b| (a - raw).abs().partial_cmp(&(b - raw).abs()).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(1.0)
}

const LOOKAHEAD: usize = 16;

// ── RandomSong ───────────────────────────────────────────────────────────────

/// A virtual "song" backed by the Leitner scheduler.
/// `size = None` → infinite session; `size = Some(n)` → stops after n notes.
pub struct RandomSong {
    pub active_low: u8,
    pub active_high: u8,
    scheduler: Scheduler,
    /// Current note plus up to LOOKAHEAD upcoming notes.
    queue: VecDeque<Note>,
    size: Option<usize>,
    pub index: usize,
    pub beats_per_measure: u8,
    pub beat_unit: u8,
}

impl RandomSong {
    pub fn new(low: u8, high: u8, size: Option<usize>) -> Self {
        let mut scheduler = Scheduler::new(low, high);
        let fill = size.map_or(LOOKAHEAD, |s| s.min(LOOKAHEAD));
        let queue = (0..fill).map(|_| scheduler.pick_next()).collect();
        Self { active_low: low, active_high: high, scheduler, queue, size, index: 0, beats_per_measure: 4, beat_unit: 4 }
    }

    pub fn advance(&mut self) {
        self.queue.pop_front();
        self.index += 1;
        // Refill unless we have already selected `size` total notes.
        let total_selected = self.index + self.queue.len();
        if self.size.is_none_or(|s| total_selected < s) {
            self.queue.push_back(self.scheduler.pick_next());
        }
    }

    pub fn is_complete(&self) -> bool {
        self.size.is_some() && self.queue.is_empty()
    }

    /// `Some((index, size))` when the session has a fixed length, `None` for infinite.
    pub fn progress(&self) -> Option<(usize, usize)> {
        self.size.map(|s| (self.index, s))
    }

    /// Up to `count` upcoming notes starting from the current position.
    pub fn peek(&self, count: usize) -> Vec<Note> {
        self.queue.iter().take(count).copied().collect()
    }

    pub fn in_range(&self, midi: u8) -> bool {
        (self.active_low..=self.active_high).contains(&midi)
    }

    pub fn record_correct(&mut self, midi: u8, latency: Duration) {
        self.scheduler.record_correct(midi, latency);
    }

    pub fn record_incorrect(&mut self, midi: u8) {
        self.scheduler.record_incorrect(midi);
    }

    pub fn box_counts(&self) -> [usize; 5] {
        self.scheduler.box_counts()
    }

    pub fn note_box(&self, midi: u8) -> u8 {
        self.scheduler.note_box(midi)
    }

    pub fn restart(&mut self) {
        self.index = 0;
        self.queue.clear();
        let fill = self.size.map_or(LOOKAHEAD, |s| s.min(LOOKAHEAD));
        for _ in 0..fill {
            self.queue.push_back(self.scheduler.pick_next());
        }
    }
}

// ── MidiFileSong ─────────────────────────────────────────────────────────────

pub struct MidiFileSong {
    pub filename: String,
    notes: Vec<(u8, f32)>, // (midi, beats)
    pub index: usize,
    pub beats_per_measure: u8,
    pub beat_unit: u8,
}

impl MidiFileSong {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("Cannot read file: {e}"))?;
        let smf = midly::Smf::parse(&data).map_err(|e| format!("Invalid MIDI file: {e}"))?;

        // Ticks-per-beat from header (used to convert tick durations to beats).
        let ticks_per_beat = match smf.header.timing {
            midly::Timing::Metrical(tpb) => f32::from(tpb.as_int()),
            midly::Timing::Timecode(_, _) => 480.0,
        };

        // Extract time signature from the first meta event that declares one.
        let mut beats_per_measure = 4u8;
        let mut beat_unit = 4u8;
        'ts: for track in &smf.tracks {
            for event in track {
                if let midly::TrackEventKind::Meta(
                    midly::MetaMessage::TimeSignature(num, denom, _, _)
                ) = event.kind {
                    beats_per_measure = num;
                    beat_unit = 1u8 << denom.min(7);
                    break 'ts;
                }
            }
        }

        // Extract notes with durations. For each NoteOn, find its matching NoteOff
        // (or NoteOn with vel=0) to compute the tick duration, then quantize to beats.
        let mut timed: Vec<(u64, u8, f32)> = Vec::new(); // (on_tick, midi, beats)
        for track in &smf.tracks {
            let mut tick: u64 = 0;
            let mut active: HashMap<u8, u64> = HashMap::new(); // key → on_tick
            for event in track {
                tick += u64::from(event.delta.as_int());
                match event.kind {
                    midly::TrackEventKind::Midi {
                        message: midly::MidiMessage::NoteOn { key, vel }, ..
                    } => {
                        let k = key.as_int();
                        if vel.as_int() > 0 {
                            active.insert(k, tick);
                        } else if let Some(on_tick) = active.remove(&k) {
                            #[allow(clippy::cast_precision_loss)]
                        let beats = quantize_beats((tick - on_tick) as f32 / ticks_per_beat);
                            timed.push((on_tick, k, beats));
                        }
                    }
                    midly::TrackEventKind::Midi {
                        message: midly::MidiMessage::NoteOff { key, .. }, ..
                    } => {
                        let k = key.as_int();
                        if let Some(on_tick) = active.remove(&k) {
                            #[allow(clippy::cast_precision_loss)]
                        let beats = quantize_beats((tick - on_tick) as f32 / ticks_per_beat);
                            timed.push((on_tick, k, beats));
                        }
                    }
                    _ => {}
                }
            }
            // Notes still active at end of track (no NoteOff) get a quarter-note duration.
            for (k, on_tick) in active {
                timed.push((on_tick, k, 1.0));
            }
        }

        if timed.is_empty() {
            return Err("No notes found in MIDI file".to_string());
        }

        timed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let notes: Vec<(u8, f32)> = timed.into_iter().map(|(_, midi, beats)| (midi, beats)).collect();

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.mid")
            .to_string();

        Ok(Self { filename, notes, index: 0, beats_per_measure, beat_unit })
    }

    pub fn advance(&mut self) {
        if self.index < self.notes.len() {
            self.index += 1;
        }
    }

    pub fn is_complete(&self) -> bool {
        self.index >= self.notes.len()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.index, self.notes.len())
    }

    /// Up to `count` upcoming notes starting from the current position.
    pub fn peek(&self, count: usize) -> Vec<Note> {
        self.notes[self.index..]
            .iter()
            .take(count)
            .map(|&(midi, beats)| Note::with_beats(midi, beats))
            .collect()
    }

    pub fn restart(&mut self) {
        self.index = 0;
    }
}

// ── Song (unified interface) ─────────────────────────────────────────────────

/// A sequence of notes to practise — either a real MIDI file or a virtual
/// random session.  Both variants share the same interface so the rest of the
/// app never needs to branch on which kind it is for note-delivery logic.
pub enum Song {
    Random(RandomSong),
    MidiFile(MidiFileSong),
}

impl Song {
    pub fn advance(&mut self) {
        match self {
            Song::Random(r) => r.advance(),
            Song::MidiFile(f) => f.advance(),
        }
    }

    pub fn is_complete(&self) -> bool {
        match self {
            Song::Random(r) => r.is_complete(),
            Song::MidiFile(f) => f.is_complete(),
        }
    }

    /// `Some((index, total))` for a finite song/session, `None` for an
    /// infinite random session.
    pub fn progress(&self) -> Option<(usize, usize)> {
        match self {
            Song::Random(r) => r.progress(),
            Song::MidiFile(f) => Some(f.progress()),
        }
    }

    /// Up to `count` upcoming notes from the current position.
    pub fn peek(&self, count: usize) -> Vec<Note> {
        match self {
            Song::Random(r) => r.peek(count),
            Song::MidiFile(f) => f.peek(count),
        }
    }

    /// Whether `midi` falls within the accepted input range for this song.
    /// Always `true` for a MIDI-file song (no range restriction).
    pub fn in_range(&self, midi: u8) -> bool {
        match self {
            Song::Random(r) => r.in_range(midi),
            Song::MidiFile(_) => true,
        }
    }

    pub fn record_correct(&mut self, midi: u8, latency: Duration) {
        if let Song::Random(r) = self { r.record_correct(midi, latency); }
    }

    pub fn record_incorrect(&mut self, midi: u8) {
        if let Song::Random(r) = self { r.record_incorrect(midi); }
    }

    pub fn restart(&mut self) {
        match self {
            Song::Random(r) => r.restart(),
            Song::MidiFile(f) => f.restart(),
        }
    }

    pub fn as_random(&self) -> Option<&RandomSong> {
        if let Song::Random(r) = self { Some(r) } else { None }
    }

    pub fn as_midi_file(&self) -> Option<&MidiFileSong> {
        if let Song::MidiFile(f) = self { Some(f) } else { None }
    }

    pub fn beats_per_measure(&self) -> u8 {
        match self {
            Song::Random(r) => r.beats_per_measure,
            Song::MidiFile(f) => f.beats_per_measure,
        }
    }

    pub fn beat_unit(&self) -> u8 {
        match self {
            Song::Random(r) => r.beat_unit,
            Song::MidiFile(f) => f.beat_unit,
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    // Test module is a child of song.rs so it can access private struct fields directly.
    fn make_midi_song(notes: Vec<u8>) -> MidiFileSong {
        let notes = notes.into_iter().map(|m| (m, 1.0_f32)).collect();
        MidiFileSong { filename: "test.mid".to_string(), notes, index: 0, beats_per_measure: 4, beat_unit: 4 }
    }

    // ── RandomSong ───────────────────────────────────────────────────────────────

    #[test]
    fn random_song_peek_count_respected() {
        let song = RandomSong::new(60, 72, None);
        assert_eq!(song.peek(3).len(), 3);
    }

    #[test]
    fn random_song_peek_at_most_queue_size() {
        let song = RandomSong::new(60, 72, None);
        assert!(song.peek(1000).len() <= LOOKAHEAD);
    }

    #[test]
    fn random_song_peek_notes_within_range() {
        let (lo, hi) = (60u8, 72u8);
        let song = RandomSong::new(lo, hi, None);
        for note in song.peek(LOOKAHEAD) {
            assert!(note.midi >= lo && note.midi <= hi);
        }
    }

    #[test]
    fn random_song_advance_increments_index() {
        let mut song = RandomSong::new(60, 72, None);
        assert_eq!(song.index, 0);
        song.advance();
        assert_eq!(song.index, 1);
        song.advance();
        assert_eq!(song.index, 2);
    }

    #[test]
    fn random_song_advance_shifts_peek() {
        let mut song = RandomSong::new(60, 72, None);
        let before = song.peek(2);
        song.advance();
        let after = song.peek(1);
        // First note before advance equals first note after, shifted by one.
        assert_eq!(before[1].midi, after[0].midi);
    }

    #[test]
    fn random_song_is_complete_infinite_never() {
        let mut song = RandomSong::new(60, 72, None);
        for _ in 0..50 {
            assert!(!song.is_complete());
            song.advance();
        }
    }

    #[test]
    fn random_song_is_complete_sized_after_exhaustion() {
        let mut song = RandomSong::new(60, 72, Some(4));
        for _ in 0..4 {
            assert!(!song.is_complete());
            song.advance();
        }
        assert!(song.is_complete());
    }

    #[test]
    fn random_song_sized_peek_limited_by_size() {
        let song = RandomSong::new(60, 72, Some(3));
        assert_eq!(song.peek(100).len(), 3);
    }

    #[test]
    fn random_song_progress_none_for_infinite() {
        let song = RandomSong::new(60, 72, None);
        assert!(song.progress().is_none());
    }

    #[test]
    fn random_song_progress_some_for_sized() {
        let mut song = RandomSong::new(60, 72, Some(5));
        assert_eq!(song.progress(), Some((0, 5)));
        song.advance();
        assert_eq!(song.progress(), Some((1, 5)));
    }

    #[test]
    fn random_song_in_range_accepts_endpoints() {
        let song = RandomSong::new(60, 72, None);
        assert!(song.in_range(60));
        assert!(song.in_range(72));
    }

    #[test]
    fn random_song_in_range_rejects_outside() {
        let song = RandomSong::new(60, 72, None);
        assert!(!song.in_range(59));
        assert!(!song.in_range(73));
    }

    #[test]
    fn random_song_restart_resets_index() {
        let mut song = RandomSong::new(60, 72, None);
        for _ in 0..10 {
            song.advance();
        }
        song.restart();
        assert_eq!(song.index, 0);
    }

    #[test]
    fn random_song_restart_refills_queue() {
        let mut song = RandomSong::new(60, 72, None);
        for _ in 0..10 {
            song.advance();
        }
        song.restart();
        assert_eq!(song.peek(LOOKAHEAD).len(), LOOKAHEAD);
    }

    #[test]
    fn random_song_sized_restart_allows_replay() {
        let mut song = RandomSong::new(60, 72, Some(3));
        for _ in 0..3 {
            song.advance();
        }
        assert!(song.is_complete());
        song.restart();
        assert!(!song.is_complete());
        assert_eq!(song.peek(3).len(), 3);
    }

    // ── MidiFileSong ─────────────────────────────────────────────────────────────

    #[test]
    fn midi_file_song_peek_returns_all_notes_from_start() {
        let song = make_midi_song(vec![60, 62, 64]);
        let midis: Vec<u8> = song.peek(3).iter().map(|n| n.midi).collect();
        assert_eq!(midis, vec![60, 62, 64]);
    }

    #[test]
    fn midi_file_song_peek_count_respected() {
        let song = make_midi_song(vec![60, 62, 64, 65]);
        assert_eq!(song.peek(2).len(), 2);
    }

    #[test]
    fn midi_file_song_peek_beyond_length_is_safe() {
        let song = make_midi_song(vec![60]);
        assert_eq!(song.peek(100).len(), 1);
    }

    #[test]
    fn midi_file_song_advance_shifts_peek() {
        let mut song = make_midi_song(vec![60, 62, 64]);
        song.advance();
        let midis: Vec<u8> = song.peek(3).iter().map(|n| n.midi).collect();
        assert_eq!(midis, vec![62, 64]);
    }

    #[test]
    fn midi_file_song_advance_increments_progress() {
        let mut song = make_midi_song(vec![60, 62, 64]);
        assert_eq!(song.progress(), (0, 3));
        song.advance();
        assert_eq!(song.progress(), (1, 3));
        song.advance();
        assert_eq!(song.progress(), (2, 3));
    }

    #[test]
    fn midi_file_song_is_complete_false_when_notes_remain() {
        let song = make_midi_song(vec![60, 62]);
        assert!(!song.is_complete());
    }

    #[test]
    fn midi_file_song_is_complete_after_all_advanced() {
        let mut song = make_midi_song(vec![60, 62]);
        song.advance();
        assert!(!song.is_complete());
        song.advance();
        assert!(song.is_complete());
    }

    #[test]
    fn midi_file_song_advance_past_end_is_safe() {
        let mut song = make_midi_song(vec![60]);
        song.advance();
        song.advance(); // should not panic
        assert!(song.is_complete());
    }

    #[test]
    fn midi_file_song_peek_empty_when_complete() {
        let mut song = make_midi_song(vec![60]);
        song.advance();
        assert!(song.peek(5).is_empty());
    }

    #[test]
    fn midi_file_song_restart_resets_to_start() {
        let mut song = make_midi_song(vec![60, 62, 64]);
        song.advance();
        song.advance();
        song.restart();
        assert_eq!(song.progress(), (0, 3));
        assert!(!song.is_complete());
        assert_eq!(song.peek(1)[0].midi, 60);
    }

    // ── Song (unified dispatch) ───────────────────────────────────────────────────

    #[test]
    fn song_midi_file_in_range_always_true() {
        let song = Song::MidiFile(make_midi_song(vec![60]));
        assert!(song.in_range(0));
        assert!(song.in_range(60));
        assert!(song.in_range(127));
    }

    #[test]
    fn song_random_progress_none_for_infinite() {
        let song = Song::Random(RandomSong::new(60, 72, None));
        assert!(song.progress().is_none());
    }

    #[test]
    fn song_midi_file_progress_some() {
        let song = Song::MidiFile(make_midi_song(vec![60, 62]));
        assert_eq!(song.progress(), Some((0, 2)));
    }

    #[test]
    fn song_peek_and_advance_consistent() {
        let mut song = Song::MidiFile(make_midi_song(vec![60, 62, 64]));
        assert_eq!(song.peek(1)[0].midi, 60);
        song.advance();
        assert_eq!(song.peek(1)[0].midi, 62);
        song.advance();
        assert_eq!(song.peek(1)[0].midi, 64);
    }

    #[test]
    fn song_is_complete_random_infinite_never() {
        let song = Song::Random(RandomSong::new(60, 72, None));
        assert!(!song.is_complete());
    }

    #[test]
    fn song_is_complete_midi_file_after_exhaustion() {
        let mut song = Song::MidiFile(make_midi_song(vec![60]));
        assert!(!song.is_complete());
        song.advance();
        assert!(song.is_complete());
    }

    #[test]
    fn song_restart_allows_replay() {
        let mut song = Song::MidiFile(make_midi_song(vec![60, 62]));
        song.advance();
        song.advance();
        assert!(song.is_complete());
        song.restart();
        assert!(!song.is_complete());
        assert_eq!(song.peek(1)[0].midi, 60);
    }

    #[test]
    fn song_page_flip_scenario() {
        // Simulates the paging logic in TrainerApp: play through a page,
        // then peek the next page and verify continuity.
        let mut song = Song::MidiFile(make_midi_song(vec![60, 62, 64, 65, 67]));
        let page1: Vec<u8> = song.peek(3).iter().map(|n| n.midi).collect();
        assert_eq!(page1, vec![60, 62, 64]);
        // Play through the page.
        song.advance();
        song.advance();
        song.advance();
        // Next peek should start from where we left off.
        let page2: Vec<u8> = song.peek(3).iter().map(|n| n.midi).collect();
        assert_eq!(page2, vec![65, 67]);
    }

    #[test]
    fn song_as_random_returns_some_for_random() {
        let song = Song::Random(RandomSong::new(60, 72, None));
        assert!(song.as_random().is_some());
        assert!(song.as_midi_file().is_none());
    }

    #[test]
    fn song_as_midi_file_returns_some_for_midi_file() {
        let song = Song::MidiFile(make_midi_song(vec![60]));
        assert!(song.as_midi_file().is_some());
        assert!(song.as_random().is_none());
    }
}
