use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Lowest MIDI note number your keyboard can send (e.g. 36 = C2 for 61-key)
    pub midi_low: u8,
    /// Highest MIDI note number your keyboard can send (e.g. 96 = C7 for 61-key)
    pub midi_high: u8,
    /// Name of the MIDI input port to use (None = prompt user)
    pub midi_port: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            midi_low: 48,  // C3
            midi_high: 84, // C6
            midi_port: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(cfg) = toml::from_str(&text) {
                    return cfg;
                }
            }
        }
        let default = Self::default();
        default.save();
        default
    }

    pub fn save(&self) {
        if let Ok(text) = toml::to_string_pretty(self) {
            let path = config_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, text);
        }
    }
}

fn config_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".config")
        .join("midi-staff-trainer")
        .join("config.toml")
}
