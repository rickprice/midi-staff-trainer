use crate::{
    config::Config,
    midi::MidiReceiver,
    staff::{Note, random_natural_note},
};
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
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
    Correct(String),
    Wrong { expected: String, got: String },
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
            return;
        }
        self.score.attempts += 1;
        if played == self.current_note.midi {
            self.score.correct += 1;
            self.feedback = Feedback::Correct(self.current_note.name());
            self.correct_at = Some(Instant::now());
        } else {
            self.feedback = Feedback::Wrong {
                expected: self.current_note.name(),
                got: Note::new(played).name(),
            };
        }
    }

    fn draw_staff(&self, painter: &Painter, rect: Rect) {
        let cx = rect.center().x;
        let cy = rect.center().y;
        let line_spacing = 14.0_f32;
        let staff_width = rect.width() * 0.85;
        let x0 = cx - staff_width / 2.0;
        let x1 = cx + staff_width / 2.0;

        let staff_color = Color32::from_gray(40);
        for i in 0..5i32 {
            let y = cy + (2 - i) as f32 * line_spacing;
            painter.line_segment(
                [Pos2::new(x0, y), Pos2::new(x1, y)],
                Stroke::new(1.5_f32, staff_color),
            );
        }

        // staff_position() 0=C4; E4 is the treble bottom line at staff_position 2.
        let pos = self.current_note.staff_position();
        let bottom_line_y = cy + 2.0 * line_spacing;
        let top_line_y = cy - 2.0 * line_spacing;
        let note_y = bottom_line_y - (pos - 2) as f32 * (line_spacing / 2.0);
        let note_x = cx;
        let note_r = line_spacing * 0.45;
        let ledger_hw = note_r * 2.2;

        // Ledger lines below staff.
        let mut ly = bottom_line_y + line_spacing;
        while ly <= note_y + 0.5 {
            painter.line_segment(
                [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                Stroke::new(1.5_f32, staff_color),
            );
            ly += line_spacing;
        }
        // Ledger lines above staff.
        let mut ly = top_line_y - line_spacing;
        while ly >= note_y - 0.5 {
            painter.line_segment(
                [Pos2::new(note_x - ledger_hw, ly), Pos2::new(note_x + ledger_hw, ly)],
                Stroke::new(1.5_f32, staff_color),
            );
            ly -= line_spacing;
        }

        let note_color = match &self.feedback {
            Feedback::Correct(_) => Color32::from_rgb(50, 180, 80),
            Feedback::Wrong { .. } => Color32::from_rgb(210, 60, 60),
            Feedback::Waiting => Color32::from_gray(30),
        };

        // egui 0.31 Painter has no ellipse_filled; use circle_filled.
        painter.circle_filled(Pos2::new(note_x, note_y), note_r, note_color);

        // Stem up unless note is high on staff (B4 = position 5).
        let stem_up = pos < 5;
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

impl eframe::App for TrainerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Collect MIDI notes before mutably borrowing self for handling.
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

        // Auto-advance after CORRECT_DISPLAY_MS.
        if let Some(t) = self.correct_at {
            let elapsed = t.elapsed();
            let delay = Duration::from_millis(CORRECT_DISPLAY_MS);
            if elapsed >= delay {
                self.next_note();
            } else {
                ctx.request_repaint_after(delay - elapsed);
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading("MIDI Staff Trainer");
                ui.add_space(4.0);
                ui.label(format!(
                    "Score: {}/{} | Range: {} – {}",
                    self.score.correct,
                    self.score.attempts,
                    Note::new(self.config.midi_low).name(),
                    Note::new(self.config.midi_high).name(),
                ));
                ui.add_space(10.0);

                if let Some(ref err) = self.midi_error {
                    ui.colored_label(Color32::RED, format!("MIDI error: {err}"));
                    ui.add_space(4.0);
                    let ports = MidiReceiver::list_ports();
                    if ports.is_empty() {
                        ui.label("No MIDI ports detected.");
                    } else {
                        ui.label("Available ports:");
                        for p in &ports {
                            ui.label(format!("  • {p}"));
                        }
                    }
                }
            });

            let available = ui.available_rect_before_wrap();
            let staff_h = 200.0_f32.min(available.height() * 0.55);
            let staff_rect = Rect::from_center_size(
                Pos2::new(available.center().x, available.top() + staff_h / 2.0 + 10.0),
                Vec2::new(available.width(), staff_h),
            );
            self.draw_staff(ui.painter(), staff_rect);
            ui.add_space(staff_h + 20.0);

            ui.vertical_centered(|ui| {
                match &self.feedback {
                    Feedback::Waiting => {
                        ui.label("Play the note shown on the staff.");
                        if ui.button("Skip").clicked() {
                            self.next_note();
                        }
                    }
                    Feedback::Correct(name) => {
                        ui.colored_label(
                            Color32::from_rgb(50, 180, 80),
                            format!("Correct! ({name}) — next note coming…"),
                        );
                        if ui.button("Next now").clicked() {
                            self.next_note();
                        }
                    }
                    Feedback::Wrong { expected, got } => {
                        let msg = format!("Wrong — expected {expected}, got {got}. Try again.");
                        ui.colored_label(Color32::from_rgb(210, 60, 60), msg);
                        if ui.button("Skip").clicked() {
                            self.next_note();
                        }
                    }
                }
            });
        });
    }
}
