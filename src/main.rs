mod app;
mod config;
mod midi;
mod scheduler;
mod staff;
mod state;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MIDI Staff Trainer")
            .with_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MIDI Staff Trainer",
        native_options,
        Box::new(|cc| Ok(Box::new(app::TrainerApp::new(cc)))),
    )
}
