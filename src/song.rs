use crate::scheduler::Scheduler;
use crate::staff::Note;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

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
}

impl RandomSong {
    pub fn new(low: u8, high: u8, size: Option<usize>) -> Self {
        let mut scheduler = Scheduler::new(low, high);
        let fill = size.map_or(LOOKAHEAD, |s| s.min(LOOKAHEAD));
        let queue = (0..fill).map(|_| scheduler.pick_next()).collect();
        Self { active_low: low, active_high: high, scheduler, queue, size, index: 0 }
    }

    pub fn current(&self) -> Option<Note> {
        self.queue.front().copied()
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
    #[allow(dead_code)]
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
    notes: Vec<u8>,
    pub index: usize,
}

impl MidiFileSong {
    pub fn load(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path).map_err(|e| format!("Cannot read file: {e}"))?;
        let smf = midly::Smf::parse(&data).map_err(|e| format!("Invalid MIDI file: {e}"))?;

        let mut timed: Vec<(u64, u8)> = Vec::new();
        for track in &smf.tracks {
            let mut tick: u64 = 0;
            for event in track {
                tick += u64::from(event.delta.as_int());
                if let midly::TrackEventKind::Midi {
                    message: midly::MidiMessage::NoteOn { key, vel },
                    ..
                } = event.kind
                    && vel.as_int() > 0
                {
                    timed.push((tick, key.as_int()));
                }
            }
        }

        if timed.is_empty() {
            return Err("No notes found in MIDI file".to_string());
        }

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

    pub fn is_complete(&self) -> bool {
        self.index >= self.notes.len()
    }

    pub fn progress(&self) -> (usize, usize) {
        (self.index, self.notes.len())
    }

    /// Up to `count` upcoming notes starting from the current position.
    #[allow(dead_code)]
    pub fn peek(&self, count: usize) -> Vec<Note> {
        self.notes[self.index..]
            .iter()
            .take(count)
            .map(|&n| Note::new(n))
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
    pub fn current(&self) -> Option<Note> {
        match self {
            Song::Random(r) => r.current(),
            Song::MidiFile(f) => f.current(),
        }
    }

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
    /// Used today to render the single current note; will drive multi-note
    /// look-ahead display when the staff renderer is extended.
    #[allow(dead_code)]
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
}
