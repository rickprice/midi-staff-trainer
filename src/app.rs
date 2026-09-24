use crate::{
    config::Config,
    midi::MidiReceiver,
    scheduler::{Scheduler, BOX_WEIGHTS, FAST_THRESHOLD_MS},
    song::SongPlayer,
    staff::Note,
    state::AppState,
};
use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect, Stroke};
use std::time::{Duration, Instant};

const CORRECT_DISPLAY_MS: u64 = 900;

enum AppMode {
    Training,
    SettingRange(Vec<u8>),
    Song(SongPlayer),
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
    scheduler: Scheduler,
    active_low: u8,
    active_high: u8,
    mode: AppMode,
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
    pub fn new(_cc: &eframe::CreationContext) -> Self {
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
        _cc.egui_ctx.set_fonts(fonts);

        let config = Config::load();
        let app_state = AppState::load();
        let (midi, midi_port_name, midi_error) = match MidiReceiver::connect(config.midi_port.as_deref(), _cc.egui_ctx.clone()) {
            Ok(r) => {
                let name = r.port_name.clone();
                (Some(r), Some(name), None)
            }
            Err(e) => (None, None, Some(e)),
        };
        let active_low = app_state.training_low.unwrap_or(config.midi_low);
        let active_high = app_state.training_high.unwrap_or(config.midi_high);
        let mut scheduler = Scheduler::new(active_low, active_high);
        let current_note = scheduler.pick_next();
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
            scheduler,
            active_low,
            active_high,
            mode: AppMode::Training,
        }
    }

    /// Advance to the next note — random in Training mode, sequential in Song mode.
    fn advance_to_next(&mut self) {
        let next = match &mut self.mode {
            AppMode::Song(player) => {
                player.advance();
                player.current()
            }
            _ => Some(self.scheduler.pick_next()),
        };
        if let Some(note) = next {
            self.current_note = note;
        }
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
    }

    fn handle_midi_note(&mut self, played: u8) {
        if matches!(self.feedback, Feedback::Correct(_)) {
            return;
        }
        let is_training = matches!(self.mode, AppMode::Training);
        self.score.attempts += 1;
        if played == self.current_note.midi {
            let latency = self.note_shown_at
                .map(|t| t.elapsed())
                .unwrap_or(Duration::from_secs(99));
            if is_training {
                self.scheduler.record_correct(played, latency);
            }
            self.score.correct += 1;
            self.feedback = Feedback::Correct(self.current_note);
            self.correct_at = Some(Instant::now());
        } else {
            if is_training {
                self.scheduler.record_incorrect(self.current_note.midi);
            }
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
        self.active_low = low;
        self.active_high = high;
        self.app_state.training_low = Some(low);
        self.app_state.training_high = Some(high);
        self.app_state.save();
        self.scheduler = Scheduler::new(low, high);
        self.mode = AppMode::Training;
        self.advance_to_next();
    }

    fn reset_range(&mut self) {
        let (low, high) = (self.config.midi_low, self.config.midi_high);
        self.set_active_range(low, high);
    }

    fn load_midi_file(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("MIDI files", &["mid", "midi"])
            .pick_file();
        if let Some(path) = picked {
            match SongPlayer::load(&path) {
                Ok(mut player) => {
                    self.current_note = player.current().unwrap_or(Note::new(60));
                    player.index = 0; // already 0, but be explicit
                    self.mode = AppMode::Song(player);
                    self.score = Score::default();
                    self.feedback = Feedback::Waiting;
                    self.correct_at = None;
                    self.note_shown_at = Some(Instant::now());
                    self.midi_error = None;
                }
                Err(e) => self.midi_error = Some(e),
            }
        }
    }

    fn draw_staff(&self, painter: &Painter, rect: Rect) {
        let cx = rect.center().x;
        let cy = rect.center().y;
        let line_spacing = (rect.height() * 0.075).clamp(12.0, 28.0);
        let staff_width = rect.width() * 0.85;
        let x0 = cx - staff_width / 2.0;
        let x1 = cx + staff_width / 2.0;

        let staff_color = Color32::from_gray(220);
        let staff_stroke = Stroke::new(1.5_f32, staff_color);
        for i in 0..5_i32 {
            let y = cy + (2 - i) as f32 * line_spacing;
            painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], staff_stroke);
        }

        draw_treble_clef(painter, x0, cy, line_spacing, staff_color);

        let pos = self.current_note.staff_position();
        let bottom_line_y = cy + 2.0 * line_spacing;
        let top_line_y = cy - 2.0 * line_spacing;
        let note_y = bottom_line_y - (pos - 2) as f32 * (line_spacing / 2.0);
        let note_x = cx;
        let note_r = line_spacing * 0.45;
        let ledger_hw = note_r * 2.2;
        let ledger_stroke = Stroke::new(1.5_f32, staff_color);

        let mut ly = bottom_line_y + line_spacing;
        while ly <= note_y + 0.5 {
            painter.line_segment(
                [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                ledger_stroke,
            );
            ly += line_spacing;
        }
        let mut ly = top_line_y - line_spacing;
        while ly >= note_y - 0.5 {
            painter.line_segment(
                [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                ledger_stroke,
            );
            ly -= line_spacing;
        }

        let note_color = match self.feedback {
            Feedback::Correct(_) => Color32::from_rgb(50, 180, 80),
            Feedback::Wrong { .. } => Color32::from_rgb(210, 60, 60),
            Feedback::Waiting | Feedback::OutOfRange(_) => Color32::from_gray(230),
        };

        // Sharp symbol to the left of the note head for accidentals.
        if self.current_note.is_accidental() {
            painter.text(
                Pos2::new(note_x - note_r * 3.5, note_y),
                egui::Align2::CENTER_CENTER,
                "♯",
                FontId::proportional(line_spacing * 1.3),
                note_color,
            );
        }

        painter.circle_filled(Pos2::new(note_x, note_y), note_r, note_color);

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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let midi_notes: Vec<u8> = self
            .midi
            .as_ref()
            .map(|m| std::iter::from_fn(|| m.rx.try_recv().ok()).collect())
            .unwrap_or_default();

        let is_setting_range = matches!(self.mode, AppMode::SettingRange(_));
        let is_song_mode = matches!(self.mode, AppMode::Song(_));

        for note in midi_notes {
            if is_setting_range {
                self.handle_range_key(note);
            } else if is_song_mode {
                // In song mode any key is checked against the current song note;
                // there is no range restriction.
                self.handle_midi_note(note);
            } else if (self.active_low..=self.active_high).contains(&note) {
                self.handle_midi_note(note);
            } else {
                self.feedback = Feedback::OutOfRange(Note::new(note));
            }
        }

        let (r_pressed, esc_pressed) = ctx.input(|i| (
            i.key_pressed(egui::Key::R),
            i.key_pressed(egui::Key::Escape),
        ));
        if r_pressed && matches!(self.mode, AppMode::Training) {
            self.mode = AppMode::SettingRange(Vec::new());
            self.correct_at = None;
            self.feedback = Feedback::Waiting;
        }
        if esc_pressed {
            match &self.mode {
                AppMode::SettingRange(_) => self.mode = AppMode::Training,
                AppMode::Song(_) => {
                    self.mode = AppMode::Training;
                    self.current_note = self.scheduler.pick_next();
                    self.feedback = Feedback::Waiting;
                    self.correct_at = None;
                    self.note_shown_at = Some(Instant::now());
                }
                AppMode::Training => {}
            }
        }

        if let Some(t) = self.correct_at {
            let elapsed = t.elapsed();
            let delay = Duration::from_millis(CORRECT_DISPLAY_MS);
            if elapsed >= delay {
                let song_done = matches!(&self.mode, AppMode::Song(p) if p.is_complete());
                if !song_done {
                    self.advance_to_next();
                } else {
                    self.correct_at = None;
                }
            } else {
                ctx.request_repaint_after(delay - elapsed);
            }
        }

        // Snapshot read-only fields before UI drawing (avoids re-borrows).
        let accuracy_str = self.score.accuracy_pct()
            .map(|p| format!(" ({p}%)"))
            .unwrap_or_default();
        let box_counts = self.scheduler.box_counts();
        let current_box = self.scheduler.note_box(self.current_note.midi);
        let mode_state: Option<(usize, Option<u8>)> = match &self.mode {
            AppMode::SettingRange(keys) => Some((keys.len(), keys.first().copied())),
            _ => None,
        };
        let range_changed = self.active_low != self.config.midi_low
            || self.active_high != self.config.midi_high;
        let song_info: Option<(String, usize, usize, bool)> = match &self.mode {
            AppMode::Song(p) => Some((p.filename.clone(), p.index, p.total(), p.is_complete())),
            _ => None,
        };

        egui::Panel::top("header").show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading("MIDI Staff Trainer");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    if let Some((ref filename, idx, total, _)) = song_info {
                        ui.label(format!(
                            "Score: {}/{}{} | Song: {} | Note {}/{}",
                            self.score.correct,
                            self.score.attempts,
                            accuracy_str,
                            filename,
                            idx,
                            total,
                        ));
                    } else {
                        ui.label(format!(
                            "Score: {}/{}{} | Range: {} – {}",
                            self.score.correct,
                            self.score.attempts,
                            accuracy_str,
                            Note::new(self.active_low),
                            Note::new(self.active_high),
                        ));
                        if mode_state.is_none() {
                            if ui.small_button("Set Range [R]").clicked() {
                                self.mode = AppMode::SettingRange(Vec::new());
                                self.correct_at = None;
                                self.feedback = Feedback::Waiting;
                            }
                            if range_changed && ui.small_button("Reset").clicked() {
                                self.reset_range();
                            }
                        }
                    }
                    if ui.small_button("Load MIDI").clicked() {
                        self.load_midi_file();
                    }
                    if song_info.is_some() && ui.small_button("Exit Song [Esc]").clicked() {
                        self.mode = AppMode::Training;
                        self.current_note = self.scheduler.pick_next();
                        self.feedback = Feedback::Waiting;
                        self.correct_at = None;
                        self.note_shown_at = Some(Instant::now());
                    }
                });

                if song_info.is_none() {
                    let box_str = box_counts
                        .iter()
                        .enumerate()
                        .map(|(i, &n)| format!("{i}:{n}"))
                        .collect::<Vec<_>>()
                        .join("  ");
                    ui.label(format!(
                        "Boxes (weight {:.0}/{:.0}/{:.0}/{:.1}/{:.1}) → {box_str}",
                        BOX_WEIGHTS[0], BOX_WEIGHTS[1], BOX_WEIGHTS[2],
                        BOX_WEIGHTS[3], BOX_WEIGHTS[4],
                    ));
                }

                if let Some(ref name) = self.midi_port_name {
                    ui.colored_label(
                        Color32::from_rgb(50, 180, 80),
                        format!("Connected: {name}"),
                    );
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

                if let Some((_, idx, total, complete)) = song_info {
                    if complete {
                        ui.colored_label(
                            Color32::from_rgb(50, 180, 80),
                            format!("Song complete! {}/{} notes correct{}.", self.score.correct, total, accuracy_str),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Restart Song").clicked() {
                                if let AppMode::Song(ref mut player) = self.mode {
                                    player.restart();
                                    self.current_note = player.current().unwrap_or(Note::new(60));
                                }
                                self.score = Score::default();
                                self.feedback = Feedback::Waiting;
                                self.correct_at = None;
                                self.note_shown_at = Some(Instant::now());
                            }
                            if ui.button("Load New Song").clicked() {
                                self.load_midi_file();
                            }
                        });
                    } else {
                        match self.feedback {
                            Feedback::Waiting => {
                                ui.label(format!(
                                    "Note {}/{} — play the note shown on the staff.",
                                    idx + 1,
                                    total,
                                ));
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
                                    .map(|t| t.elapsed().as_millis())
                                    .unwrap_or(0);
                                ui.colored_label(
                                    Color32::from_rgb(50, 180, 80),
                                    format!("Correct! ({note})  {latency_ms}ms — next note…"),
                                );
                                if ui.button("Next now").clicked() {
                                    self.advance_to_next();
                                }
                            }
                            Feedback::Wrong { expected, got } => {
                                ui.colored_label(
                                    Color32::from_rgb(210, 60, 60),
                                    format!("Wrong — expected {expected}, got {got}. Try again."),
                                );
                                if ui.button("Skip").clicked() {
                                    self.advance_to_next();
                                }
                            }
                            Feedback::OutOfRange(_) => {}
                        }
                    }
                } else if let Some((count, first_key)) = mode_state {
                    if count == 0 {
                        ui.label("Press any 2 MIDI keys to define the training range — order doesn't matter.");
                    } else if let Some(k) = first_key {
                        ui.label(format!(
                            "Got {} — press one more key to complete the range.",
                            Note::new(k),
                        ));
                    }
                    if ui.button("Cancel [Esc]").clicked() {
                        self.mode = AppMode::Training;
                    }
                } else {
                    match self.feedback {
                        Feedback::Waiting => {
                            ui.label(format!(
                                "Play the note shown on the staff.  [Box {current_box}  |  fast threshold: {FAST_THRESHOLD_MS}ms]"
                            ));
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
                                .map(|t| t.elapsed().as_millis())
                                .unwrap_or(0);
                            let speed = if latency_ms <= FAST_THRESHOLD_MS as u128 { "fast" } else { "slow" };
                            ui.colored_label(
                                Color32::from_rgb(50, 180, 80),
                                format!("Correct! ({note})  {latency_ms}ms [{speed}] → Box {current_box}  — next note coming…"),
                            );
                            if ui.button("Next now").clicked() {
                                self.advance_to_next();
                            }
                        }
                        Feedback::Wrong { expected, got } => {
                            ui.colored_label(
                                Color32::from_rgb(210, 60, 60),
                                format!("Wrong — expected {expected}, got {got}. Try again.  [Box {current_box}]"),
                            );
                            if ui.button("Skip").clicked() {
                                self.advance_to_next();
                            }
                        }
                        Feedback::OutOfRange(note) => {
                            ui.colored_label(
                                Color32::from_rgb(180, 140, 50),
                                format!("You played {note} — outside the training range ({} – {}). Expected: {}.",
                                    Note::new(self.active_low),
                                    Note::new(self.active_high),
                                    self.current_note,
                                ),
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
            self.draw_staff(ui.painter(), rect);
        });
    }
}
