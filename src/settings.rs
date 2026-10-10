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
