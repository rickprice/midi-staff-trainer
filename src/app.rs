use crate::{
    config::Config,
    midi::MidiReceiver,
    scheduler::{BOX_WEIGHTS, FAST_THRESHOLD_MS},
    song::{MidiFileSong, RandomSong, Song},
    staff::Note,
    state::AppState,
};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke};
use std::cmp::Ordering;
use std::time::{Duration, Instant};

const CORRECT_DISPLAY_MS: u64 = 900;

enum AppMode {
    Playing,
    SettingRange(Vec<u8>),
}

pub struct TrainerApp {
    config: Config,
    app_state: AppState,
    midi: Option<MidiReceiver>,
    midi_port_name: Option<String>,
    midi_error: Option<String>,
    current_note: Note,
    feedback: Feedback,
    correct_at: Option<Instant>,
    note_shown_at: Option<Instant>,
    score: Score,
    song: Song,
    mode: AppMode,
    page: Vec<Note>,
    page_cursor: usize,
    page_size: usize,
    beat_cursor: f32,
    last_key_result: Option<(u8, bool, Instant)>,
    show_keyboard: bool,
}

#[derive(Default)]
struct Score {
    correct: u32,
    attempts: u32,
}

impl Score {
    fn accuracy_pct(&self) -> Option<u32> {
        (self.correct * 100).checked_div(self.attempts)
    }
}

enum Feedback {
    Waiting,
    Correct(Note),
    Wrong { expected: Note, got: Note },
    OutOfRange(Note),
}

impl TrainerApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "NotoMusic".to_owned(),
            FontData::from_static(include_bytes!("../fonts/NotoMusic-Regular.otf")).into(),
        );
        fonts
            .families
            .entry(FontFamily::Name("NotoMusic".into()))
            .or_default()
            .push("NotoMusic".to_owned());
        cc.egui_ctx.set_fonts(fonts);

        let config = Config::load();
        let app_state = AppState::load();
        let (midi, midi_port_name, midi_error) = match MidiReceiver::connect(config.midi_port.as_deref(), cc.egui_ctx.clone()) {
            Ok(r) => {
                let name = r.port_name.clone();
                (Some(r), Some(name), None)
            }
            Err(e) => (None, None, Some(e)),
        };
        let active_low = app_state.training_low.unwrap_or(config.midi_low);
        let active_high = app_state.training_high.unwrap_or(config.midi_high);
        let song = Song::Random(RandomSong::new(active_low, active_high, None));
        let page_size = 8;
        let page = song.peek(page_size);
        let current_note = page.first().copied().unwrap_or(Note::new(60));
        Self {
            config,
            app_state,
            midi,
            midi_port_name,
            midi_error,
            current_note,
            feedback: Feedback::Waiting,
            correct_at: None,
            note_shown_at: Some(Instant::now()),
            score: Score::default(),
            song,
            mode: AppMode::Playing,
            page,
            page_cursor: 0,
            page_size,
            beat_cursor: 0.0,
            last_key_result: None,
            show_keyboard: true,
        }
    }

    fn advance_to_next(&mut self) {
        self.beat_cursor += self.page.get(self.page_cursor).map_or(1.0, |n| n.beats);
        self.song.advance();
        self.page_cursor += 1;
        if self.page_cursor >= self.page.len() {
            self.page = self.song.peek(self.page_size.max(1));
            self.page_cursor = 0;
        }
        if let Some(note) = self.page.get(self.page_cursor).copied() {
            self.current_note = note;
        }
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
        self.last_key_result = None;
    }

    fn reset_page(&mut self) {
        self.page = self.song.peek(self.page_size.max(1));
        self.page_cursor = 0;
        self.beat_cursor = 0.0;
        self.current_note = self.page.first().copied().unwrap_or(Note::new(60));
        self.last_key_result = None;
    }

    fn restart_song(&mut self) {
        self.song.restart();
        self.reset_page();
        self.score = Score::default();
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
        self.last_key_result = None;
    }

    fn handle_midi_note(&mut self, played: u8) {
        // If the previous note was correct and the display delay is still running,
        // advance immediately so the player doesn't have to wait for the animation.
        if matches!(self.feedback, Feedback::Correct(_)) {
            self.advance_to_next();
        }
        self.score.attempts += 1;
        if played == self.current_note.midi {
            let latency = self.note_shown_at
                .map_or(Duration::from_secs(99), |t| t.elapsed());
            self.song.record_correct(played, latency);
            self.score.correct += 1;
            self.feedback = Feedback::Correct(self.current_note);
            self.correct_at = Some(Instant::now());
            self.last_key_result = Some((played, true, Instant::now()));
        } else {
            self.song.record_incorrect(self.current_note.midi);
            self.feedback = Feedback::Wrong {
                expected: self.current_note,
                got: Note::new(played),
            };
            self.last_key_result = Some((played, false, Instant::now()));
        }
    }

    fn handle_range_key(&mut self, midi: u8) {
        if let AppMode::SettingRange(ref mut keys) = self.mode
            && !keys.contains(&midi) {
            keys.push(midi);
        }
        let result = if let AppMode::SettingRange(ref keys) = self.mode {
            (keys.len() >= 2).then(|| {
                (*keys.iter().min().unwrap(), *keys.iter().max().unwrap())
            })
        } else {
            None
        };
        if let Some((low, high)) = result {
            self.set_active_range(low, high);
        }
    }

    fn set_active_range(&mut self, low: u8, high: u8) {
        self.app_state.training_low = Some(low);
        self.app_state.training_high = Some(high);
        self.app_state.save();
        self.song = Song::Random(RandomSong::new(low, high, None));
        self.reset_page();
        self.mode = AppMode::Playing;
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
    }

    fn reset_range(&mut self) {
        let (low, high) = (self.config.midi_low, self.config.midi_high);
        self.set_active_range(low, high);
    }

    fn exit_to_random(&mut self) {
        let low = self.app_state.training_low.unwrap_or(self.config.midi_low);
        let high = self.app_state.training_high.unwrap_or(self.config.midi_high);
        self.song = Song::Random(RandomSong::new(low, high, None));
        self.reset_page();
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
    }

    fn load_midi_file(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("MIDI files", &["mid", "midi"])
            .pick_file();
        if let Some(path) = picked {
            match MidiFileSong::load(&path) {
                Ok(f) => {
                    self.song = Song::MidiFile(f);
                    self.reset_page();
                    self.score = Score::default();
                    self.feedback = Feedback::Waiting;
                    self.correct_at = None;
                    self.note_shown_at = Some(Instant::now());
                    self.mode = AppMode::Playing;
                    self.midi_error = None;
                }
                Err(e) => self.midi_error = Some(e),
            }
        }
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::too_many_lines)]
    fn draw_staff(&self, painter: &Painter, rect: Rect) -> usize {
        let cx = rect.center().x;
        let cy = rect.center().y;
        let line_spacing = (rect.height() * 0.075).clamp(12.0, 28.0);
        let staff_width = rect.width() * 0.85;
        let x0 = cx - staff_width / 2.0;
        let x1 = cx + staff_width / 2.0;

        let bottom_line_y = cy + 2.0 * line_spacing;
        let top_line_y = cy - 2.0 * line_spacing;

        let staff_color = Color32::from_gray(220);
        let staff_stroke = Stroke::new(1.5_f32, staff_color);
        for i in 0..5_i32 {
            let y = cy + (2 - i) as f32 * line_spacing;
            painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], staff_stroke);
        }

        draw_treble_clef(painter, x0, cy, line_spacing, staff_color);

        // Time signature — drawn after the clef, before the first note.
        let beats = self.song.beats_per_measure();
        let unit = self.song.beat_unit();
        let ts_x = x0 + line_spacing * 3.4;
        let ts_font = FontId::proportional(line_spacing * 1.9);
        painter.text(Pos2::new(ts_x, cy - line_spacing * 0.85), egui::Align2::CENTER_CENTER, beats.to_string(), ts_font.clone(), staff_color);
        painter.text(Pos2::new(ts_x, cy + line_spacing * 0.85), egui::Align2::CENTER_CENTER, unit.to_string(), ts_font, staff_color);

        // Compute how many notes fit after the treble clef + time signature.
        let clef_width = line_spacing * 5.5;
        let note_spacing = line_spacing * 3.0;
        let notes_area = staff_width - clef_width;
        let page_size = ((notes_area / note_spacing).floor() as usize).max(1);
        let notes_start_x = x0 + clef_width + note_spacing * 0.5;

        // Bar lines: left edge, after clef/time-sig, right edge.
        for bar_x in [x0, x0 + clef_width, x1] {
            painter.line_segment(
                [Pos2::new(bar_x, top_line_y), Pos2::new(bar_x, bottom_line_y)],
                staff_stroke,
            );
        }

        // Measure bar lines: placed by cumulative beats rather than note count.
        let played_page_beats: f32 = self.page[..self.page_cursor].iter().map(|n| n.beats).sum();
        let page_start_beat = self.beat_cursor - played_page_beats;
        let beats_f = f32::from(beats);
        let visible = self.page.len().min(page_size);
        let mut running_beats = page_start_beat;
        for i in 0..visible {
            if i > 0 {
                let curr_measure = (running_beats / beats_f).floor() as i32;
                let prev_measure = ((running_beats - self.page[i - 1].beats) / beats_f).floor() as i32;
                if curr_measure > prev_measure {
                    let bar_x = notes_start_x + (i as f32 - 0.5) * note_spacing;
                    painter.line_segment(
                        [Pos2::new(bar_x, top_line_y), Pos2::new(bar_x, bottom_line_y)],
                        staff_stroke,
                    );
                }
            }
            running_beats += self.page[i].beats;
        }

        let note_r = line_spacing * 0.45;
        let ledger_hw = note_r * 2.2;
        let ledger_stroke = Stroke::new(1.5_f32, staff_color);

        for (i, note) in self.page.iter().take(page_size).enumerate() {
            let note_x = notes_start_x + i as f32 * note_spacing;
            let pos = note.staff_position();
            let note_y = bottom_line_y - (pos - 2) as f32 * (line_spacing / 2.0);

            let note_color = match i.cmp(&self.page_cursor) {
                Ordering::Less => Color32::from_gray(80),
                Ordering::Equal => match self.feedback {
                    Feedback::Correct(_) => Color32::from_rgb(50, 180, 80),
                    Feedback::Wrong { .. } => Color32::from_rgb(210, 60, 60),
                    Feedback::Waiting | Feedback::OutOfRange(_) => Color32::from_rgb(212, 175, 55),
                },
                Ordering::Greater => Color32::from_gray(155),
            };

            // Ledger lines below staff
            let mut ly = bottom_line_y + line_spacing;
            while ly <= note_y + 0.5 {
                painter.line_segment(
                    [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                    ledger_stroke,
                );
                ly += line_spacing;
            }
            // Ledger lines above staff
            let mut ly = top_line_y - line_spacing;
            while ly >= note_y - 0.5 {
                painter.line_segment(
                    [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                    ledger_stroke,
                );
                ly -= line_spacing;
            }

            if note.is_accidental() {
                painter.text(
                    Pos2::new(note_x - note_r * 3.5, note_y),
                    egui::Align2::CENTER_CENTER,
                    "♯",
                    FontId::proportional(line_spacing * 1.3),
                    note_color,
                );
            }

            // Note head — open (hollow) for whole/half notes, filled for quarter and shorter.
            let open_head = note.beats >= 2.0;
            if open_head {
                let r = if note.beats >= 4.0 { note_r * 1.15 } else { note_r };
                painter.circle_stroke(Pos2::new(note_x, note_y), r, Stroke::new(2.0_f32, note_color));
            } else {
                painter.circle_filled(Pos2::new(note_x, note_y), note_r, note_color);
            }

            // Stem — whole notes have none.
            if note.beats < 4.0 {
                let stem_up = pos <= 6;
                let (stem_x, stem_y0, stem_y1) = if stem_up {
                    (note_x + note_r, note_y, note_y - line_spacing * 3.5)
                } else {
                    (note_x - note_r, note_y, note_y + line_spacing * 3.5)
                };
                painter.line_segment(
                    [Pos2::new(stem_x, stem_y0), Pos2::new(stem_x, stem_y1)],
                    Stroke::new(1.5_f32, note_color),
                );

                // Flags — eighth note gets one, sixteenth gets two.
                if note.beats <= 0.5 {
                    let num_flags: i32 = if note.beats <= 0.25 { 2 } else { 1 };
                    let flag_stroke = Stroke::new(1.5_f32, note_color);
                    for f in 0..num_flags {
                        let (fy0, fy1) = if stem_up {
                            let base = stem_y1 + f as f32 * line_spacing * 0.7;
                            (base, base + line_spacing * 1.4)
                        } else {
                            let base = stem_y1 - f as f32 * line_spacing * 0.7;
                            (base, base - line_spacing * 1.4)
                        };
                        painter.line_segment(
                            [Pos2::new(stem_x, fy0), Pos2::new(stem_x + note_r * 3.0, fy1)],
                            flag_stroke,
                        );
                    }
                }
            }
        }

        page_size
    }

    #[allow(clippy::cast_precision_loss)]
    fn draw_keyboard(&self, painter: &Painter, rect: Rect) {
        const MIDI_MIN: u8 = 21;
        const MIDI_MAX: u8 = 108;
        const WHITE_KEYS: f32 = 52.0;

        let white_width = rect.width() / WHITE_KEYS;
        let black_width = white_width * 0.6;

        // Reserve a small strip at the top for range marker triangles.
        let tri_strip = 14.0_f32;
        let key_top = rect.top() + tri_strip;
        let key_height = rect.height() - tri_strip;
        let black_height = key_height * 0.62;

        // Dark surround so white keys are visible in both light and dark themes.
        painter.rect_filled(rect, 0.0, Color32::from_gray(40));

        // Training range — None means all keys shown equally (MIDI file mode).
        let range: Option<(u8, u8)> = self.song.as_random().map(|r| (r.active_low, r.active_high));
        let flash: Option<(u8, bool)> = self.last_key_result.and_then(|(m, ok, when)| {
            (when.elapsed() < Duration::from_millis(500)).then_some((m, ok))
        });
        let expected = self.current_note.midi;

        // White keys.
        for midi in MIDI_MIN..=MIDI_MAX {
            if piano_is_black(midi) { continue; }
            let x = rect.left() + piano_white_idx(midi) as f32 * white_width;
            let key_rect = Rect::from_min_max(
                Pos2::new(x, key_top),
                Pos2::new(x + white_width - 1.0, key_top + key_height),
            );
            let in_range = range.is_none_or(|(lo, hi)| midi >= lo && midi <= hi);
            let base = if in_range { Color32::from_gray(245) } else { Color32::from_gray(160) };
            painter.rect_filled(key_rect, 0.0, piano_key_color(midi, expected, flash, base));
            painter.rect_stroke(key_rect, 0.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Outside);
        }

        // Black keys (drawn on top).
        for midi in MIDI_MIN..=MIDI_MAX {
            if !piano_is_black(midi) { continue; }
            let x_center = rect.left() + piano_key_center_x(midi, white_width);
            let key_rect = Rect::from_min_max(
                Pos2::new(x_center - black_width * 0.5, key_top),
                Pos2::new(x_center + black_width * 0.5, key_top + black_height),
            );
            let in_range = range.is_none_or(|(lo, hi)| midi >= lo && midi <= hi);
            let base = if in_range { Color32::from_gray(20) } else { Color32::from_gray(70) };
            painter.rect_filled(key_rect, 2.0, piano_key_color(midi, expected, flash, base));
        }

        // Downward-pointing triangles above boundary keys.
        // In random mode: show the training range boundaries.
        // In MIDI file mode: show the physical keyboard range from config.
        let (tri_lo, tri_hi) = range.unwrap_or((self.config.midi_low, self.config.midi_high));
        let tri_color = Color32::from_gray(180);
        let tri_h = tri_strip * 0.75;
        let tri_w = tri_h * 0.9;
        for &boundary in &[tri_lo, tri_hi] {
            let cx = rect.left() + piano_key_center_x(boundary, white_width);
            let y_tip = key_top;
            painter.add(Shape::convex_polygon(
                vec![
                    Pos2::new(cx - tri_w, y_tip - tri_h),
                    Pos2::new(cx + tri_w, y_tip - tri_h),
                    Pos2::new(cx, y_tip),
                ],
                tri_color,
                Stroke::NONE,
            ));
        }
    }
}

fn piano_is_black(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

/// White key index (0 = A0 = MIDI 21) for a MIDI note.
/// Returns the number of white keys in [21, midi).
/// Uses the fixed semitone-offset pattern starting from A, repeated every octave.
const fn piano_white_idx(midi: u8) -> usize {
    // White keys before each semitone offset within one octave starting at A (pc=9):
    // A=0,A#=1,B=1,C=2,C#=3,D=3,D#=4,E=4,F=5,F#=6,G=6,G#=7
    const WHITES_BEFORE: [usize; 12] = [0, 1, 1, 2, 3, 3, 4, 4, 5, 6, 6, 7];
    let n = (midi - 21) as usize;
    (n / 12) * 7 + WHITES_BEFORE[n % 12]
}

/// X center of a key (white or black) relative to the keyboard left edge, in pixels.
#[allow(clippy::cast_precision_loss)]
fn piano_key_center_x(midi: u8, white_width: f32) -> f32 {
    if piano_is_black(midi) {
        // Center of black key = right edge of the white key immediately below
        (piano_white_idx(midi - 1) + 1) as f32 * white_width
    } else {
        (piano_white_idx(midi) as f32 + 0.5) * white_width
    }
}

fn piano_key_color(midi: u8, expected: u8, flash: Option<(u8, bool)>, base: Color32) -> Color32 {
    if let Some((flash_midi, correct)) = flash
        && flash_midi == midi {
        return if correct { Color32::from_rgb(50, 180, 80) } else { Color32::from_rgb(210, 60, 60) };
    }
    if midi == expected { Color32::from_rgb(212, 175, 55) } else { base }
}

fn draw_treble_clef(painter: &Painter, x0: f32, cy: f32, s: f32, color: Color32) {
    let font_size = s * 4.5;
    let pos = Pos2::new(x0 + s * 0.15, cy - s * 0.1);
    painter.text(
        pos,
        egui::Align2::LEFT_CENTER,
        "\u{1D11E}",
        FontId::new(font_size, FontFamily::Name("NotoMusic".into())),
        color,
    );
}

impl eframe::App for TrainerApp {
    #[allow(clippy::too_many_lines)] // egui UI methods are inherently long
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let midi_notes: Vec<u8> = self
            .midi
            .as_ref()
            .map(|m| std::iter::from_fn(|| m.rx.try_recv().ok()).collect())
            .unwrap_or_default();

        let is_setting_range = matches!(self.mode, AppMode::SettingRange(_));

        for note in midi_notes {
            if is_setting_range {
                self.handle_range_key(note);
            } else if self.song.in_range(note) {
                self.handle_midi_note(note);
            } else {
                self.feedback = Feedback::OutOfRange(Note::new(note));
            }
        }

        let (r_pressed, ctrl_r_pressed, k_pressed, esc_pressed) = ctx.input(|i| (
            i.key_pressed(egui::Key::R) && !i.modifiers.ctrl,
            i.key_pressed(egui::Key::R) && i.modifiers.ctrl,
            i.key_pressed(egui::Key::K),
            i.key_pressed(egui::Key::Escape),
        ));
        if ctrl_r_pressed && matches!(self.mode, AppMode::Playing) {
            self.restart_song();
        }
        if k_pressed {
            self.show_keyboard = !self.show_keyboard;
        }
        if r_pressed && matches!(self.mode, AppMode::Playing) && self.song.as_random().is_some() {
            self.mode = AppMode::SettingRange(Vec::new());
            self.correct_at = None;
            self.feedback = Feedback::Waiting;
        }
        if esc_pressed {
            match self.mode {
                AppMode::SettingRange(_) => self.mode = AppMode::Playing,
                AppMode::Playing if matches!(self.song, Song::MidiFile(_)) => self.exit_to_random(),
                AppMode::Playing => {}
            }
        }

        if let Some(t) = self.correct_at {
            let elapsed = t.elapsed();
            let delay = Duration::from_millis(CORRECT_DISPLAY_MS);
            if elapsed >= delay {
                self.advance_to_next();
            } else {
                ctx.request_repaint_after(delay.saturating_sub(elapsed));
            }
        }

        // Only schedule a flash repaint on the wrong-note path; on the correct path
        // the correct_at timer (900ms) already ensures a repaint before 500ms expires.
        if self.correct_at.is_none()
            && let Some((_, _, when)) = self.last_key_result {
            let flash_dur = Duration::from_millis(500);
            let elapsed = when.elapsed();
            if elapsed < flash_dur {
                ctx.request_repaint_after(flash_dur.saturating_sub(elapsed));
            }
        }

        // Snapshot fields for UI drawing (avoids re-borrows inside closures).
        let accuracy_str = self.score.accuracy_pct()
            .map(|p| format!(" ({p}%)"))
            .unwrap_or_default();
        let mode_state: Option<(usize, Option<u8>)> = match &self.mode {
            AppMode::SettingRange(keys) => Some((keys.len(), keys.first().copied())),
            AppMode::Playing => None,
        };
        let current_box = self.song.as_random()
            .map_or(0, |r| r.note_box(self.current_note.midi));
        let box_display: Option<String> = self.song.as_random().map(|r| {
            let counts = r.box_counts();
            let box_str = counts.iter().enumerate()
                .map(|(i, &n)| format!("{i}:{n}"))
                .collect::<Vec<_>>()
                .join("  ");
            format!(
                "Boxes (weight {:.0}/{:.0}/{:.0}/{:.1}/{:.1}) → {box_str}",
                BOX_WEIGHTS[0], BOX_WEIGHTS[1], BOX_WEIGHTS[2],
                BOX_WEIGHTS[3], BOX_WEIGHTS[4],
            )
        });
        let range_info: Option<(String, bool)> = self.song.as_random().map(|r| {
            let label = format!("Range: {} – {}", Note::new(r.active_low), Note::new(r.active_high));
            let changed = r.active_low != self.config.midi_low
                || r.active_high != self.config.midi_high;
            (label, changed)
        });
        let midi_file_info: Option<(String, usize, usize, bool)> = self.song.as_midi_file().map(|f| {
            let (idx, total) = f.progress();
            (f.filename.clone(), idx, total, f.is_complete())
        });
        let song_is_complete = self.song.is_complete();

        egui::Panel::top("header").show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading("MIDI Staff Trainer");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if let Some((ref filename, idx, total, _)) = midi_file_info {
                        ui.label(format!(
                            "Score: {}/{}{} | Song: {} | Note {}/{}",
                            self.score.correct, self.score.attempts, accuracy_str,
                            filename, idx, total,
                        ));
                    } else {
                        let range_str = range_info.as_ref()
                            .map_or("—", |(s, _)| s.as_str());
                        ui.label(format!(
                            "Score: {}/{}{} | {range_str}",
                            self.score.correct, self.score.attempts, accuracy_str,
                        ));
                        if mode_state.is_none() {
                            if ui.small_button("Set Range [R]").clicked() {
                                self.mode = AppMode::SettingRange(Vec::new());
                                self.correct_at = None;
                                self.feedback = Feedback::Waiting;
                            }
                            if range_info.as_ref().is_some_and(|(_, changed)| *changed)
                                && ui.small_button("Reset").clicked()
                            {
                                self.reset_range();
                            }
                        }
                    }
                    if mode_state.is_none() && !song_is_complete
                        && ui.small_button("Restart [Ctrl+R]").clicked()
                    {
                        self.restart_song();
                    }
                    let kb_label = if self.show_keyboard { "Hide Keys [K]" } else { "Show Keys [K]" };
                    if ui.small_button(kb_label).clicked() {
                        self.show_keyboard = !self.show_keyboard;
                    }
                    if ui.small_button("Load MIDI").clicked() {
                        self.load_midi_file();
                    }
                    if midi_file_info.is_some() && ui.small_button("Exit Song [Esc]").clicked() {
                        self.exit_to_random();
                    }
                });

                if let Some(ref s) = box_display {
                    ui.label(s);
                }

                if let Some(ref name) = self.midi_port_name {
                    ui.colored_label(Color32::from_rgb(50, 180, 80), format!("Connected: {name}"));
                }
                ui.add_space(8.0);

                if let Some(ref err) = self.midi_error {
                    ui.colored_label(Color32::RED, format!("MIDI error: {err}"));
                    let ports = MidiReceiver::list_ports();
                    if ports.is_empty() {
                        ui.label("No MIDI ports detected.");
                    } else {
                        ui.label("Available ports:");
                        for p in &ports {
                            ui.label(format!("  • {p}"));
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });

        egui::Panel::bottom("feedback").show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);

                if song_is_complete {
                    let total = self.song.progress().map_or(0, |(_, t)| t);
                    ui.colored_label(
                        Color32::from_rgb(50, 180, 80),
                        format!("Complete! {}/{} notes correct{}.", self.score.correct, total, accuracy_str),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Restart [Ctrl+R]").clicked() {
                            self.restart_song();
                        }
                        if midi_file_info.is_some() && ui.button("Load New Song").clicked() {
                            self.load_midi_file();
                        }
                    });
                } else if let Some((count, first_key)) = mode_state {
                    if count == 0 {
                        ui.label("Press any 2 MIDI keys to set the training range — order doesn't matter.");
                    } else if let Some(k) = first_key {
                        ui.label(format!("Got {} — press one more key to complete the range.", Note::new(k)));
                    }
                    if ui.button("Cancel [Esc]").clicked() {
                        self.mode = AppMode::Playing;
                    }
                } else {
                    let is_midi_file = midi_file_info.is_some();
                    match self.feedback {
                        Feedback::Waiting => {
                            if let Some((idx, total)) = self.song.progress() {
                                ui.label(format!("Note {}/{} — play the note shown on the staff.", idx + 1, total));
                            } else {
                                ui.label(format!(
                                    "Play the note shown on the staff.  [Box {current_box}  |  fast threshold: {FAST_THRESHOLD_MS}ms]"
                                ));
                            }
                            ui.colored_label(
                                Color32::from_rgb(212, 175, 55),
                                format!("Note: {}", self.current_note),
                            );
                            if ui.button("Skip").clicked() {
                                self.advance_to_next();
                            }
                        }
                        Feedback::Correct(note) => {
                            let latency_ms = self.note_shown_at
                                .map_or(0, |t| t.elapsed().as_millis());
                            if is_midi_file {
                                ui.colored_label(
                                    Color32::from_rgb(50, 180, 80),
                                    format!("Correct! ({note})  {latency_ms}ms — next note…"),
                                );
                            } else {
                                let speed = if latency_ms <= u128::from(FAST_THRESHOLD_MS) { "fast" } else { "slow" };
                                ui.colored_label(
                                    Color32::from_rgb(50, 180, 80),
                                    format!("Correct! ({note})  {latency_ms}ms [{speed}] → Box {current_box}  — next note coming…"),
                                );
                            }
                            if ui.button("Next now").clicked() {
                                self.advance_to_next();
                            }
                        }
                        Feedback::Wrong { expected, got } => {
                            if is_midi_file {
                                ui.colored_label(
                                    Color32::from_rgb(210, 60, 60),
                                    format!("Wrong — expected {expected}, got {got}. Try again."),
                                );
                            } else {
                                ui.colored_label(
                                    Color32::from_rgb(210, 60, 60),
                                    format!("Wrong — expected {expected}, got {got}. Try again.  [Box {current_box}]"),
                                );
                            }
                            if ui.button("Skip").clicked() {
                                self.advance_to_next();
                            }
                        }
                        Feedback::OutOfRange(note) => {
                            let (lo, hi) = self.song.as_random()
                                .map_or((0, 127), |r| (r.active_low, r.active_high));
                            ui.colored_label(
                                Color32::from_rgb(180, 140, 50),
                                format!("You played {note} — outside the training range ({} – {}). Expected: {}.",
                                    Note::new(lo), Note::new(hi), self.current_note),
                            );
                            if ui.button("Skip").clicked() {
                                self.advance_to_next();
                            }
                        }
                    }
                }
                ui.add_space(12.0);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let rect = ui.available_rect_before_wrap();
            let (staff_rect, kb_rect) = if self.show_keyboard {
                let kb_height = (rect.height() * 0.20).max(60.0);
                let staff = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.max.y - kb_height));
                let kb = Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - kb_height), rect.max);
                (staff, Some(kb))
            } else {
                (rect, None)
            };
            let new_page_size = self.draw_staff(ui.painter(), staff_rect);
            if new_page_size != self.page_size {
                self.page_size = new_page_size;
                self.page = self.song.peek(self.page_size);
                self.page_cursor = 0;
                self.current_note = self.page.first().copied().unwrap_or(self.current_note);
            }
            if let Some(kb) = kb_rect {
                self.draw_keyboard(ui.painter(), kb);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── piano_is_black ────────────────────────────────────────────────────────

    #[test]
    fn black_keys_are_the_five_sharps() {
        // Within an octave, only C#/D#/F#/G#/A# are black.
        for pc in [1u8, 3, 6, 8, 10] {
            assert!(piano_is_black(pc), "pitch class {pc} should be black");
            assert!(piano_is_black(pc + 12), "pitch class {pc} + octave should be black");
        }
    }

    #[test]
    fn white_keys_are_the_seven_naturals() {
        for pc in [0u8, 2, 4, 5, 7, 9, 11] {
            assert!(!piano_is_black(pc), "pitch class {pc} should be white");
        }
    }

    #[test]
    fn black_pattern_repeats_every_octave() {
        for midi in 21u8..=96 {
            assert_eq!(
                piano_is_black(midi),
                piano_is_black(midi + 12),
                "black/white should repeat at octave for MIDI {midi}"
            );
        }
    }

    #[test]
    fn piano_range_boundaries_are_white() {
        assert!(!piano_is_black(21),  "A0 (MIDI 21) should be white");
        assert!(!piano_is_black(108), "C8 (MIDI 108) should be white");
    }

    #[test]
    fn a_sharp_is_black() {
        // A#0 = MIDI 22
        assert!(piano_is_black(22));
    }

    #[test]
    fn exactly_five_black_keys_per_octave() {
        let black_count = (0u8..12).filter(|&pc| piano_is_black(pc)).count();
        assert_eq!(black_count, 5);
    }

    #[test]
    fn exactly_seven_white_keys_per_octave() {
        let white_count = (0u8..12).filter(|&pc| !piano_is_black(pc)).count();
        assert_eq!(white_count, 7);
    }

    // ── piano_white_idx ───────────────────────────────────────────────────────

    #[test]
    fn a0_is_white_key_zero() {
        assert_eq!(piano_white_idx(21), 0);
    }

    #[test]
    fn b0_is_white_key_one() {
        // MIDI 23 = B0
        assert_eq!(piano_white_idx(23), 1);
    }

    #[test]
    fn c1_is_white_key_two() {
        // MIDI 24 = C1
        assert_eq!(piano_white_idx(24), 2);
    }

    #[test]
    fn white_keys_in_first_octave_fragment() {
        // A0=0, B0=1, then C1 starts at 2
        assert_eq!(piano_white_idx(21), 0);  // A0
        assert_eq!(piano_white_idx(23), 1);  // B0
        assert_eq!(piano_white_idx(24), 2);  // C1
        assert_eq!(piano_white_idx(26), 3);  // D1
        assert_eq!(piano_white_idx(28), 4);  // E1
        assert_eq!(piano_white_idx(29), 5);  // F1
        assert_eq!(piano_white_idx(31), 6);  // G1
        assert_eq!(piano_white_idx(33), 7);  // A1
        assert_eq!(piano_white_idx(35), 8);  // B1
        assert_eq!(piano_white_idx(36), 9);  // C2
    }

    #[test]
    fn middle_c_is_white_key_23() {
        // C4 = MIDI 60: 2 (A0-B0) + 7*(3 octaves C1-B3) = 23
        assert_eq!(piano_white_idx(60), 23);
    }

    #[test]
    fn c4_neighbours() {
        assert_eq!(piano_white_idx(59), 22);  // B3
        assert_eq!(piano_white_idx(60), 23);  // C4
        assert_eq!(piano_white_idx(62), 24);  // D4
        assert_eq!(piano_white_idx(64), 25);  // E4
        assert_eq!(piano_white_idx(65), 26);  // F4
        assert_eq!(piano_white_idx(67), 27);  // G4
        assert_eq!(piano_white_idx(69), 28);  // A4
        assert_eq!(piano_white_idx(71), 29);  // B4
        assert_eq!(piano_white_idx(72), 30);  // C5
    }

    #[test]
    fn c8_is_white_key_51() {
        // MIDI 108 = C8, the last white key on an 88-key piano
        assert_eq!(piano_white_idx(108), 51);
    }

    #[test]
    fn seven_new_white_keys_per_octave() {
        // Each octave from C adds exactly 7 white keys.
        for start in [24u8, 36, 48, 60, 72, 84] {
            let idx_start = piano_white_idx(start);
            let idx_next_c = piano_white_idx(start + 12);
            assert_eq!(
                idx_next_c - idx_start, 7,
                "Octave from MIDI {start} should add 7 white keys"
            );
        }
    }

    #[test]
    fn white_key_indices_are_strictly_ascending() {
        let mut prev = piano_white_idx(21);
        for midi in 22u8..=108 {
            if piano_is_black(midi) { continue; }
            let curr = piano_white_idx(midi);
            assert_eq!(
                curr, prev + 1,
                "White key indices must be consecutive (MIDI {midi})"
            );
            prev = curr;
        }
    }

    #[test]
    fn total_white_keys_in_piano_range() {
        let count = (21u8..=108).filter(|&m| !piano_is_black(m)).count();
        assert_eq!(count, 52, "An 88-key piano has 52 white keys");
    }

    // ── piano_key_center_x ────────────────────────────────────────────────────

    const WW: f32 = 10.0; // test white-key width

    #[test]
    fn a0_center_is_half_white_width() {
        // A0 = white key 0; center = 0.5 * WW
        assert!((piano_key_center_x(21, WW) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn b0_center() {
        // B0 = white key 1; center = 1.5 * WW
        assert!((piano_key_center_x(23, WW) - 15.0).abs() < 1e-4);
    }

    #[test]
    fn a_sharp0_center_is_between_a0_and_b0() {
        // A#0 is between A0 (ends at WW) and B0 (starts at WW): center = WW
        let cx = piano_key_center_x(22, WW);
        assert!((cx - 10.0).abs() < 1e-4, "A#0 center should be {WW}, got {cx}");
    }

    #[test]
    fn c1_center() {
        // C1 = white key 2; center = 2.5 * WW
        assert!((piano_key_center_x(24, WW) - 25.0).abs() < 1e-4);
    }

    #[test]
    fn c_sharp1_center_is_between_c1_and_d1() {
        // C#1 black: right edge of C1 (white key 2) = 3 * WW = 30.0
        let cx = piano_key_center_x(25, WW);
        assert!((cx - 30.0).abs() < 1e-4, "C#1 center should be 30.0, got {cx}");
    }

    #[test]
    fn c8_center_is_last_key() {
        // C8 = white key 51; center = 51.5 * WW
        let cx = piano_key_center_x(108, WW);
        assert!((cx - 515.0).abs() < 1e-4, "C8 center should be 515.0, got {cx}");
    }

    #[test]
    fn black_key_center_always_between_adjacent_white_keys() {
        // For every black key, its center must lie strictly between the centers of
        // the white keys immediately to its left and right.
        for midi in 22u8..=108 {
            if !piano_is_black(midi) { continue; }
            let left_center = piano_key_center_x(midi - 1, WW);
            let right_center = piano_key_center_x(midi + 1, WW);
            let black_center = piano_key_center_x(midi, WW);
            assert!(
                black_center > left_center && black_center < right_center,
                "Black key MIDI {midi} center {black_center} not between {left_center} and {right_center}"
            );
        }
    }

    #[test]
    fn white_key_centers_are_one_white_width_apart() {
        let mut prev_center = piano_key_center_x(21, WW);
        for midi in 22u8..=108 {
            if piano_is_black(midi) { continue; }
            let curr_center = piano_key_center_x(midi, WW);
            let gap = (curr_center - prev_center - WW).abs();
            assert!(gap < 1e-3, "Adjacent white keys should be WW apart (MIDI {midi}): gap={gap}");
            prev_center = curr_center;
        }
    }

    // ── piano_key_color ───────────────────────────────────────────────────────

    const BASE: Color32 = Color32::from_gray(245);
    const GOLD: Color32 = Color32::from_rgb(212, 175, 55);
    const GREEN: Color32 = Color32::from_rgb(50, 180, 80);
    const RED: Color32 = Color32::from_rgb(210, 60, 60);

    #[test]
    fn non_expected_key_with_no_flash_returns_base() {
        let color = piano_key_color(60, 62, None, BASE);
        assert_eq!(color, BASE);
    }

    #[test]
    fn expected_key_with_no_flash_returns_gold() {
        let color = piano_key_color(60, 60, None, BASE);
        assert_eq!(color, GOLD);
    }

    #[test]
    fn correct_flash_on_played_key_returns_green() {
        let color = piano_key_color(60, 60, Some((60, true)), BASE);
        assert_eq!(color, GREEN);
    }

    #[test]
    fn wrong_flash_on_played_key_returns_red() {
        let color = piano_key_color(60, 60, Some((60, false)), BASE);
        assert_eq!(color, RED);
    }

    #[test]
    fn correct_flash_on_different_key_does_not_affect_other_keys() {
        // Flash is on MIDI 62; MIDI 60 is expected — should still show gold.
        let color = piano_key_color(60, 60, Some((62, true)), BASE);
        assert_eq!(color, GOLD);
    }

    #[test]
    fn wrong_flash_on_different_key_does_not_affect_other_keys() {
        // Flash is on MIDI 62; MIDI 60 is expected — should still show gold.
        let color = piano_key_color(60, 60, Some((62, false)), BASE);
        assert_eq!(color, GOLD);
    }

    #[test]
    fn correct_flash_key_shows_green_even_if_not_expected() {
        // MIDI 62 was played correctly; it is not the expected note — still green.
        let color = piano_key_color(62, 60, Some((62, true)), BASE);
        assert_eq!(color, GREEN);
    }

    #[test]
    fn wrong_flash_key_shows_red_even_if_not_expected() {
        let color = piano_key_color(62, 60, Some((62, false)), BASE);
        assert_eq!(color, RED);
    }

    #[test]
    fn green_takes_priority_over_gold_when_flash_is_on_expected_key() {
        // Correct flash on the expected key: green wins over gold.
        let color = piano_key_color(60, 60, Some((60, true)), BASE);
        assert_eq!(color, GREEN, "green should take priority over gold");
    }

    #[test]
    fn red_takes_priority_over_gold_when_flash_is_on_expected_key() {
        // Wrong flash on the expected key: red wins over gold.
        let color = piano_key_color(60, 60, Some((60, false)), BASE);
        assert_eq!(color, RED, "red should take priority over gold");
    }

    #[test]
    fn unrelated_key_with_flash_elsewhere_returns_base() {
        // MIDI 64 is neither expected (60) nor flashed (62): base color.
        let color = piano_key_color(64, 60, Some((62, true)), BASE);
        assert_eq!(color, BASE);
    }

    #[test]
    fn no_flash_non_expected_key_always_base() {
        for midi in [21u8, 36, 48, 72, 84, 96, 108] {
            let color = piano_key_color(midi, 60, None, BASE);
            assert_eq!(color, BASE, "MIDI {midi} should be base with no flash");
        }
    }
}
