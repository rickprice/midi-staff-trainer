use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppState {
    pub training_low: Option<u8>,
    pub training_high: Option<u8>,
}

impl AppState {
    pub fn load() -> Self {
        let path = state_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<Self>(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(text) = toml::to_string_pretty(self) {
            let path = state_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, text);
        }
    }
}

fn state_path() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".cache")
        .join("midi-staff-trainer")
        .join("state.toml")
}
