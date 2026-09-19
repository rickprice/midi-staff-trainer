use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// Lowest MIDI note number your keyboard can send (e.g. 36 = C2 for 61-key).
    pub midi_low: u8,
    /// Highest MIDI note number your keyboard can send (e.g. 96 = C7 for 61-key).
    pub midi_high: u8,
    /// Substring of the MIDI input port name to connect to (`None` → first port).
    pub midi_port: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            midi_low: 41,  // F2 (Roland Cakewalk A300-PRO minimum)
            midi_high: 72, // C5 (Roland Cakewalk A300-PRO maximum)
            midi_port: None,
        }
    }
}

impl Config {
    /// Load config from disk, writing defaults if the file is absent or unparseable.
    pub fn load() -> Self {
        let path = config_path();
        if let Some(cfg) = fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<Self>(&text).ok())
        {
            return cfg;
        }
        let default = Self::default();
        default.save();
        default
    }

    /// Persist config to `~/.config/midi-staff-trainer/config.toml`.
    /// Silently ignores I/O errors (best-effort).
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Config::default ───────────────────────────────────────────────────────

    #[test]
    fn default_midi_range() {
        let cfg = Config::default();
        assert_eq!(cfg.midi_low, 41);  // F2
        assert_eq!(cfg.midi_high, 72); // C5
    }

    #[test]
    fn default_no_port() {
        assert!(Config::default().midi_port.is_none());
    }

    #[test]
    fn default_range_is_valid() {
        let cfg = Config::default();
        assert!(cfg.midi_low < cfg.midi_high, "midi_low must be less than midi_high");
    }

    // ── TOML round-trip ───────────────────────────────────────────────────────

    #[test]
    fn roundtrip_preserves_all_fields() {
        let original = Config {
            midi_low: 36,
            midi_high: 96,
            midi_port: Some("My Keyboard".to_string()),
        };
        let text = toml::to_string_pretty(&original).expect("serialise");
        let restored: Config = toml::from_str(&text).expect("deserialise");
        assert_eq!(restored, original);
    }

    #[test]
    fn roundtrip_none_port() {
        let original = Config::default();
        let text = toml::to_string_pretty(&original).expect("serialise");
        let restored: Config = toml::from_str(&text).expect("deserialise");
        assert_eq!(restored.midi_port, None);
    }

    // ── Partial TOML (graceful defaults) ─────────────────────────────────────

    #[test]
    fn partial_toml_overrides_only_specified_fields() {
        let text = "midi_low = 36\nmidi_high = 96\n";
        let cfg: Config = toml::from_str(text).expect("deserialise");
        assert_eq!(cfg.midi_low, 36);
        assert_eq!(cfg.midi_high, 96);
        assert!(cfg.midi_port.is_none());
    }

    #[test]
    fn toml_with_port_parses_correctly() {
        let text = r#"
midi_low = 48
midi_high = 84
midi_port = "Alesis Q88"
"#;
        let cfg: Config = toml::from_str(text).expect("deserialise");
        assert_eq!(cfg.midi_port.as_deref(), Some("Alesis Q88"));
    }

    #[test]
    fn toml_port_none_when_key_absent() {
        let text = "midi_low = 48\nmidi_high = 84\n";
        let cfg: Config = toml::from_str(text).expect("deserialise");
        assert!(cfg.midi_port.is_none());
    }

    // ── Edge-case values ──────────────────────────────────────────────────────

    #[test]
    fn min_max_midi_values_survive_roundtrip() {
        let cfg = Config { midi_low: 0, midi_high: 127, midi_port: None };
        let text = toml::to_string_pretty(&cfg).expect("serialise");
        let restored: Config = toml::from_str(&text).expect("deserialise");
        assert_eq!(restored.midi_low, 0);
        assert_eq!(restored.midi_high, 127);
    }
}
