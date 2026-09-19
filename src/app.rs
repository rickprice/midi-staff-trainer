use crate::{
    config::Config,
    midi::MidiReceiver,
    scheduler::{Scheduler, BOX_WEIGHTS, FAST_THRESHOLD_MS},
    staff::Note,
};
use egui::{Color32, Painter, Pos2, Rect, Stroke};
use std::time::{Duration, Instant};

const CORRECT_DISPLAY_MS: u64 = 900;

enum AppMode {
    Training,
    SettingRange(Vec<u8>),
}

pub struct TrainerApp {
    config: Config,
    midi: Option<MidiReceiver>,
    midi_port_name: Option<String>,
    midi_error: Option<String>,
    current_note: Note,
    feedback: Feedback,
    correct_at: Option<Instant>,
    note_shown_at: Option<Instant>,   // when the current note appeared; used for latency
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
        let config = Config::load();
        let (midi, midi_port_name, midi_error) = match MidiReceiver::connect(config.midi_port.as_deref(), _cc.egui_ctx.clone()) {
            Ok(r) => {
                let name = r.port_name.clone();
                (Some(r), Some(name), None)
            }
            Err(e) => (None, None, Some(e)),
        };
        let active_low = config.midi_low;
        let active_high = config.midi_high;
        let mut scheduler = Scheduler::new(active_low, active_high);
        let current_note = scheduler.pick_next();
        Self {
            config,
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

    fn next_note(&mut self) {
        self.current_note = self.scheduler.pick_next();
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
        self.note_shown_at = Some(Instant::now());
    }

    fn handle_midi_note(&mut self, played: u8) {
        if matches!(self.feedback, Feedback::Correct(_)) {
            return; // ignore input while success is displayed
        }
        self.score.attempts += 1;
        if played == self.current_note.midi {
            let latency = self.note_shown_at
                .map(|t| t.elapsed())
                .unwrap_or(Duration::from_secs(99));
            self.scheduler.record_correct(played, latency);
            self.score.correct += 1;
            self.feedback = Feedback::Correct(self.current_note);
            self.correct_at = Some(Instant::now());
        } else {
            self.scheduler.record_incorrect(self.current_note.midi);
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
        self.scheduler = Scheduler::new(low, high);
        self.mode = AppMode::Training;
        self.next_note();
    }

    fn reset_range(&mut self) {
        let (low, high) = (self.config.midi_low, self.config.midi_high);
        self.set_active_range(low, high);
    }

    fn draw_staff(&self, painter: &Painter, rect: Rect) {
        let cx = rect.center().x;
        let cy = rect.center().y;
        // Scale so the 5 staff lines fill ~30 % of the available height.
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

        // staff_position() maps C4 → 0; E4 (treble bottom line) → 2.
        let pos = self.current_note.staff_position();
        let bottom_line_y = cy + 2.0 * line_spacing;
        let top_line_y = cy - 2.0 * line_spacing;
        let note_y = bottom_line_y - (pos - 2) as f32 * (line_spacing / 2.0);
        let note_x = cx;
        let note_r = line_spacing * 0.45;
        let ledger_hw = note_r * 2.2;
        let ledger_stroke = Stroke::new(1.5_f32, staff_color);

        // Ledger lines below staff.
        let mut ly = bottom_line_y + line_spacing;
        while ly <= note_y + 0.5 {
            painter.line_segment(
                [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                ledger_stroke,
            );
            ly += line_spacing;
        }
        // Ledger lines above staff.
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

        painter.circle_filled(Pos2::new(note_x, note_y), note_r, note_color);

        // Stem up when note is at or below B4 (position 6 = midline of treble staff).
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

/// Draws a treble clef symbol at the left edge of the staff.
///
/// `x0` — left edge of the staff lines; `cy` — vertical centre of the staff
/// (the B4 / middle line); `s` — line spacing in pixels.
fn draw_treble_clef(painter: &Painter, x0: f32, cy: f32, s: f32, color: Color32) {
    use egui::epaint::{CubicBezierShape, PathStroke};

    let sw = 1.8_f32;
    let psk = PathStroke::new(sw, color);
    let bezier = |pts: [Pos2; 4]| {
        CubicBezierShape::from_points_stroke(pts, false, Color32::TRANSPARENT, psk.clone())
    };

    // Stem x; oval is shifted slightly right of the stem so the stem bisects it.
    let sx = x0 + s * 0.70;
    let ox = sx + 0.12 * s;          // oval centre x
    let oy = cy + 0.95 * s;          // oval centre y  (near G4 line = cy + s)
    let rx = 0.55 * s;               // oval half-width
    let ry = 1.10 * s;               // oval half-height
    let kx = 0.5523 * rx;            // bezier approximation constant
    let ky = 0.5523 * ry;

    let otop   = Pos2::new(ox,      oy - ry);
    let oright = Pos2::new(ox + rx, oy);
    let obot   = Pos2::new(ox,      oy + ry);
    let oleft  = Pos2::new(ox - rx, oy);

    // ── Oval loop (four quarter-arc cubic bezier segments) ───────────────────
    painter.add(bezier([otop,   Pos2::new(ox + kx, oy - ry), Pos2::new(ox + rx, oy - ky), oright]));
    painter.add(bezier([oright, Pos2::new(ox + rx, oy + ky), Pos2::new(ox + kx, oy + ry), obot  ]));
    painter.add(bezier([obot,   Pos2::new(ox - kx, oy + ry), Pos2::new(ox - rx, oy + ky), oleft ]));
    painter.add(bezier([oleft,  Pos2::new(ox - rx, oy - ky), Pos2::new(ox - kx, oy - ry), otop  ]));

    // ── Stem (vertical line through the oval) ────────────────────────────────
    let stem_top_y = cy - 3.10 * s;
    let stem_bot_y = cy + 3.30 * s;
    painter.line_segment(
        [Pos2::new(sx, stem_top_y), Pos2::new(sx, stem_bot_y)],
        Stroke::new(sw, color),
    );

    // ── Top curl: loop at stem top, then descend to connect to oval top ──────
    let curl_end = Pos2::new(sx + 0.60 * s, stem_top_y + 0.45 * s);
    painter.add(bezier([
        Pos2::new(sx, stem_top_y),
        Pos2::new(sx + 0.45 * s, stem_top_y - 0.55 * s),
        Pos2::new(sx + 0.90 * s, stem_top_y - 0.25 * s),
        curl_end,
    ]));
    painter.add(bezier([
        curl_end,
        Pos2::new(sx + 0.25 * s, stem_top_y + 1.00 * s),
        Pos2::new(sx + 0.10 * s, stem_top_y + 1.60 * s),
        otop,
    ]));

    // ── Bottom hook: sweeps right then turns back up ─────────────────────────
    painter.add(bezier([
        Pos2::new(sx,             stem_bot_y),
        Pos2::new(sx + 0.35 * s, stem_bot_y + 0.20 * s),
        Pos2::new(sx + 0.70 * s, stem_bot_y + 0.05 * s),
        Pos2::new(sx + 0.50 * s, stem_bot_y - 0.55 * s),
    ]));
}

impl eframe::App for TrainerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drain the MIDI channel into a local buffer before borrowing self mutably.
        let midi_notes: Vec<u8> = self
            .midi
            .as_ref()
            .map(|m| std::iter::from_fn(|| m.rx.try_recv().ok()).collect())
            .unwrap_or_default();
        let is_setting_range = matches!(self.mode, AppMode::SettingRange(_));
        for note in midi_notes {
            if is_setting_range {
                self.handle_range_key(note);
            } else if (self.active_low..=self.active_high).contains(&note) {
                self.handle_midi_note(note);
            } else {
                self.feedback = Feedback::OutOfRange(Note::new(note));
            }
        }

        // R enters range-capture mode; Esc cancels it.
        let (r_pressed, esc_pressed) = ctx.input(|i| (
            i.key_pressed(egui::Key::R),
            i.key_pressed(egui::Key::Escape),
        ));
        if r_pressed && matches!(self.mode, AppMode::Training) {
            self.mode = AppMode::SettingRange(Vec::new());
            self.correct_at = None;
            self.feedback = Feedback::Waiting;
        }
        if esc_pressed && matches!(self.mode, AppMode::SettingRange(_)) {
            self.mode = AppMode::Training;
        }

        // Schedule repaint for the auto-advance moment; advance when time arrives.
        if let Some(t) = self.correct_at {
            let elapsed = t.elapsed();
            let delay = Duration::from_millis(CORRECT_DISPLAY_MS);
            if elapsed >= delay {
                self.next_note();
            } else {
                ctx.request_repaint_after(delay - elapsed);
            }
        }

        let box_counts = self.scheduler.box_counts();
        let current_box = self.scheduler.note_box(self.current_note.midi);
        let mode_state: Option<(usize, Option<u8>)> = match &self.mode {
            AppMode::SettingRange(keys) => Some((keys.len(), keys.first().copied())),
            AppMode::Training => None,
        };
        let range_changed = self.active_low != self.config.midi_low
            || self.active_high != self.config.midi_high;

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading("MIDI Staff Trainer");
                ui.add_space(4.0);

                // Score and accuracy
                let accuracy_str = self.score.accuracy_pct()
                    .map(|p| format!(" ({p}%)"))
                    .unwrap_or_default();
                ui.horizontal(|ui| {
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
                });

                // Leitner box distribution
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

        egui::TopBottomPanel::bottom("feedback").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(12.0);
                if let Some((count, first_key)) = mode_state {
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
                                Color32::from_gray(140),
                                format!("Note: {}", self.current_note),
                            );
                            if ui.button("Skip").clicked() {
                                self.next_note();
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
                                self.next_note();
                            }
                        }
                        Feedback::Wrong { expected, got } => {
                            ui.colored_label(
                                Color32::from_rgb(210, 60, 60),
                                format!("Wrong — expected {expected}, got {got}. Try again.  [Box {current_box}]"),
                            );
                            if ui.button("Skip").clicked() {
                                self.next_note();
                            }
                        }
                        Feedback::OutOfRange(note) => {
                            ui.colored_label(
                                Color32::from_rgb(180, 140, 50),
                                format!("You played {note} — outside the training range ({} – {}). Play a note within range.",
                                    Note::new(self.active_low),
                                    Note::new(self.active_high),
                                ),
                            );
                            if ui.button("Skip").clicked() {
                                self.next_note();
                            }
                        }
                    }
                }
                ui.add_space(12.0);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            self.draw_staff(ui.painter(), rect);
        });
    }
}
