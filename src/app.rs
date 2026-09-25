use crate::{
    config::Config,
    midi::MidiReceiver,
    scheduler::{BOX_WEIGHTS, FAST_THRESHOLD_MS},
    song::{MidiFileSong, RandomSong, Song},
    staff::Note,
    state::AppState,
};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Stroke};
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
    }

    fn reset_page(&mut self) {
        self.page = self.song.peek(self.page_size.max(1));
        self.page_cursor = 0;
        self.beat_cursor = 0.0;
        self.current_note = self.page.first().copied().unwrap_or(Note::new(60));
    }

    fn restart_song(&mut self) {
        self.song.restart();
        self.reset_page();
        self.score = Score::default();
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
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
        } else {
            self.song.record_incorrect(self.current_note.midi);
            self.feedback = Feedback::Wrong {
                expected: self.current_note,
                got: Note::new(played),
            };
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

        let (r_pressed, ctrl_r_pressed, esc_pressed) = ctx.input(|i| (
            i.key_pressed(egui::Key::R) && !i.modifiers.ctrl,
            i.key_pressed(egui::Key::R) && i.modifiers.ctrl,
            i.key_pressed(egui::Key::Escape),
        ));
        if ctrl_r_pressed && matches!(self.mode, AppMode::Playing) {
            self.restart_song();
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
            let new_page_size = self.draw_staff(ui.painter(), rect);
            if new_page_size != self.page_size {
                self.page_size = new_page_size;
                self.page = self.song.peek(self.page_size);
                self.page_cursor = 0;
                self.current_note = self.page.first().copied().unwrap_or(self.current_note);
            }
        });
    }
}
