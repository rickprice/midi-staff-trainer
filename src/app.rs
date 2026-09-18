use crate::{
    config::Config,
    midi::MidiReceiver,
    staff::{Note, random_natural_note},
};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke};
use std::time::{Duration, Instant};

const CORRECT_DISPLAY_MS: u64 = 900;

pub struct TrainerApp {
    config: Config,
    midi: Option<MidiReceiver>,
    midi_error: Option<String>,
    current_note: Note,
    feedback: Feedback,
    correct_at: Option<Instant>,
    score: Score,
}

#[derive(Default)]
struct Score {
    correct: u32,
    attempts: u32,
}

enum Feedback {
    Waiting,
    Correct(Note),
    Wrong { expected: Note, got: Note },
}

impl TrainerApp {
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        let config = Config::load();
        let (midi, midi_error) = match MidiReceiver::connect(config.midi_port.as_deref()) {
            Ok(r) => (Some(r), None),
            Err(e) => (None, Some(e)),
        };
        let current_note = random_natural_note(config.midi_low, config.midi_high);
        Self {
            config,
            midi,
            midi_error,
            current_note,
            feedback: Feedback::Waiting,
            correct_at: None,
            score: Score::default(),
        }
    }

    fn next_note(&mut self) {
        self.current_note = random_natural_note(self.config.midi_low, self.config.midi_high);
        self.feedback = Feedback::Waiting;
        self.correct_at = None;
    }

    fn handle_midi_note(&mut self, played: u8) {
        if matches!(self.feedback, Feedback::Correct(_)) {
            return; // ignore input while success is displayed
        }
        self.score.attempts += 1;
        if played == self.current_note.midi {
            self.score.correct += 1;
            self.feedback = Feedback::Correct(self.current_note);
            self.correct_at = Some(Instant::now());
        } else {
            self.feedback = Feedback::Wrong {
                expected: self.current_note,
                got: Note::new(played),
            };
        }
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
            Feedback::Waiting => Color32::from_gray(230),
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

        // Note name label: at least 3 line-spacings below the bottom staff line,
        // or below the note head itself — whichever is lower.
        let label_y = f32::max(
            bottom_line_y + line_spacing * 3.0,
            note_y + note_r + line_spacing,
        );
        painter.text(
            Pos2::new(note_x, label_y),
            Align2::CENTER_CENTER,
            self.current_note.to_string(),
            FontId::proportional(line_spacing * 1.1),
            note_color,
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
        for note in midi_notes {
            if (self.config.midi_low..=self.config.midi_high).contains(&note) {
                self.handle_midi_note(note);
            }
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

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading("MIDI Staff Trainer");
                ui.add_space(4.0);
                ui.label(format!(
                    "Score: {}/{} | Range: {} – {}",
                    self.score.correct,
                    self.score.attempts,
                    Note::new(self.config.midi_low),
                    Note::new(self.config.midi_high),
                ));
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
                match self.feedback {
                    Feedback::Waiting => {
                        ui.label("Play the note shown on the staff.");
                        if ui.button("Skip").clicked() {
                            self.next_note();
                        }
                    }
                    Feedback::Correct(note) => {
                        ui.colored_label(
                            Color32::from_rgb(50, 180, 80),
                            format!("Correct! ({note}) — next note coming…"),
                        );
                        if ui.button("Next now").clicked() {
                            self.next_note();
                        }
                    }
                    Feedback::Wrong { expected, got } => {
                        ui.colored_label(
                            Color32::from_rgb(210, 60, 60),
                            format!("Wrong — expected {expected}, got {got}. Try again."),
                        );
                        if ui.button("Skip").clicked() {
                            self.next_note();
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
