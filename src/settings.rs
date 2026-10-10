use serde::{Deserialize, Serialize};

use crate::log;
use crate::paths;
use crate::thermal::ThermalMode;

#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    mode: String,
    /// Executable path that was registered with the logon task, so a moved
    /// executable can be detected on the next start. `None` while autostart is off
    /// or when the file was written by an older version; it is then left out of the
    /// file entirely, which keeps the old format for everybody who never used it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    autostart_path: Option<String>,
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
    update(|data| set_mode(data, mode));
}

/// Path that was registered with the logon task when autostart was turned on, or
/// `None` when nothing is recorded (never enabled, or an older `settings.json`).
pub fn recorded_autostart_path() -> Option<String> {
    read().and_then(|data| data.autostart_path)
}

/// Remembers the path registered with the logon task; `None` clears the record.
pub fn record_autostart_path(path: Option<&str>) {
    update(|data| set_autostart_path(data, path));
}

fn set_mode(data: &mut Data, mode: ThermalMode) {
    data.mode = mode.name().to_string();
}

fn set_autostart_path(data: &mut Data, path: Option<&str>) {
    data.autostart_path = path.map(str::to_owned);
}

/// Parses the settings file; `None` when it is missing or not parseable.
fn read() -> Option<Data> {
    let path = paths::data_dir()?.join("settings.json");
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read-modify-write of the settings file: fields that are not touched by `change`
/// keep whatever is already on disk, so saving the mode never drops the recorded
/// autostart path and recording a path never drops the mode.
fn update(change: impl FnOnce(&mut Data)) {
    let Some(path) = paths::data_dir().map(|dir| dir.join("settings.json")) else {
        return;
    };
    let mut data = read().unwrap_or_default();
    change(&mut data);
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
        let mut data = Data::default();
        set_mode(&mut data, ThermalMode::Balanced);
        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        assert!(
            json.contains(r#""mode": "Balanced""#),
            "unexpected settings format: {json}"
        );
    }

    /// A file that never recorded an autostart path stays in the old one-field shape.
    #[test]
    fn settings_without_a_recorded_path_keep_the_old_format() {
        let mut data = Data::default();
        set_mode(&mut data, ThermalMode::Balanced);
        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        assert!(
            !json.contains("autostart_path"),
            "unexpected key in: {json}"
        );
    }

    #[test]
    fn mode_and_autostart_path_are_stored_together() {
        let mut data = Data::default();
        set_mode(&mut data, ThermalMode::GMode);
        set_autostart_path(&mut data, Some(r"C:\Tools\GModeSwitcher.exe"));

        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        let reparsed: Data = serde_json::from_str(&json).expect("settings have to parse");
        assert_eq!(reparsed.mode, "G_Mode");
        assert_eq!(
            reparsed.autostart_path.as_deref(),
            Some(r"C:\Tools\GModeSwitcher.exe")
        );
    }

    /// Saving the mode is the most frequent write, and it must not drop the record
    /// that the startup self-healing relies on.
    #[test]
    fn saving_the_mode_keeps_the_recorded_path() {
        let mut data: Data =
            serde_json::from_str(r#"{"mode":"G_Mode","autostart_path":"C:\\Tools\\app.exe"}"#)
                .expect("settings have to parse");

        set_mode(&mut data, ThermalMode::Balanced);

        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        assert!(
            json.contains(r#""mode": "Balanced""#),
            "unexpected mode in: {json}"
        );
        let reparsed: Data = serde_json::from_str(&json).expect("settings have to parse");
        assert_eq!(
            reparsed.autostart_path.as_deref(),
            Some(r"C:\Tools\app.exe")
        );
    }

    /// `disable()` clears the record with `None`.
    #[test]
    fn clearing_the_recorded_path_drops_it() {
        let mut data: Data =
            serde_json::from_str(r#"{"mode":"G_Mode","autostart_path":"C:\\Tools\\app.exe"}"#)
                .expect("settings have to parse");

        set_autostart_path(&mut data, None);

        let json = serde_json::to_string_pretty(&data).expect("settings have to serialize");
        assert!(
            !json.contains("autostart_path"),
            "unexpected key in: {json}"
        );
        assert!(
            json.contains(r#""mode": "G_Mode""#),
            "mode got lost in: {json}"
        );
    }

    /// Files written before the path was recorded must keep loading.
    #[test]
    fn settings_without_autostart_path_still_parse() {
        let data: Data =
            serde_json::from_str(r#"{"mode":"G_Mode"}"#).expect("old settings have to parse");
        assert_eq!(data.mode, "G_Mode");
        assert_eq!(data.autostart_path, None);
    }
}
