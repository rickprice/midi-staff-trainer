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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_state(path: &PathBuf, state: &AppState) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, toml::to_string_pretty(state).unwrap()).unwrap();
    }

    fn read_state(path: &PathBuf) -> AppState {
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn default_has_no_training_range() {
        let s = AppState::default();
        assert!(s.training_low.is_none());
        assert!(s.training_high.is_none());
    }

    #[test]
    fn roundtrip_with_range() {
        let original = AppState { training_low: Some(48), training_high: Some(72) };
        let text = toml::to_string_pretty(&original).expect("serialise");
        let restored: AppState = toml::from_str(&text).expect("deserialise");
        assert_eq!(restored, original);
    }

    #[test]
    fn roundtrip_without_range() {
        let original = AppState::default();
        let text = toml::to_string_pretty(&original).expect("serialise");
        let restored: AppState = toml::from_str(&text).expect("deserialise");
        assert_eq!(restored, original);
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let path = PathBuf::from("/tmp/midi-staff-trainer-test-missing/state.toml");
        let _ = fs::remove_file(&path);
        let result: AppState = fs::read_to_string(&path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default();
        assert_eq!(result, AppState::default());
    }

    #[test]
    fn save_and_reload_via_toml() {
        let path = PathBuf::from("/tmp/midi-staff-trainer-test-state/state.toml");
        let original = AppState { training_low: Some(41), training_high: Some(65) };
        write_state(&path, &original);
        let restored = read_state(&path);
        assert_eq!(restored, original);
    }
}
