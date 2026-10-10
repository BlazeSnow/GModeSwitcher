use serde::{Deserialize, Serialize};

use crate::log;
use crate::paths;
use crate::thermal::ThermalMode;

#[derive(Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    mode: String,
}

/// The only persisted setting is the thermal mode, stored by name.
pub fn load() -> ThermalMode {
    let Some(path) = paths::data_dir().map(|dir| dir.join("settings.json")) else {
        return ThermalMode::Balanced;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return ThermalMode::Balanced;
    };
    let mode = serde_json::from_str::<Data>(&text)
        .ok()
        .and_then(|data| ThermalMode::from_name(&data.mode));
    match mode {
        Some(mode) => mode,
        None => {
            log::line("settings file unreadable or unknown mode, falling back to Balanced");
            ThermalMode::Balanced
        }
    }
}

pub fn save(mode: ThermalMode) {
    let Some(path) = paths::data_dir().map(|dir| dir.join("settings.json")) else {
        return;
    };
    let data = Data {
        mode: mode.name().to_string(),
    };
    match serde_json::to_string_pretty(&data) {
        Ok(text) => {
            if let Err(error) = std::fs::write(&path, text) {
                log::line(&format!("failed to save settings: {error}"));
            }
        }
        Err(error) => log::line(&format!("failed to serialize settings: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same path `load()` takes over the parsed file: parse, then map the stored name.
    fn mode_from(json: &str) -> Option<ThermalMode> {
        serde_json::from_str::<Data>(json)
            .ok()
            .and_then(|data| ThermalMode::from_name(&data.mode))
    }

    #[test]
    fn stored_mode_name_maps_back_to_the_mode() {
        assert_eq!(mode_from(r#"{"mode":"G_Mode"}"#), Some(ThermalMode::GMode));
    }

    /// `{}` parses because the field is `#[serde(default)]`; it carries no mode name,
    /// which is exactly what makes `load()` fall back to Balanced.
    #[test]
    fn empty_settings_object_carries_no_mode() {
        assert_eq!(mode_from("{}"), None);
    }

    /// Compatibility guarantee for settings files written by older versions.
    #[test]
    fn leftover_custom_mode_is_not_known() {
        assert_eq!(mode_from(r#"{"mode":"Custom"}"#), None);
    }

    /// Pins the on-disk format: the mode is stored as its readable name.
    #[test]
    fn saved_file_contains_the_mode_name() {
        let data = Data {
            mode: ThermalMode::Balanced.name().to_string(),
        };
        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        assert!(
            json.contains(r#""mode": "Balanced""#),
            "unexpected settings format: {json}"
        );
    }
}
